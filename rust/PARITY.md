# Nitro TS → Rust parity tracker

The Rust port grows alongside the existing TypeScript app under `rust/`.
Both binaries share `~/.nitro/` so users can swap between them at any time.
This file is the authoritative checklist — a feature only flips to
`verified` once a Rust test covers it AND a side-by-side smoke against the
TypeScript binary matches.

## Legend

- `[ ]` — not started
- `[~]` — in progress / partial
- `[x]` — implemented in Rust
- `[v]` — verified against TypeScript (tests + manual smoke)

## Phase status

| Phase | Title                                  | Status  |
| ----- | -------------------------------------- | ------- |
| 0     | Cargo scaffold + CLI dispatch parity   | [x]     |
| 1     | Pure-data layer                         | [x]     |
| 2     | Bash tool execution + safety classifier | [x]     |
| 3     | LLM clients (3 backends)                | [x]     |
| 4     | Headless one-shot CLI end-to-end        | [x]     |
| 5     | ratatui scaffold + EULA + Settings      | [x]     |
| 6     | Provider screens                        | [x]     |
| 7     | Chat screen                             | [x]     |
| 8     | Cutover + polish                        | [ ]     |

## Feature inventory

### CLI surface (`src/index.ts`)

| Feature                                             | TS | Rust |
| --------------------------------------------------- | -- | ---- |
| `nitro` (no args) prints usage                      | x  | x    |
| `nitro help`                                        | x  | x    |
| `nitro "<request>"` (multi-word) → one-shot         | x  | x    |
| `nitro interactive [req]` / `nitro i [req]`         | x  | [~] (Phase 7) |
| `nitro continue <req>` / `c <req>`                  | x  | x    |
| `nitro continue` without request → error + exit 1   | x  | x    |
| `nitro resume [req]` / `r [req]`                    | x  | [~] (Phase 7) |
| `nitro strict [req]` / `s [req]`                    | x  | x    |
| `nitro settings`                                    | x  | [~] (Phase 5) |
| `nitro provider list`                               | x  | x    |
| `nitro provider add\|edit\|remove\|default`         | x  | [~] (Phase 6) |
| Unknown command → red error + usage                 | x  | x    |
| EULA gate before any command                        | x  | x    |

### Data layer (`src/logic/`)

| File                            | Rust module          | Status |
| ------------------------------- | -------------------- | ------ |
| `config.ts` (APP_DATA_DIR, perms) | `logic::config`      | x      |
| `eula.ts` (version + text)        | `logic::eula`        | x      |
| `settings.ts` (load/save/schema)  | `logic::settings`    | x      |
| `provider.ts` (auth.json CRUD)    | `logic::provider`    | x      |
| `defaultProviders.ts` (presets)   | `logic::defaults`    | x      |
| `defaultProviders.ts::fetchModels`| `logic::llm::fetch_models` | x  |
| `conversation.ts` (chats/state)   | `logic::conversation`| x      |
| `llm.ts` (3 clients + streaming)  | `logic::llm`         | x (responses=alias for compat; native responses-API in Phase 8) |

### Tools (`src/tools/`)

| Tool         | Rust module     | Status |
| ------------ | --------------- | ------ |
| Bash exec    | `tools::bash`   | x      |
| Bash safety  | `tools::bash`   | x      |
| AskUser      | `tools::ask`    | x      |
| Tool dispatch | `tools` (top)  | x      |

### Screens (`src/screens/`)

| Screen           | Rust module                | Status |
| ---------------- | -------------------------- | ------ |
| EulaScreen       | `screens::eula_screen`     | x      |
| SettingsScreen   | `screens::settings_screen` | x      |
| ProviderRouter   | `app::provider_router`     | x      |
| ProviderList     | `screens::provider_screens::ProviderListScreen` | x |
| ProviderAdd      | `screens::provider_screens::ProviderWizardScreen` | x |
| ProviderEdit     | `screens::provider_screens::ProviderWizardScreen` | x |
| ProviderRemove   | `screens::provider_screens::PickProviderScreen`   | x |
| ProviderDefault  | `screens::provider_screens::PickProviderScreen`   | x |
| ChatScreen       | `screens::chat_screen`     | x      |

### TUI components (`src/components/`)

| Component        | Notes                                             | Status |
| ---------------- | ------------------------------------------------- | ------ |
| Message          | Render assistant/user/tool messages               | x      |
| ChatBox          | Streaming text + scrollback                       | x      |
| ToolDisplay      | Render tool calls + results                       | x      |
| BashPrompt       | Approve/reject bash command modal                 | x      |
| AskPrompt        | Multi-question modal                              | x      |
| TokenUsageContext| Token totals across the session                   | [~] (Phase 8 polish) |
| Custom widgets   | TextInput, Text, Select                           | x (inline)           |

### Tests to mirror

| TS test file                       | Rust counterpart                  | Status |
| ---------------------------------- | --------------------------------- | ------ |
| `tests/cli.test.ts`                | `rust/src/cli.rs::tests`          | x      |
| `tests/config.test.ts`             | `rust/src/logic/config.rs::tests` | x      |
| `tests/settings.test.tsx` (logic)  | `rust/src/logic/settings.rs` + `tests/format_compat.rs` | x |
| `tests/provider.test.ts`           | `rust/src/logic/provider.rs` + `tests/format_compat.rs` | x |
| `tests/conversation.test.ts`       | `rust/src/logic/conversation.rs` + `tests/format_compat.rs` | x |
| `tests/llm.test.ts`                | `rust/tests/llm_streaming.rs` + provider unit tests | x |
| `tests/bash.test.tsx` (logic)      | `rust/tests/bash_exec.rs` + `tools::bash::tests` | x |
| `tests/tool.test.tsx`              | `rust/src/tools/bash.rs::tests` (output schemas) | x |
| `tests/question.test.tsx` (logic)  | `rust/src/tools/ask.rs::tests`    | x      |
| `tests/providers.test.tsx` (UI)    | `rust/tests/provider_ui.rs`       | [ ]    |
| `tests/ui/BashPromptTest.tsx`      | `rust/tests/ui/bash_prompt.rs`    | [ ]    |
| `tests/ui/AskPromptTest.tsx`       | `rust/tests/ui/ask_prompt.rs`     | [ ]    |

### File-format compatibility checks (Phase 1 must satisfy all)

- `~/.nitro/settings.json` — TS-written → Rust-read → identical struct, and
  vice versa. Mode `0o600`.
- `~/.nitro/auth.json` — same.
- `~/.nitro/state.json` — same.
- `~/.nitro/chats/<timestamp>.json` — same; messages roundtrip with no loss.
- `~/.nitro/system_prompt_template.md` — re-written on every load, content
  byte-identical between TS and Rust.
