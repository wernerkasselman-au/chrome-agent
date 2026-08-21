//! One command's changes must not be reported as another command's delta.
//!
//! `eval` runs caller-supplied JavaScript and is deliberately outside `mutates_page`, because
//! a change report costs a settle plus a tree read and `eval` is also the documented way to
//! read structured data out of a page. The cost of that exclusion was a stale baseline: the
//! document is unchanged, so the identity check passes, and the next action's diff is
//! believed.
//!
//! Measured before the fix, on this fixture: an `eval` appended a paragraph, and the
//! following click on an inert button answered `changed / tree_delta` and quoted that
//! paragraph as its own delta. Not a missing claim, a false one, about causation.

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

fn run_pipe(browser: &str, lines: &[String]) -> Vec<Value> {
    let mut child = Command::new(binary())
        .args(["--browser", browser, "--timeout", "5", "pipe"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn pipe");
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("pipe stdin");
        for line in lines {
            writeln!(stdin, "{line}").expect("write pipe command");
        }
    }
    let out = child.wait_with_output().expect("pipe output");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}")))
        .collect()
}

#[test]
fn an_evals_changes_are_never_reported_as_the_next_commands_delta() {
    if !common::browser_ready() {
        return;
    }
    let browser = TestBrowser::new("eval-attribution");
    let url = common::fixture_url("eval_mutates_between_actions.html");
    let responses = run_pipe(
        browser.name(),
        &[
            format!(r#"{{"cmd":"goto","url":"{url}"}}"#),
            r#"{"cmd":"inspect"}"#.to_string(),
            r#"{"cmd":"eval","expression":"document.getElementById('host').innerHTML = '<p>ADDED_BY_EVAL</p>'; 1"}"#.to_string(),
            r##"{"cmd":"click","selector":"#inert"}"##.to_string(),
        ],
    );
    let eval = &responses[2];
    let click = &responses[3];

    // The eval says it dropped the baseline, so `no_baseline` on the next command is
    // explained rather than mysterious.
    assert_eq!(eval["baseline_cleared"], true, "eval must say it invalidated the baseline: {eval}");

    // The click did nothing. Whatever it answers, it must not claim the eval's paragraph.
    let delta = click["delta"].as_str().unwrap_or_default();
    assert!(
        !delta.contains("ADDED_BY_EVAL"),
        "the click reported another command's change as its own: {click}"
    );
    assert_ne!(
        click["verdict_reason"], "tree_delta",
        "an inert click has no tree delta of its own: {click}"
    );
}
