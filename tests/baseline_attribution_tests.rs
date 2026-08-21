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
//!
//! The stored snapshot is NOT dropped, and `diff_tests` pins why: `diff` asks what changed
//! since the caller last looked, and an `eval`'s work belongs in that answer. It is only
//! wrong as the base for the next action's claim. So it is flagged instead, and the action
//! path re-reads the page before acting. The click then answers accurately rather than
//! merely declining to answer.

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
    assert_eq!(eval["baseline_moved"], true, "eval must say it overtook the baseline: {eval}");

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
    // Re-reading rather than dropping the baseline is what lets this stay a real answer
    // instead of an admission of ignorance.
    assert_ne!(
        click["verdict_reason"], "no_baseline",
        "the action re-reads the page, so it still has something to say: {click}"
    );
}

/// `extract --scroll` loads content and then, on this fixture, fails to find a pattern.
///
/// Two things at once. The scroll appended rows the next command must not claim, and the
/// command errored, so a baseline cleared after the dispatch would never have run: the error
/// path returns early. Measured before the fix, the click answered `changed / tree_delta` and
/// quoted `LAZY_A` and `LAZY_B`.
#[test]
fn a_scrolling_extract_does_not_leave_its_lazy_rows_for_the_next_command() {
    if !common::browser_ready() {
        return;
    }
    let browser = TestBrowser::new("extract-attribution");
    let url = common::fixture_url("lazy_list_loads_on_scroll.html");
    let responses = run_pipe(
        browser.name(),
        &[
            format!(r#"{{"cmd":"goto","url":"{url}"}}"#),
            r#"{"cmd":"inspect"}"#.to_string(),
            r#"{"cmd":"extract","scroll":true,"limit":10}"#.to_string(),
            r##"{"cmd":"click","selector":"#inert"}"##.to_string(),
        ],
    );
    let extract = &responses[2];
    let click = &responses[3];

    assert_eq!(
        extract["baseline_moved"], true,
        "a scrolling extract overtakes the baseline even when it fails: {extract}"
    );
    let delta = click["delta"].as_str().unwrap_or_default();
    assert!(
        !delta.contains("LAZY_"),
        "the click reported the extract's lazy rows as its own: {click}"
    );
}

/// An `extract` that did not scroll moved nothing, so it must not cost the next command its
/// report. The guard against fixing one false claim by making every answer useless.
#[test]
fn an_extract_that_did_not_scroll_leaves_the_baseline_alone() {
    if !common::browser_ready() {
        return;
    }
    let browser = TestBrowser::new("extract-noscroll");
    let url = common::fixture_url("lazy_list_loads_on_scroll.html");
    let responses = run_pipe(
        browser.name(),
        &[
            format!(r#"{{"cmd":"goto","url":"{url}"}}"#),
            r#"{"cmd":"inspect"}"#.to_string(),
            r#"{"cmd":"extract","limit":10}"#.to_string(),
            r##"{"cmd":"click","selector":"#inert"}"##.to_string(),
        ],
    );
    assert!(
        responses[2].get("baseline_moved").is_none(),
        "a plain extract moves nothing: {}",
        responses[2]
    );
    assert_ne!(
        responses[3]["verdict_reason"], "no_baseline",
        "the next command keeps its report: {}",
        responses[3]
    );
}

/// The CLI keeps its baseline in `sessions.json` between invocations, so the same stale
/// snapshot outlives the process that made it. Measured before the fix: a `click` run as a
/// separate command after a mutating `eval` answered `changed / tree_delta` and quoted the
/// eval's paragraph.
///
/// A separate code path from the two JSON dispatchers, and it had the same defect.
#[test]
fn the_cli_does_not_carry_an_evals_changes_into_the_next_invocation() {
    if !common::browser_ready() {
        return;
    }
    let browser = TestBrowser::new("cli-attribution");
    let url = common::fixture_url("eval_mutates_between_actions.html");
    let b = browser.name();

    assert_eq!(run_cli(&["--browser", b, "goto", &url]).1, 0, "goto");
    assert_eq!(run_cli(&["--browser", b, "inspect"]).1, 0, "inspect takes the baseline");
    assert_eq!(
        run_cli(&[
            "--browser", b, "eval",
            "document.getElementById('host').innerHTML = '<p>ADDED_BY_EVAL</p>'; 1",
        ])
        .1,
        0,
        "eval mutates"
    );

    let (stdout, code) = run_cli(&["--browser", b, "--json", "click", "--selector", "#inert"]);
    assert_eq!(code, 0, "the click itself succeeds: {stdout}");
    let click: Value = serde_json::from_str(&stdout).expect("json response");
    assert!(
        !click["delta"].as_str().unwrap_or_default().contains("ADDED_BY_EVAL"),
        "the click reported the eval's change as its own: {click}"
    );
    assert_ne!(
        click["verdict_reason"], "tree_delta",
        "an inert click has no tree delta of its own: {click}"
    );
}
