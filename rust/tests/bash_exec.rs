//! Real shell-execution tests for the Bash tool.
//!
//! These tests *intentionally* call [`enable_execution`] so they can verify
//! the line-prefix, exit-code, and timeout behaviour against a real `bash`
//! subprocess. To keep them safe:
//! - every test runs in its own [`TempDir`] working directory and only
//!   operates on files inside that directory
//! - tests are gated to Unix where `bash` is reasonably guaranteed
//! - the only side effect outside the temp dir is reading well-known
//!   binaries (`bash`, `sleep`, `false`, `pwd`)
//!
//! NOTE: These tests run in a separate test binary from the in-crate
//! `tools::bash::tests` module so the `EXECUTION_ENABLED` flag never leaks
//! into the unit tests where we assert it stays off.

#![cfg(unix)]

use std::time::Duration;

use nitro::tools::bash::{
    enable_execution, run_for_test, BashApproval, BashModelInput, BashTool, BashToolOutput,
    BehaviorTag, RiskLevel, TIMEOUT_EXIT_CODE,
};
use tempfile::TempDir;

fn input(cmd: &str, timeout_ms: u64) -> BashModelInput {
    BashModelInput {
        command: cmd.to_string(),
        explanation: "test".to_string(),
        risk_level: RiskLevel::ReadOnly,
        behavior_tags: vec![BehaviorTag::Safe],
        timeout: timeout_ms,
    }
}

#[tokio::test]
async fn echo_writes_stdout_with_out_prefix() {
    enable_execution();
    let tmp = TempDir::new().unwrap();
    let (raw, exit) = run_for_test("echo hello", Duration::from_millis(5_000), tmp.path()).await;
    assert_eq!(exit, 0);
    assert!(raw.contains("out:\thello"), "got: {raw}");
}

#[tokio::test]
async fn stderr_uses_err_prefix() {
    enable_execution();
    let tmp = TempDir::new().unwrap();
    let (raw, exit) = run_for_test(
        "echo via-stderr 1>&2",
        Duration::from_millis(5_000),
        tmp.path(),
    )
    .await;
    assert_eq!(exit, 0);
    assert!(raw.contains("err:\tvia-stderr"), "got: {raw}");
}

#[tokio::test]
async fn nonzero_exit_is_propagated() {
    enable_execution();
    let tmp = TempDir::new().unwrap();
    let (_raw, exit) = run_for_test("exit 7", Duration::from_millis(5_000), tmp.path()).await;
    assert_eq!(exit, 7);
}

#[tokio::test]
async fn timeout_kills_process_and_returns_124() {
    enable_execution();
    let tmp = TempDir::new().unwrap();
    let (raw, exit) = run_for_test("sleep 5", Duration::from_millis(150), tmp.path()).await;
    assert_eq!(exit, TIMEOUT_EXIT_CODE);
    assert!(raw.contains("Tool Error: Command timed out"), "got: {raw}");
}

#[tokio::test]
async fn execute_runs_command_when_approved_and_enabled() {
    enable_execution();
    // BashTool::execute uses the current working directory; do the work
    // inside a TempDir so we don't dirty the repo.
    let tmp = TempDir::new().unwrap();
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let result = BashTool::execute(
        &input("touch from-rust && echo created", 5_000),
        BashApproval::Approved,
    )
    .await;
    let restored = std::env::set_current_dir(&prev);
    assert!(restored.is_ok());

    match result {
        BashToolOutput::Approved {
            command_output,
            exit_code,
            ..
        } => {
            assert_eq!(exit_code, 0);
            assert!(command_output.contains("out:\tcreated"));
            assert!(tmp.path().join("from-rust").exists());
        }
        other => panic!("expected Approved, got {other:?}"),
    }
}
