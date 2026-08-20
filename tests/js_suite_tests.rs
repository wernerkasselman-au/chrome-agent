//! The vendored extraction engine's own test suite is part of the gate.
//!
//! `vendor/extract.js` carries 120+ jsdom unit tests under `tests/js/`, and nothing ran
//! them: not `cargo test`, not CI. They were a suite you had to remember to invoke by
//! hand, which is the same as a suite that cannot fail — a regression in the extraction
//! heuristics shipped green. This runs them as part of the Rust gate.
//!
//! Skips when node or the jsdom install is missing, and, like every other skip here, that
//! skip is fatal under `CHROME_AGENT_REQUIRE_CHROME`.

use std::path::PathBuf;
use std::process::Command;

mod common;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn the_extraction_engine_suite_passes() {
    let root = repo_root();
    let js_dir = root.join("tests/js");

    if Command::new("node").arg("--version").output().is_err() {
        common::unavailable("node not found — the extract.js suite cannot run");
        return;
    }
    if !js_dir.join("node_modules/jsdom").exists() {
        common::unavailable("tests/js/node_modules/jsdom missing — run `npm ci` in tests/js");
        return;
    }

    let tests: Vec<PathBuf> = std::fs::read_dir(&js_dir)
        .expect("read tests/js")
        .filter_map(|e| {
            let path = e.ok()?.path();
            path.to_str()?.ends_with(".test.js").then_some(path)
        })
        .collect();
    assert!(
        !tests.is_empty(),
        "no *.test.js under {} — an empty suite reports the same green as a passing one",
        js_dir.display()
    );

    // `--test-reporter=tap` is not cosmetic: the pass count below is parsed out of this
    // output, and node's default reporter is not stable across versions. It emitted TAP
    // (`# pass 127`) when this test was written and emits the spec reporter (`ℹ pass 127`)
    // on node 24, so the `strip_prefix("# pass ")` finds nothing and the parse fails.
    //
    // The failure mode is the one this file exists to prevent, inverted. The suite runs and
    // passes, node exits 0, and the gate panics on its own bookkeeping. Worse, the same
    // version drift in the other direction would be silent: a reporter with no pass line at
    // all and a runner that matched no files both leave the assertion below unable to speak.
    // Pinning the format keeps the check readable by this parser whatever node does next.
    //
    // CI has not seen it because no workflow step pins a node version, and the runner image
    // currently ships one whose default is still TAP.
    let output = Command::new("node")
        .arg("--test")
        .arg("--test-reporter=tap")
        .args(&tests)
        .current_dir(&root)
        .output()
        .expect("run node --test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the extract.js suite failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // A runner that silently matched nothing exits 0 too.
    let passed: usize = stdout
        .lines()
        .find_map(|l| l.strip_prefix("# pass ")?.trim().parse().ok())
        .expect("node --test should report a pass count");
    assert!(
        passed >= 100,
        "expected the full extraction suite (100+ tests), got {passed} — did the runner match the files?"
    );
}
