//! A command's output pipe must close when the command exits, not when the browser does.
//!
//! `goto` deliberately leaves Chrome running: sessions persist so the next command costs a
//! connection rather than a launch. That makes the browser a grandchild of whoever invoked
//! us, and on Windows it used to inherit our stdout.
//!
//! `CreateProcessW` is called with `bInheritHandles = TRUE` and no handle list, so every
//! inheritable handle in the process passes to the child. Redirecting Chrome's own three
//! handles to null does not help, because the leaked one is ours. Chrome then held the write
//! end of the caller's pipe after we exited, the reader never saw EOF, and anything doing the
//! obvious thing (`wait_with_output`, or a shell pipeline) blocked for the browser's lifetime
//! rather than the command's.
//!
//! Measured on CI: `action_report_tests` sat inside one test for 28 minutes. Unix cannot
//! reach this, because Rust creates its pipe descriptors close-on-exec.

use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

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

/// Generous, because it is a deadlock detector rather than a performance budget. A cold
/// Chrome launch is seconds; the failure it guards against is unbounded.
const PATIENCE: Duration = Duration::from_secs(90);

#[test]
fn reading_a_commands_output_ends_with_the_command_not_the_browser() {
    if !common::browser_ready() {
        return;
    }
    let browser = TestBrowser::new("pipe-eof");
    let url = common::fixture_url("press_keys.html");
    let name = browser.name().to_string();

    // Read on another thread so the test can out-wait a hang instead of joining it. A plain
    // `wait_with_output()` here would make the failure indistinguishable from a slow machine,
    // and would hang the whole suite rather than failing one test.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let output = Command::new(binary())
            .args(["--browser", &name, "goto", &url])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn goto")
            .wait_with_output()
            .expect("read goto output");
        let _ = tx.send(output.status.success());
    });

    match rx.recv_timeout(PATIENCE) {
        Ok(succeeded) => assert!(succeeded, "goto should succeed"),
        // Both error cases mean the same thing here and want the same message: `Timeout` is
        // the reader still blocked, and `Disconnected` is the reader thread having died
        // without sending, which is equally a failure to reach EOF.
        Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => panic!(
            "the command exited but its output pipe never closed: something it spawned is \
             still holding the write end. On Windows that is Chrome inheriting our stdout."
        ),
    }
}
