# Nitro — Rust port

A faithful Rust port of the [Nitro](../README.md) CLI. The Rust binary
shares `~/.nitro/` with the upstream TypeScript binary so users can swap
between the two at any time without losing settings, providers, or
conversation history.

## Status

See [`PARITY.md`](./PARITY.md) for the running checklist of TS → Rust
feature parity. As of the latest commit on this branch, every TS feature
the project ships has a Rust counterpart:

| Phase | Title                                  |
| ----- | -------------------------------------- |
| 0     | Cargo scaffold + CLI dispatch parity   |
| 1     | Pure-data layer                         |
| 2     | Bash tool execution + safety classifier |
| 3     | LLM clients (openai-compatible, anthropic; openai-responses routes through chat-completions) |
| 4     | Headless one-shot CLI end-to-end        |
| 5     | ratatui scaffold + EULA + Settings      |
| 6     | Provider management screens             |
| 7     | Chat screen with streaming + tool modals |

## Build

```bash
cd rust
cargo build --release
./target/release/nitro --help    # not actually a flag — prints usage anyway
./target/release/nitro "find all rs files and count their lines"
```

## Run from source

```bash
cargo run -- "<request>"
cargo run -- interactive
cargo run -- provider add
cargo run -- settings
```

## Layout

```
rust/
├── Cargo.toml
├── PARITY.md              # TS → Rust feature checklist
├── src/
│   ├── main.rs            # binary entry; the *only* call site of
│   │                      # tools::bash::enable_execution
│   ├── lib.rs
│   ├── cli.rs             # argv parsing (mirrors src/index.ts)
│   ├── app/               # orchestrator: dispatches Command -> screens / chat
│   ├── logic/             # pure-data layer — no UI
│   │   ├── config.rs
│   │   ├── conversation.rs
│   │   ├── defaults.rs
│   │   ├── eula.rs
│   │   ├── llm/           # 3 provider adapters + SSE parser + transcripts
│   │   ├── provider.rs
│   │   └── settings.rs
│   ├── tools/
│   │   ├── ask.rs
│   │   └── bash.rs        # safety guard + truncation; same sentinel as TS
│   └── screens/
│       ├── app_shell.rs   # raw-mode + alt-screen lifecycle, panic hook
│       ├── eula_screen.rs
│       ├── settings_screen.rs
│       ├── provider_screens.rs
│       └── chat_screen/   # streaming chat + Bash/Ask modals
├── tests/                 # integration tests against wiremock + tempfile
│   ├── bash_exec.rs       # real shell, in TempDir; Unix-gated
│   ├── format_compat.rs   # TS-fixture-backed round-trip checks
│   ├── headless_chat.rs   # full one-shot path through wiremock'd LLM
│   └── llm_streaming.rs   # SSE + tool-call decoding for both adapters
└── target/                # cargo build artefacts (gitignored)
```

## Safety

The Bash tool's `EXECUTION_ENABLED` flag defaults to `false`. The binary
opts in once at `main()` startup; tests must never touch it. When the
flag is off, every "approved" execution returns the
`[EXECUTION DISABLED] Command was not executed.` sentinel — same string
the TypeScript app uses, so prompt-side parity is preserved across
either binary.

Integration tests that need real shell execution
(`tests/bash_exec.rs`) call a `#[doc(hidden)]` test helper that scopes
each invocation to a `TempDir`. They run in their own integration-test
binary so the unit tests never see the flag flip.

## File-format compatibility

The Rust port reads and writes the same JSON shape as the TypeScript
app for every persisted file:

- `~/.nitro/settings.json`
- `~/.nitro/auth.json`
- `~/.nitro/state.json`
- `~/.nitro/chats/*.json`
- `~/.nitro/system_prompt_template.md`

`tests/format_compat.rs` pins the wire shape against fixtures
hand-written from the TS schemas. Future TS-side additions (unknown
fields) are tolerated; future Rust-side additions are gated behind
`#[serde(default)]` to keep older readers happy.
