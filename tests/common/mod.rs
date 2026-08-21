//! Shared test harness.
//!
//! Every browser test in this suite used to open with the same twelve lines: find Chrome,
//! `eprintln!("SKIP: …")`, `return`. A bare `return` inside `#[test]` is a pass, so a machine
//! without Chrome — or a fixture deleted by mistake — turned ~57 of 68 tests into green
//! no-ops. This module makes that failure loud when it matters and quiet when it doesn't:
//! locally a missing Chrome still skips, but with `CHROME_AGENT_REQUIRE_CHROME=1` (set in CI)
//! the same condition panics.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;

/// Environment variable CI sets to turn every skip into a failure.
pub const REQUIRE_ENV: &str = "CHROME_AGENT_REQUIRE_CHROME";

/// Whether a raw `REQUIRE_ENV` value demands a real browser run. Split from the lookup so
/// the decision is testable without mutating the process environment (`set_var` is unsafe
/// in edition 2024 and this crate forbids `unsafe`).
#[must_use]
pub fn require_from(value: Option<&str>) -> bool {
    matches!(value, Some(v) if v != "0" && !v.is_empty())
}

/// Whether the caller demands a real browser run (CI) or tolerates a skip (a laptop
/// without Chrome).
#[must_use]
pub fn require_chrome() -> bool {
    require_from(std::env::var(REQUIRE_ENV).ok().as_deref())
}

/// Report a precondition the test cannot meet. Returns `false` (the caller then returns and
/// the test passes as a skip) unless a browser run was required, in which case it panics.
///
/// The pure form is `unavailable_with`; this is the environment-reading wrapper.
pub fn unavailable(reason: &str) -> bool {
    unavailable_with(require_chrome(), reason)
}

/// `unavailable` with the policy passed in.
///
/// # Panics
/// When `require` is set — that is the point: a CI run that silently skips every browser
/// test reports the same green as one that ran them.
pub fn unavailable_with(require: bool, reason: &str) -> bool {
    assert!(
        !require,
        "{REQUIRE_ENV} is set, so this test may not be skipped: {reason}"
    );
    eprintln!("SKIP: {reason}");
    false
}

/// True when a Chrome binary exists on this machine.
#[must_use]
pub fn chrome_available() -> bool {
    let candidates: Vec<String> = if cfg!(target_os = "macos") {
        vec!["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string()]
    } else if cfg!(target_os = "windows") {
        // Windows used to fall through to the Unix arm and look for `google-chrome` with
        // `which`, neither of which exists there, so this always answered false. The tests
        // that gate on it skipped, and a skipped test prints its reason with `eprintln!`,
        // which cargo captures and never shows for a test it counts as passing. Five tests
        // were reported green on Windows without running, until
        // `CHROME_AGENT_REQUIRE_CHROME` turned the skip into the failure it should have been.
        let mut paths = vec!["chrome.exe".to_string()];
        for root in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Ok(dir) = std::env::var(root) {
                paths.push(format!(r"{dir}\Google\Chrome\Application\chrome.exe"));
            }
        }
        paths
    } else {
        vec!["google-chrome".to_string(), "chromium".to_string()]
    };
    // `which` is not a command on Windows.
    let locator = if cfg!(target_os = "windows") { "where" } else { "which" };
    for candidate in &candidates {
        if std::path::Path::new(candidate).exists() {
            return true;
        }
        if Command::new(locator).arg(candidate).output().is_ok_and(|o| o.status.success()) {
            return true;
        }
    }
    false
}

/// `true` when the test may proceed. Skips (or fails, under `REQUIRE_ENV`) otherwise.
#[must_use]
pub fn browser_ready() -> bool {
    if chrome_available() {
        return true;
    }
    unavailable("Chrome not found")
}

/// Absolute path of a fixture, asserted to exist.
///
/// # Panics
/// When the fixture is missing. Deleting a fixture used to leave the tests that load it
/// green: `file://…/gone.html` navigates to an error page and every later assertion was
/// guarded by an early return.
#[must_use]
pub fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(name);
    assert!(
        path.exists(),
        "fixture does not exist: {} — a test that navigates to a missing file:// URL cannot fail honestly",
        path.display()
    );
    path
}

/// `file://` URL of a fixture, asserted to exist.
///
/// Not `format!("file://{}", path.display())`. That is correct on Unix only by accident: the
/// path already starts with `/`, so the two slashes plus the root make the three a file URL
/// needs. On Windows the same expression produced
/// `file://D:\a\chrome-agent\...\press_keys.html`, which is not a URL Chrome will load:
/// the drive letter lands where the authority belongs and the separators are backslashes.
///
/// Every browser test navigates through this, so on Windows every one of them was aimed at
/// an unloadable address.
#[must_use]
pub fn fixture_url(name: &str) -> String {
    let path = fixture_path(name).display().to_string().replace('\\', "/");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        // `D:/a/...` needs the third slash that a rooted Unix path brings with it.
        format!("file:///{path}")
    }
}

/// Path to the `chrome-agent` binary under test, as a string.
///
/// `current_exe()` is the test binary in `target/<profile>/deps/`; two `parent()` hops
/// reach the profile directory the CLI is built into.
///
/// Was copied verbatim into 24 test files. Four others keep their own spelling because
/// they genuinely differ: three return a `PathBuf`, and `profile_prune_tests` pops a
/// trailing `deps` conditionally rather than unconditionally. Those are left alone here
/// rather than folded in, since collapsing them would be a behaviour change wearing the
/// clothes of a cleanup.
#[must_use]
pub fn binary() -> String {
    let mut path = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
    path.push("chrome-agent");
    path.to_string_lossy().into_owned()
}

/// Run the CLI and return `(stdout, exit code)`.
///
/// Byte-identical in 21 test files before this moved here. An exit code of `-1` stands for
/// a process killed by a signal, where `code()` is `None`; no command under test returns a
/// negative status, so the two cannot be confused.
///
/// Deliberately not the only shape. Three files need stderr as well and one needs the code
/// alone, so `run_cli_full` is the richer form and the code-only case reads `.1` off this
/// one. Adding optional out-params to a single helper would put the shape decision at every
/// call site instead of in the signature.
///
/// Not `#[must_use]`: several tests run a setup command (a `goto`, a `close`) for its effect
/// and have no use for the output. Marking it would turn those into `let _ =` noise.
pub fn run_cli(args: &[&str]) -> (String, i32) {
    let output = Command::new(binary()).args(args).output().expect("Failed to run chrome-agent");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

/// Run the CLI and return `(stdout, stderr, exit code)`.
///
/// For the tests that assert on the stream a message lands in, which is a contract in its
/// own right: `--json` puts a failed assertion on stdout, text mode on stderr with stdout
/// left empty, so a shell pipeline can use the exit code alone.
pub fn run_cli_full(args: &[&str]) -> (String, String, i32) {
    let output = Command::new(binary()).args(args).output().expect("Failed to run chrome-agent");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

/// One live pipe session, driven a command at a time.
///
/// Needed whenever a test must read one response before it can compose the next command,
/// which is every uid-targeted test: a uid is a `backendNodeId` and is only meaningful
/// inside the document it was read from. Probing in one `run_pipe` and acting in another
/// looks right and is not: those are two browsers and two documents, and the ids agreeing
/// is a coincidence that held on Linux and did not on Windows.
///
/// The uid-targeted tests need the uids from an `inspect` before they can compose the next
/// command, and a uid is only good inside the document it was read from. Sending everything
/// up front and reading at the end cannot do that: a second session re-navigates, and the
/// backendNodeId counters land somewhere else.
pub struct PipeSession {
    child: std::process::Child,
    out: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
}

impl PipeSession {
    pub fn start(browser: &str) -> Self {
        use std::io::BufRead;
        let mut child = Command::new(binary())
            .args(["--browser", browser, "--timeout", "3", "pipe"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn pipe");
        let out = std::io::BufReader::new(child.stdout.take().expect("pipe stdout")).lines();
        Self { child, out }
    }

    /// Send one command and return its response.
    pub fn send(&mut self, line: &str) -> serde_json::Value {
        use std::io::Write;
        let stdin = self.child.stdin.as_mut().expect("pipe stdin");
        writeln!(stdin, "{line}").expect("write pipe command");
        stdin.flush().expect("flush pipe command");
        let raw = self.out.next().expect("a response line").expect("readable response");
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("not JSON ({e}): {raw}"))
    }
}

impl Drop for PipeSession {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}
