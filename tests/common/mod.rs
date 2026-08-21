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
    let candidates = if cfg!(target_os = "macos") {
        vec!["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"]
    } else {
        vec!["google-chrome", "chromium"]
    };
    for candidate in candidates {
        if std::path::Path::new(candidate).exists() {
            return true;
        }
        if Command::new("which").arg(candidate).output().is_ok_and(|o| o.status.success()) {
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
