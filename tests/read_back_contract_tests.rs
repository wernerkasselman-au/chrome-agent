//! The read-back verbs answer about a page that took the write away in one shape.
//!
//! `fill` reported `not_kept` / `value_reverted` with a `value` object and a `next` token.
//! `select` and `check` reported the same fact as prose in `error`, with no verdict, no
//! `value` and no `next`, because their refusal threw the measurement away on the way out.
//! Same situation, two contracts, and an agent told to branch on `verdict` got nothing from
//! two of the three.
//!
//! The refusal is unchanged and deliberately so: `select` and `check` still answer `ok:false`
//! where `fill` answers `ok:true`, because they decline to report a state the page does not
//! hold. What changed is that the refusal now carries the measurement behind it.

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

fn pipe(browser: &str, lines: &[String]) -> Vec<Value> {
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

/// One fixture, three verbs, one contract.
#[test]
fn every_read_back_verb_reports_a_reverted_write_the_same_way() {
    if !common::browser_ready() {
        return;
    }
    let b = TestBrowser::new("readback-contract");
    let url = common::fixture_url("controlled_reverts_every_write.html");
    let responses = pipe(
        b.name(),
        &[
            format!(r#"{{"cmd":"goto","url":"{url}"}}"#),
            r##"{"cmd":"fill","selector":"#txt","value":"hello"}"##.to_string(),
            r##"{"cmd":"select","selector":"#sel","value":"y"}"##.to_string(),
            r##"{"cmd":"check","selector":"#cb"}"##.to_string(),
        ],
    );

    for (verb, response) in [("fill", &responses[1]), ("select", &responses[2]), ("check", &responses[3])] {
        assert_eq!(
            response["verdict"], "not_kept",
            "{verb} must say the page did not keep the write: {response}"
        );
        assert_eq!(
            response["next"], "stop",
            "{verb} must not invite a retry of a write the page rejects: {response}"
        );
        let value = &response["value"];
        assert!(
            value.is_object(),
            "{verb} must carry the measurement, not just prose: {response}"
        );
        assert_eq!(
            value["verbatim"], false,
            "{verb} must say the read-back disagreed: {response}"
        );
        assert!(
            value.get("requested").is_some() && value.get("actual").is_some(),
            "{verb} must report both sides: {response}"
        );
    }
}

/// The refusal itself is unchanged. `select` and `check` decline; `fill` reports. Pinned so
/// that giving them a shared contract is not mistaken for giving them shared control flow.
#[test]
fn the_refusing_verbs_still_refuse() {
    if !common::browser_ready() {
        return;
    }
    let b = TestBrowser::new("readback-refusal");
    let url = common::fixture_url("controlled_reverts_every_write.html");
    let responses = pipe(
        b.name(),
        &[
            format!(r#"{{"cmd":"goto","url":"{url}"}}"#),
            r##"{"cmd":"fill","selector":"#txt","value":"hello"}"##.to_string(),
            r##"{"cmd":"select","selector":"#sel","value":"y"}"##.to_string(),
            r##"{"cmd":"check","selector":"#cb"}"##.to_string(),
        ],
    );
    assert_eq!(responses[1]["ok"], true, "fill reports rather than refuses");
    assert_eq!(responses[2]["ok"], false, "select refuses");
    assert_eq!(responses[3]["ok"], false, "check refuses");
    for verb in [2usize, 3] {
        assert!(
            responses[verb]["error"].as_str().is_some_and(|e| !e.is_empty()),
            "the prose the refusal always had is still there: {}",
            responses[verb]
        );
    }
}
