//! The JSON surfaces resolve a spelling to a verb once, and every question is asked of the
//! verb. Both dispatch matches are exhaustive over it, so a command that one surface handles
//! and the other does not is a compile error rather than a runtime surprise.
//!
//! What is left to test from outside is the wording a caller actually sees.

use std::process::Command;

use serde_json::Value;

mod common;
use common::{binary, run_cli};

struct TestBrowser(String);
impl TestBrowser {
    fn new(label: &str) -> Self {
        Self(format!("{label}-{}", std::process::id()))
    }
    fn name(&self) -> &str {
        &self.0
    }
}
impl Drop for TestBrowser {
    fn drop(&mut self) {
        let _ = run_cli(&["--browser", &self.0, "close", "--purge"]);
    }
}

fn pipe(browser: &str, lines: &[&str]) -> Vec<Value> {
    let mut child = Command::new(binary())
        .args(["--browser", browser, "pipe"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn pipe");
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("pipe stdin");
        for line in lines {
            writeln!(stdin, "{line}").expect("write");
        }
    }
    let out = child.wait_with_output().expect("output");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}")))
        .collect()
}

/// `batch` is a known command that is simply not nestable, and it used to fall through the
/// same arm an unknown word does. The recovery for "unknown command" is to check the
/// spelling; the recovery here is to hoist the commands into the outer batch.
#[test]
fn a_nested_batch_is_refused_by_name_not_as_an_unknown_word() {
    if !common::browser_ready() {
        return;
    }
    let b = TestBrowser::new("vocab-batch");
    let out = pipe(
        b.name(),
        &[r#"{"cmd":"batch","commands":[{"cmd":"batch","commands":[]}]}"#],
    );
    let inner = &out[0]["results"][0];
    let err = inner["error"].as_str().unwrap_or_default();
    assert!(
        !err.contains("Unknown command"),
        "batch is known, it is only not nestable: {inner}"
    );
    assert!(err.contains("nested"), "the error must say why: {inner}");
}

/// The three spellings `mutates_page` used to classify and no dispatcher ever accepted.
/// They are CLI-only clap aliases, and the JSON surfaces take none of clap's convenience
/// aliases, so they stay refused. The point is that the classification can no longer disagree
/// with the dispatcher about them, because there is only one list now.
#[test]
fn the_cli_only_aliases_are_not_pipe_verbs() {
    if !common::browser_ready() {
        return;
    }
    let b = TestBrowser::new("vocab-alias");
    for dead in ["tap", "double_click", "double-click"] {
        let out = pipe(b.name(), &[&format!(r#"{{"cmd":"{dead}"}}"#)]);
        assert!(
            out[0]["error"].as_str().unwrap_or_default().contains("Unknown command"),
            "{dead} should not resolve: {}",
            out[0]
        );
    }
}

/// A request with no `cmd` at all keeps its own message rather than being reported as an
/// unknown command named "".
#[test]
fn a_missing_cmd_field_says_so() {
    if !common::browser_ready() {
        return;
    }
    let b = TestBrowser::new("vocab-missing");
    let out = pipe(b.name(), &["{}"]);
    assert!(
        out[0]["error"].as_str().unwrap_or_default().contains("Missing"),
        "expected a missing-field message: {}",
        out[0]
    );
}
