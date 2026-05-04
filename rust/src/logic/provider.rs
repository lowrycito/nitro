//! Provider auth file (`~/.nitro/auth.json`).
//!
//! Mirrors `src/logic/provider.ts`. Same wire format so a default provider
//! configured via the TypeScript binary works under the Rust binary, and
//! vice versa.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::config::{ensure_app_data_dir, set_file_mode_600};

pub const AUTH_FILENAME: &str = "auth.json";

/// API flavour. Wire values match `ApiTypeSchema` from the TS code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiType {
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    #[serde(rename = "openai-responses")]
    OpenAiResponses,
    #[serde(rename = "anthropic")]
    Anthropic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderInfo {
    #[serde(rename = "baseURL")]
    pub base_url: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub model: String,
    #[serde(rename = "apiType")]
    pub api_type: ApiType,
}

/// A provider with its name attached. Convenience alias used by the LLM
/// layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedProvider {
    pub name: String,
    pub info: ProviderInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Auth {
    #[serde(default, rename = "defaultProvider")]
    pub default_provider: Option<String>,
    /// `BTreeMap` so the on-disk JSON has stable key ordering across runs;
    /// makes diffs and tests deterministic.
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderInfo>,
}

pub fn auth_path(data_dir: &Path) -> PathBuf {
    data_dir.join(AUTH_FILENAME)
}

pub fn load_auth(data_dir: &Path) -> io::Result<Auth> {
    ensure_app_data_dir(data_dir)?;
    let path = auth_path(data_dir);
    if !path.exists() {
        let auth = Auth::default();
        save_auth(data_dir, &auth)?;
        return Ok(auth);
    }
    let content = fs::read_to_string(&path)?;
    match serde_json::from_str::<Auth>(&content) {
        Ok(auth) => {
            set_file_mode_600(&path)?;
            Ok(auth)
        }
        Err(_) => {
            let auth = Auth::default();
            save_auth(data_dir, &auth)?;
            Ok(auth)
        }
    }
}

pub fn save_auth(data_dir: &Path, auth: &Auth) -> io::Result<()> {
    ensure_app_data_dir(data_dir)?;
    let path = auth_path(data_dir);
    let json = serde_json::to_string_pretty(auth).map_err(io::Error::other)?;
    fs::write(&path, json)?;
    set_file_mode_600(&path)?;
    Ok(())
}

pub fn list_providers(data_dir: &Path) -> io::Result<Vec<String>> {
    Ok(load_auth(data_dir)?.providers.keys().cloned().collect())
}

pub fn get_provider(data_dir: &Path, name: &str) -> io::Result<Option<ProviderInfo>> {
    Ok(load_auth(data_dir)?.providers.get(name).cloned())
}

pub fn get_default_provider(data_dir: &Path) -> io::Result<Option<NamedProvider>> {
    let auth = load_auth(data_dir)?;
    let Some(name) = auth.default_provider.clone() else {
        return Ok(None);
    };
    Ok(auth
        .providers
        .get(&name)
        .cloned()
        .map(|info| NamedProvider { name, info }))
}

/// Set the default provider. Returns `false` (not an error) if the named
/// provider doesn't exist, matching TS semantics.
pub fn set_default_provider(data_dir: &Path, name: &str) -> io::Result<bool> {
    let mut auth = load_auth(data_dir)?;
    if !auth.providers.contains_key(name) {
        return Ok(false);
    }
    auth.default_provider = Some(name.to_string());
    save_auth(data_dir, &auth)?;
    Ok(true)
}

pub fn set_provider(data_dir: &Path, name: &str, info: ProviderInfo) -> io::Result<()> {
    let mut auth = load_auth(data_dir)?;
    auth.providers.insert(name.to_string(), info);
    save_auth(data_dir, &auth)
}

/// Remove a provider. If it was the current default, the default is cleared.
/// Returns `false` if the named provider didn't exist.
pub fn remove_provider(data_dir: &Path, name: &str) -> io::Result<bool> {
    let mut auth = load_auth(data_dir)?;
    if auth.providers.remove(name).is_none() {
        return Ok(false);
    }
    if auth.default_provider.as_deref() == Some(name) {
        auth.default_provider = None;
    }
    save_auth(data_dir, &auth)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample(api_type: ApiType) -> ProviderInfo {
        ProviderInfo {
            base_url: "https://example.com/v1".into(),
            api_key: "sk-test".into(),
            model: "model-x".into(),
            api_type,
        }
    }

    #[test]
    fn missing_file_creates_default() {
        let tmp = TempDir::new().unwrap();
        let auth = load_auth(tmp.path()).unwrap();
        assert_eq!(auth, Auth::default());
        assert!(auth_path(tmp.path()).exists());
    }

    #[test]
    fn set_get_remove_roundtrip() {
        let tmp = TempDir::new().unwrap();
        set_provider(tmp.path(), "openai", sample(ApiType::OpenAiResponses)).unwrap();
        assert_eq!(
            list_providers(tmp.path()).unwrap(),
            vec!["openai".to_string()]
        );
        let got = get_provider(tmp.path(), "openai").unwrap().unwrap();
        assert_eq!(got.api_type, ApiType::OpenAiResponses);
        assert!(remove_provider(tmp.path(), "openai").unwrap());
        assert!(get_provider(tmp.path(), "openai").unwrap().is_none());
    }

    #[test]
    fn set_default_requires_existing_provider() {
        let tmp = TempDir::new().unwrap();
        assert!(!set_default_provider(tmp.path(), "missing").unwrap());
        set_provider(tmp.path(), "anthropic", sample(ApiType::Anthropic)).unwrap();
        assert!(set_default_provider(tmp.path(), "anthropic").unwrap());
        let def = get_default_provider(tmp.path()).unwrap().unwrap();
        assert_eq!(def.name, "anthropic");
    }

    #[test]
    fn removing_default_clears_default() {
        let tmp = TempDir::new().unwrap();
        set_provider(tmp.path(), "groq", sample(ApiType::OpenAiCompatible)).unwrap();
        set_default_provider(tmp.path(), "groq").unwrap();
        remove_provider(tmp.path(), "groq").unwrap();
        let auth = load_auth(tmp.path()).unwrap();
        assert!(auth.default_provider.is_none());
        assert!(get_default_provider(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn wire_format_uses_camel_case_for_provider_fields() {
        let tmp = TempDir::new().unwrap();
        set_provider(tmp.path(), "openai", sample(ApiType::OpenAiCompatible)).unwrap();
        let raw = fs::read_to_string(auth_path(tmp.path())).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let p = &v["providers"]["openai"];
        assert!(p.get("baseURL").is_some());
        assert!(p.get("apiKey").is_some());
        assert!(p.get("apiType").is_some());
        assert_eq!(p["apiType"], "openai-compatible");
    }

    #[test]
    fn wire_format_default_provider_key_matches_ts() {
        let tmp = TempDir::new().unwrap();
        set_provider(tmp.path(), "openai", sample(ApiType::OpenAiCompatible)).unwrap();
        set_default_provider(tmp.path(), "openai").unwrap();
        let raw = fs::read_to_string(auth_path(tmp.path())).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["defaultProvider"], "openai");
    }

    #[test]
    fn malformed_auth_is_replaced_with_default() {
        let tmp = TempDir::new().unwrap();
        ensure_app_data_dir(tmp.path()).unwrap();
        fs::write(auth_path(tmp.path()), "garbage").unwrap();
        let auth = load_auth(tmp.path()).unwrap();
        assert_eq!(auth, Auth::default());
    }
}
