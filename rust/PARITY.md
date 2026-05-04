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
| 0     | Cargo scaffold + CLI dispatch parity   | [~]     |
| 1     | Pure-data layer                         | [ ]     |
| 2     | Bash tool execution + safety classifier | [ ]     |
| 3     | LLM clients (3 backends)                | [ ]     |
| 4     | Headless one-shot CLI end-to-end        | [ ]     |
| 5     | ratatui scaffold + EULA + Settings      | [ ]     |
| 6     | Provider screens                        | [ ]     |
| 7     | Chat screen                             | [ ]     |
| 8     | Cutover + polish                        | [ ]     |

## Feature inventory

### CLI surface (`src/index.ts`)

| Feature                                             | TS | Rust |
| --------------------------------------------------- | -- | ---- |
| `nitro` (no args) prints usage                      | x  | x    |
| `nitro help`                                        | x  | x    |
| `nitro "<request>"` (multi-word) → one-shot         | x  | [~]  |
| `nitro interactive [req]` / `nitro i [req]`         | x  | [~]  |
| `nitro continue <req>` / `c <req>`                  | x  | [~]  |
| `nitro continue` without request → error + exit 1   | x  | x    |
| `nitro resume [req]` / `r [req]`                    | x  | [~]  |
| `nitro strict [req]` / `s [req]`                    | x  | [~]  |
| `nitro settings`                                    | x  | [~]  |
| `nitro provider <subcommand>`                       | x  | [~]  |
| Unknown command → red error + usage                 | x  | x    |
| EULA gate before any command                        | x  | [ ]  |

### Data layer (`src/logic/`)

| File                            | Rust module          | Status |
| ------------------------------- | -------------------- | ------ |
| `config.ts` (APP_DATA_DIR, perms) | `logic::config`      | [ ]    |
| `eula.ts` (version + text)        | `logic::eula`        | [ ]    |
| `settings.ts` (load/save/schema)  | `logic::settings`    | [ ]    |
| `provider.ts` (auth.json CRUD)    | `logic::provider`    | [ ]    |
| `defaultProviders.ts` (presets)   | `logic::defaults`    | [ ]    |
| `defaultProviders.ts::fetchModels`| `logic::defaults`    | [ ]    |
| `conversation.ts` (chats/state)   | `logic::conversation`| [ ]    |
| `llm.ts` (3 clients + streaming)  | `logic::llm`         | [ ]    |

### Tools (`src/tools/`)

| Tool         | Rust module     | Status |
| ------------ | --------------- | ------ |
| Bash exec    | `tools::bash`   | [ ]    |
| Bash safety  | `tools::bash`   | [ ]    |
| AskUser      | `tools::ask`    | [ ]    |
| Tool dispatch | `tools` (top)  | [ ]    |

### Screens (`src/screens/`)

| Screen           | Rust module                | Status |
| ---------------- | -------------------------- | ------ |
| EulaScreen       | `screens::eula`            | [ ]    |
| SettingsScreen   | `screens::settings`        | [ ]    |
| ProviderRouter   | `screens::provider_router` | [ ]    |
| ProviderList     | `screens::provider_list`   | [ ]    |
| ProviderAdd      | `screens::provider_add`    | [ ]    |
| ProviderEdit     | `screens::provider_edit`   | [ ]    |
| ProviderRemove   | `screens::provider_remove` | [ ]    |
| ProviderDefault  | `screens::provider_default`| [ ]    |
| ChatScreen       | `screens::chat`            | [ ]    |

### TUI components (`src/components/`)

| Component        | Notes                                             | Status |
| ---------------- | ------------------------------------------------- | ------ |
| Message          | Render assistant/user/tool messages               | [ ]    |
| ChatBox          | Streaming text + scrollback                       | [ ]    |
| ToolDisplay      | Render tool calls + results                       | [ ]    |
| BashPrompt       | Approve/reject bash command modal                 | [ ]    |
| AskPrompt        | Multi-question modal                              | [ ]    |
| TokenUsageContext| Token totals across the session                   | [ ]    |
| Custom widgets   | TextInput, Text, Select                           | [ ]    |

### Tests to mirror

| TS test file                       | Rust counterpart                  | Status |
| ---------------------------------- | --------------------------------- | ------ |
| `tests/cli.test.ts`                | `rust/src/cli.rs::tests`          | [x]    |
| `tests/config.test.ts`             | `rust/tests/config.rs`            | [ ]    |
| `tests/settings.test.tsx` (logic)  | `rust/tests/settings.rs`          | [ ]    |
| `tests/provider.test.ts`           | `rust/tests/provider.rs`          | [ ]    |
| `tests/conversation.test.ts`       | `rust/tests/conversation.rs`      | [ ]    |
| `tests/llm.test.ts`                | `rust/tests/llm.rs`               | [ ]    |
| `tests/bash.test.tsx`              | `rust/tests/bash.rs`              | [ ]    |
| `tests/tool.test.tsx`              | `rust/tests/tool.rs`              | [ ]    |
| `tests/question.test.tsx`          | `rust/tests/question.rs`          | [ ]    |
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
