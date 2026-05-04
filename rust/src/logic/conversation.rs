//! Conversation persistence (`~/.nitro/chats/*.json` and `state.json`).
//!
//! Mirrors `src/logic/conversation.ts`. Messages are stored as raw
//! `serde_json::Value` so the in-flight model-message format from the AI SDK
//! roundtrips losslessly even before the LLM layer typed messages exist.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::config::{ensure_app_data_dir, set_file_mode_600};

pub const CHATS_DIRNAME: &str = "chats";
pub const STATE_FILENAME: &str = "state.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Conversation {
    #[serde(default)]
    pub messages: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StateFile {
    #[serde(default, rename = "lastConversation")]
    pub last_conversation: Option<String>,
}

pub fn chats_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(CHATS_DIRNAME)
}

pub fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(STATE_FILENAME)
}

pub fn ensure_chats_dir(data_dir: &Path) -> io::Result<()> {
    ensure_app_data_dir(data_dir)?;
    let dir = chats_dir(data_dir);
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

/// Mirrors the `Math.random()` 8-hex generator in TS. We don't pull in a PRNG
/// crate; mixing the high-resolution clock through a Mulberry/Knuth multiplier
/// gives plenty of spread for the timestamp-collision use case.
fn random_hex() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let mixed = nanos.wrapping_mul(2_654_435_761);
    format!("{:08x}", mixed)
}

fn generate_filename(dir: &Path) -> String {
    let timestamp = now_millis();
    let mut filename = format!("{timestamp}.json");
    let max_attempts = 10;
    let mut attempts = 0;
    while dir.join(&filename).exists() {
        if attempts >= max_attempts {
            return filename;
        }
        filename = format!("{timestamp}-{}.json", random_hex());
        attempts += 1;
    }
    filename
}

fn load_state_file(data_dir: &Path) -> StateFile {
    let path = state_path(data_dir);
    if !path.exists() {
        return StateFile::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

fn save_state_file(data_dir: &Path, state: &StateFile) -> io::Result<()> {
    let path = state_path(data_dir);
    let json = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
    fs::write(&path, json)?;
    set_file_mode_600(&path)
}

/// Save a conversation. If `existing_filename` is `Some`, overwrite that
/// file; otherwise pick a fresh timestamp-based name. Returns the filename
/// (relative to the chats directory) that was written. Updates
/// `state.json::lastConversation` to point at it.
pub fn save_conversation(
    data_dir: &Path,
    messages: &[serde_json::Value],
    existing_filename: Option<&str>,
) -> io::Result<String> {
    ensure_chats_dir(data_dir)?;
    let dir = chats_dir(data_dir);
    let filename = match existing_filename {
        Some(name) => name.to_string(),
        None => generate_filename(&dir),
    };
    let filepath = dir.join(&filename);

    if existing_filename.is_none() && filepath.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Failed to create a new conversation file.",
        ));
    }

    let conversation = Conversation {
        messages: messages.to_vec(),
    };
    let json = serde_json::to_string_pretty(&conversation).map_err(io::Error::other)?;
    fs::write(&filepath, json)?;
    set_file_mode_600(&filepath)?;

    save_state_file(
        data_dir,
        &StateFile {
            last_conversation: Some(filename.clone()),
        },
    )?;

    Ok(filename)
}

pub fn load_conversation(data_dir: &Path, filename: &str) -> Option<Conversation> {
    let path = chats_dir(data_dir).join(filename);
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn last_conversation_filename(data_dir: &Path) -> Option<String> {
    load_state_file(data_dir).last_conversation
}

pub fn load_last_conversation(data_dir: &Path) -> Option<Conversation> {
    let name = last_conversation_filename(data_dir)?;
    load_conversation(data_dir, &name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn save_creates_file_and_updates_state() {
        let tmp = TempDir::new().unwrap();
        let messages = vec![json!({ "role": "user", "content": "hi" })];
        let name = save_conversation(tmp.path(), &messages, None).unwrap();
        assert!(chats_dir(tmp.path()).join(&name).exists());
        assert_eq!(
            last_conversation_filename(tmp.path()).as_deref(),
            Some(name.as_str())
        );
    }

    #[test]
    fn load_round_trip_preserves_messages() {
        let tmp = TempDir::new().unwrap();
        let messages = vec![
            json!({ "role": "user", "content": "hi" }),
            json!({ "role": "assistant", "content": [{ "type": "text", "text": "hello" }] }),
        ];
        let name = save_conversation(tmp.path(), &messages, None).unwrap();
        let loaded = load_conversation(tmp.path(), &name).unwrap();
        assert_eq!(loaded.messages, messages);
    }

    #[test]
    fn save_with_existing_filename_overwrites() {
        let tmp = TempDir::new().unwrap();
        let m1 = vec![json!({ "role": "user", "content": "one" })];
        let name = save_conversation(tmp.path(), &m1, None).unwrap();
        let m2 = vec![json!({ "role": "user", "content": "two" })];
        let name2 = save_conversation(tmp.path(), &m2, Some(&name)).unwrap();
        assert_eq!(name, name2);
        let loaded = load_conversation(tmp.path(), &name).unwrap();
        assert_eq!(loaded.messages, m2);
    }

    #[test]
    fn load_missing_returns_none() {
        let tmp = TempDir::new().unwrap();
        ensure_chats_dir(tmp.path()).unwrap();
        assert!(load_conversation(tmp.path(), "nope.json").is_none());
    }

    #[test]
    fn last_conversation_none_when_state_missing() {
        let tmp = TempDir::new().unwrap();
        ensure_app_data_dir_for_test(tmp.path());
        assert!(last_conversation_filename(tmp.path()).is_none());
    }

    #[test]
    fn state_wire_uses_camel_case_key() {
        let tmp = TempDir::new().unwrap();
        let messages = vec![json!({ "role": "user", "content": "hi" })];
        save_conversation(tmp.path(), &messages, None).unwrap();
        let raw = fs::read_to_string(state_path(tmp.path())).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(v.get("lastConversation").is_some());
    }

    fn ensure_app_data_dir_for_test(p: &Path) {
        super::super::config::ensure_app_data_dir(p).unwrap();
    }
}
