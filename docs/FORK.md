# This is a fork, and it is ours

Forked from [sderosiaux/chrome-agent](https://github.com/sderosiaux/chrome-agent) at
`b904691` (v0.12.0), MIT. The upstream project is one person's work and a good one; nothing
here is a criticism of it.

**Decision: we do not track upstream.** Changes are made because we need the tool to be
reliable under our own agents, not because they would be accepted elsewhere. Merges from
upstream will conflict, particularly in the dispatchers, and that is accepted rather than
worked around. Nothing here is staged for a pull request, and no change should be shaped by
what a maintainer might want.

The practical consequence: if upstream ships something we want, we port it deliberately as a
change of our own. We do not `git merge upstream/main` and expect it to apply.

## Licence and attribution

MIT, unchanged. The upstream copyright notice stays in `LICENSE`, and the README keeps its
attribution. Every file we did not write remains under its original terms, including
`vendor/Readability.js` (Mozilla, MIT).

## Why we diverged

The tool's own thesis is that a response an agent reads must not let it draw a wrong
conclusion. Reviewing it against that standard turned up six reachable cases where a
response was true and still misleading, or simply false. Those are what we fixed. The
refactor that follows exists to make one of those defect classes unrepresentable rather than
merely fixed.

## What changed

### Correctness

Each was reproduced against a built binary before being fixed, and each carries a regression
test verified to fail against the unfixed source.

| Defect | The false claim |
| --- | --- |
| `fill_and_submit` wait | Submitted the form, then answered with only a wait timeout. An agent reading `ok:false` retries, and the retry is a second real submit. |
| `fill_form` mid-loop | Wrote a field, then failed argument validation on a later pair and answered with an argument error, which is the shape of a request that never touched the page. |
| `eval` | Its changes were reported as the *next* command's delta. A click on an inert button answered `changed / tree_delta` quoting a paragraph the eval had appended. |
| `extract --scroll` | The same, via lazy-loaded rows, and it survived the error path because the extract also failed. |
| `eval` / `extract --scroll` on the CLI | The same again, across process boundaries, because the CLI keeps its baseline in `sessions.json`. |
| `select` and `check` | Reported a rejected write as prose in `error`, where `fill` reports `not_kept` / `value_reverted` with a `value` object and a `next` token. An agent told to branch on `verdict` got nothing from two of the three read-back verbs. |

The invariant behind all of them, which upstream states nowhere:

> A command that can move the page must either own the claim or flag the baseline, and that
> obligation cannot depend on the command succeeding or on which surface it arrived through.

### Response-shape changes an agent will see

- A command that mutated and then failed answers `ok:false` **and** carries `mutated: true`,
  the read-back for whatever it wrote, the delta, and a verdict.
- `eval` and `extract --scroll` carry `baseline_moved: true`.
- `select` and `check` on a page that rejects the write carry `not_kept`, a `value` object
  and `next: stop`, alongside the prose they always had. They still answer `ok:false`: they
  decline to report a state the page does not hold, and the CLI exit code depends on it.
- `next` never answers `proceed` on a response whose `ok` is `false`.
- A `batch` nested inside a `batch` says so, instead of `Unknown command: batch`.

### Structure

`src/pipe_verb.rs` introduces `PipeVerb`, one vocabulary for the JSON surfaces. Four lists
keyed off command strings with nothing connecting them became one enum: both dispatch
matches are exhaustive over it, and both classifications are methods on it carrying
`#[deny(clippy::wildcard_enum_match_arm)]`. Adding a command is one variant, and the
compiler refuses to build until every question about it is answered.

The design and the reasoning are in `docs/design/verb-vocabulary.md`, with the larger and
deliberately unbuilt proposal in `docs/design/dispatch-unification.md`.

### Infrastructure

- `tests/js_suite_tests.rs` pins `--test-reporter=tap`. Node's default reporter changed, so
  on Node 24 the gate panicked parsing its own output while the suite underneath passed.
- `session::FileLock` uses `std::fs::File::lock` instead of `libc::flock` plus a non-Unix
  no-op. Windows gains the mutual exclusion the parallel-agent feature always claimed, and
  two `unsafe` blocks go.
- One `run_cli` and one `binary` in `tests/common`, replacing 25 and 33 copies.

### Platform support

`release.yml` ships five targets. Before this fork, tests had run on exactly one of them.
Running them elsewhere found that a shipped target can be entirely non-functional and nobody
would know, because the failure hides behind a suite that reports green.

On Windows, four defects sat on top of one another, each hiding the next:

1. The test suite did not compile. Two pieces of Unix-only test code were never gated, and
   either one takes the whole test binary down. So `cargo test` had never run there.
2. The binary stack-overflowed on startup. `run.rs` documents its dispatch frame as ~527 KB
   of MIR locals; Windows gives the main thread 1 MiB and Linux gives it 8 MiB. Every
   invocation died with `STATUS_STACK_OVERFLOW`, `--version` included.
3. `chrome_available()` in the test harness looked for `google-chrome` with `which`, neither
   of which exists on Windows, so every browser test skipped. A skip prints with `eprintln!`,
   which cargo captures for a test it counts as passing, so the skips were invisible in CI
   logs. `CHROME_AGENT_REQUIRE_CHROME` is what exposed them.
4. `browser.rs` listed `chrome.exe` as its only Windows candidate, as a relative path, with
   the PATH lookup gated to Linux. Chrome could never be found, over an error message
   advising the caller to put it on PATH that never consulted PATH.

1, 2 and 4 were shipped defects, not test problems. The tool has been published for Windows
for the life of the project and could not open a page there.

## Verification status

CI runs on this fork across three platforms. `workflow_dispatch` is enabled, so a run can be
triggered without inventing a commit.

| Platform | State |
| --- | --- |
| Linux (`ubuntu-24.04`) | 608 tests, 40 binaries, **0 skips** under `CHROME_AGENT_REQUIRE_CHROME=1` |
| macOS (`macos-14`) | green on its first run, full suite including browser tests |
| Windows (`windows-2022`) | 538 tests passing, 2 failing. Browser automation works; see below |

Also verified on Linux: `clippy --all-targets -- -D warnings` clean with pedantic and
nursery, the static musl artifact builds and links statically and drives a live site, and a
61-command pipe session against a live site with no drift. `clippy` is clean for
`x86_64-pc-windows-msvc` too. All three jobs install the jsdom suite and set
`CHROME_AGENT_REQUIRE_CHROME`, so a skip fails rather than passing.

The `FileLock` change is observed rather than reasoned.
`concurrent_saves_under_lock_lose_no_updates` (24 threads, load-modify-save, assert no lost
update) passes on Windows, and could not have before: the non-Unix arm was an empty struct
returning `Ok(())`.

### Windows: what was actually wrong

Six defects, each hidden by the one above it. Three were shipped, not test problems.

1. **The test suite did not compile.** Two pieces of Unix-only test code were never gated,
   and either takes the whole binary down. Nothing had ever run.
2. **The binary stack-overflowed on startup.** `run.rs` documents its dispatch frame as
   ~527 KB of MIR locals; Windows gives the main thread 1 MiB against Linux's 8 MiB. Every
   invocation died, `--version` included. Boxing the future does not fix it, because the
   value is materialised on the stack before the move; the runtime now runs on a thread whose
   stack size we choose.
3. **`chrome_available()` looked for `google-chrome` with `which`.** Neither exists on
   Windows, so every browser test skipped, and a skip prints with `eprintln!` which cargo
   hides for a test it counts as passing. Invisible until `CHROME_AGENT_REQUIRE_CHROME`.
4. **`browser.rs` could not find Chrome.** Its only Windows candidate was `chrome.exe` as a
   relative path, with the PATH lookup gated to Linux, behind an error advising the caller to
   put Chrome on PATH that never consulted PATH.
5. **Chrome inherited our stdout, so reading a command's output waited for the browser.**
   `CreateProcessW` runs with `bInheritHandles = TRUE` and no handle list, so every
   inheritable handle passes to the child, and `goto` deliberately leaves Chrome running.
   The reader never saw EOF: one test sat for 28 minutes. Unix cannot reach this because Rust
   creates its pipe descriptors close-on-exec.
6. **The fixture HTTP server reset connections instead of closing them.** Dropping a socket
   with anything queued is an abortive close on Windows, which Chrome reports as
   `net::ERR_SOCKET_NOT_CONNECTED` against the navigation about to use it.

Plus four tests that asserted Unix spellings: daemon wording, a `file://` URL built with
backslashes and no third slash, a screenshot path, and a uid carried between two browsers,
which only ever worked because the ids happened to agree.

### Windows: known, deliberately not done

`session::liveness` still answers `Unknown` on Windows, so `prune_dead` never removes an
entry there. It matters less than it did, because `close` now actually terminates the
browser and removes its entry, so the store no longer grows from ordinary use; only a
crashed browser leaves one behind.

A `tasklist` implementation was written and taken back out. `prune_dead` calls `liveness`
once per entry on every save, and a subprocess is ~150ms a call, which is a latency
regression on every command in exchange for a problem that no longer occurs in normal use.
Doing it properly means `OpenProcess`/`GetExitCodeProcess`, including the
`ERROR_ACCESS_DENIED` case that mirrors Unix `EPERM`.

### Windows: what is still failing

Two tests in `read_back_verdict_tests`, both downstream of `goto read_back_kinds.html`
failing on that platform while other fixtures load. Not yet diagnosed.

The trend across rounds is 372, 414, 485, 514, 538 passing, each round fixing a real defect
and reaching further. Windows is much closer to working than it has ever been and is not
finished. Until those two pass, treat Windows as usable at your own risk rather than
supported.

### Still untested

`--stealth`, `--connect`, `--copy-cookies`, downloads and PDF against real sites, iframes on
real pages, and the npm packaging path. `aarch64` on either Linux or macOS, both of which are
shipped targets.
