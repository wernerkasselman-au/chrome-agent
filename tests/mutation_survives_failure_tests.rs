//! A command that changed the page and then failed still has to say what it changed.
//!
//! `pipe::dispatch` and `pipe_dispatch::dispatch_single` return early on `Err`, before
//! `attach_change_report` and the verdict run. Any dispatcher that mutates and then fails
//! therefore answered with the failure and nothing else: no `delta`, no `verdict`, no
//! read-back. The caller reads `ok:false`, concludes its write did not land, and does it
//! again, which for a submit is a second real submit.
//!
//! `pipe_dispatch_actions` already stated the rule for its `read` step, "the fill and the
//! submit have already landed, failing the whole command there tells an agent its mutation
//! did not happen, and the natural response to that is to submit again", and then broke it
//! two lines above with a `?` on the wait. These tests pin both halves.

use std::process::Command;

use serde_json::Value;

mod common;
use common::{binary, run_cli, PipeSession};

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

/// The uids of the two text inputs, read out of an `inspect` response rather than assumed.
/// backendNodeId values are stable within a document but not a constant of the fixture.
fn textbox_uids(inspect: &Value) -> Vec<String> {
    inspect["snapshot"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("textbox"))
        .filter_map(|l| l.split("uid=").nth(1)?.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// Drive a pipe session and return one parsed response per input line.
fn run_pipe(browser: &str, lines: &[String]) -> Vec<Value> {
    let mut child = Command::new(binary())
        .args(["--browser", browser, "--timeout", "3", "pipe"])
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

/// The wait the caller asked for never settles, and the submit already went through.
///
/// Before: `{"error":"Timeout after 3s waiting for text matching \"NEVER_APPEARS\"","ok":false}`
/// and nothing else, over a page that had been submitted.
#[test]
fn a_submit_that_landed_is_reported_even_when_the_wait_times_out() {
    if !common::browser_ready() {
        return;
    }
    let browser = TestBrowser::new("wait-after-submit");
    let url = common::fixture_url("submit_then_wait_never_settles.html");
    let responses = run_pipe(
        browser.name(),
        &[
            format!(r#"{{"cmd":"goto","url":"{url}"}}"#),
            r##"{"cmd":"fill_and_submit","fields":[{"selector":"#email","value":"a@b.c"}],"submit":"#go","wait_for":"NEVER_APPEARS"}"##
                .to_string(),
            r#"{"cmd":"eval","expression":"(window.__submits||0)"}"#.to_string(),
        ],
    );
    assert_eq!(responses.len(), 3, "one response per command: {responses:?}");
    let submit = &responses[1];

    // The wait failed, and that is a field rather than the whole answer.
    assert_eq!(
        submit["wait_error"].as_str().map(|s| s.contains("NEVER_APPEARS")),
        Some(true),
        "the wait failure must be named: {submit}"
    );
    // The submit is the part that must not go unreported.
    assert_eq!(submit["delivery"], "target_hit", "the submit's own delivery: {submit}");
    assert!(submit.get("verdict").is_some(), "a verdict must ride on it: {submit}");
    assert!(submit.get("values").is_some(), "the fill read-back survives: {submit}");
    assert!(
        !submit["message"].as_str().unwrap_or_default().contains("waited for 'NEVER_APPEARS'"),
        "the message must not claim a wait that did not finish: {submit}"
    );

    // Ground truth: the page really was submitted exactly once.
    assert_eq!(responses[2]["result"], 1, "the submit landed: {:?}", responses[2]);
}

/// A malformed pair after a good one used to write the good one first, then answer with an
/// argument error, which is the shape of a request that never touched the page.
#[test]
fn a_malformed_pair_is_refused_before_any_field_is_written() {
    if !common::browser_ready() {
        return;
    }
    let browser = TestBrowser::new("fillform-validate");
    let url = common::fixture_url("fill_form_second_field_disabled.html");
    let mut pipe = PipeSession::start(browser.name());
    pipe.send(&format!(r#"{{"cmd":"goto","url":"{url}"}}"#));
    let uids = textbox_uids(&pipe.send(r#"{"cmd":"inspect"}"#));
    assert_eq!(uids.len(), 2, "fixture should expose two textboxes");
    let fill = pipe.send(&format!(
        r#"{{"cmd":"fill_form","pairs":[{{"uid":"{}","value":"WROTE_THIS"}},{{"value":"no-uid"}}]}}"#,
        uids[0]
    ));
    let written = pipe.send(r#"{"cmd":"eval","expression":"document.getElementById('one').value"}"#);
    let fill = &fill;
    assert_eq!(fill["ok"], false, "the malformed pair is still refused: {fill}");
    assert!(
        fill.get("mutated").is_none(),
        "nothing was written, so nothing is claimed: {fill}"
    );
    assert_eq!(written["result"], "", "the first field must be untouched: {written}");
}

/// The second field genuinely refuses the write, after the first one took it.
#[test]
fn a_bulk_fill_that_stops_halfway_reports_what_it_already_wrote() {
    if !common::browser_ready() {
        return;
    }
    let browser = TestBrowser::new("fillform-partial");
    let url = common::fixture_url("fill_form_second_field_disabled.html");
    let mut pipe = PipeSession::start(browser.name());
    pipe.send(&format!(r#"{{"cmd":"goto","url":"{url}"}}"#));
    let uids = textbox_uids(&pipe.send(r#"{"cmd":"inspect"}"#));
    assert_eq!(uids.len(), 2, "fixture should expose two textboxes");
    let fill = pipe.send(&format!(
        r#"{{"cmd":"fill_form","pairs":[{{"uid":"{}","value":"WROTE_ONE"}},{{"uid":"{}","value":"blocked"}}]}}"#,
        uids[0], uids[1]
    ));
    let fill = &fill;

    assert_eq!(fill["ok"], false, "the command did not do what was asked: {fill}");
    assert_eq!(fill["mutated"], true, "and it says the page moved anyway: {fill}");
    assert!(
        fill["values"].as_array().is_some_and(|v| !v.is_empty()),
        "the field that was written is named: {fill}"
    );
    assert!(fill.get("delta").is_some(), "the change report survives the failure: {fill}");
    assert_eq!(fill["verdict"], "changed", "and a verdict rides on it: {fill}");
    // `proceed` on a command that failed after mutating would be the same false comfort
    // `next_for` already refuses for a page it could not read.
    assert_eq!(fill["next"], "inspect", "the branch must send the caller to look: {fill}");
}
