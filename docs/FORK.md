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

CI runs on three platforms and all three are green with zero skips. Every job installs the
jsdom suite and sets `CHROME_AGENT_REQUIRE_CHROME`, so a test that declines to run fails the
build rather than passing quietly.

| Platform | Result |
| --- | --- |
| Linux (`ubuntu-24.04`) | 609 passed, 0 skips |
| macOS (`macos-14`) | passed, 0 skips |
| Windows (`windows-2022`) | 586 passed, 0 skips |

Also verified on Linux: `clippy --all-targets -- -D warnings` clean with pedantic and
nursery, the static musl artifact builds and links statically and drives a live site, and a
61-command pipe session against a live site with no drift. `clippy` is clean for
`x86_64-pc-windows-msvc` too.

## Windows: it never worked, and now it does

`release.yml` ships five targets. Before this fork, tests had run on one. Windows was
published for the life of the project and could not open a page. Six defects sat on top of
one another, each hiding the next, and four were shipped rather than test problems.

1. **The test suite did not compile.** Two pieces of Unix-only test code were never gated.
   Nothing on the platform had ever run, which is what kept the rest invisible.
2. **The binary stack-overflowed on startup.** `run.rs` documents its dispatch frame as
   ~527 KB of MIR locals; Windows gives the main thread 1 MiB against Linux's 8 MiB. Every
   invocation died, `--version` included. Boxing the future does not help, because the value
   is materialised on the stack before the move; the runtime runs on a thread whose stack size
   we choose.
3. **`chrome_available()` in the harness looked for `google-chrome` with `which`.** Neither
   exists there, so every browser test skipped, and a skip prints with `eprintln!` which cargo
   hides for a test it counts as passing.
4. **`browser.rs` could not find Chrome.** Its only candidate was `chrome.exe` as a relative
   path, with the PATH lookup gated to Linux, behind an error advising the caller to put
   Chrome on PATH that never consulted PATH.
5. **Chrome inherited our stdout.** `CreateProcessW` runs with `bInheritHandles = TRUE` and no
   handle list, and `goto` deliberately leaves Chrome running, so the browser held the write
   end of the caller's pipe after we exited. Reading a command's output waited for the
   browser's lifetime instead of the command's: one test sat for 28 minutes.
6. **`close` never terminated the browser.** `kill_pid` returned `NotABrowser` and signalled
   nothing, so every invocation leaked a Chrome. It presented as two unrelated test failures,
   and the giveaway was that the first four browsers in a suite worked and everything after
   did not.

Plus five tests that asserted Unix spellings or depended on a third party: daemon wording, a
`file://` URL built with backslashes, a screenshot path, a uid carried between two browsers,
and a fixture server that sent `Connection: close` per redirect hop.

`--no-fail-fast` on the Windows job is what made this tractable. cargo stops at the first
failing binary, so each round surfaced exactly one defect and the binaries sorting later never
ran at all.

## Known, deliberately not done

`session::liveness` still answers `Unknown` on Windows, so `prune_dead` never removes an
entry there. It matters less than it did, because `close` now terminates the browser and
removes its entry, so the store no longer grows from ordinary use; only a crashed browser
leaves one behind.

A `tasklist` implementation was written and taken back out. `prune_dead` calls `liveness`
once per entry on every save, and a subprocess is ~150ms a call: a latency regression on
every command in exchange for a problem that no longer occurs in normal use. Doing it
properly means `OpenProcess`/`GetExitCodeProcess`, including the `ERROR_ACCESS_DENIED` case
that mirrors Unix `EPERM`.

## Still untested

`--stealth`, `--connect`, `--copy-cookies`, downloads and PDF against real sites, iframes on
real pages, and the npm packaging path.

`aarch64-apple-darwin` and `aarch64-unknown-linux-musl` are shipped targets that no test has
run on. The macOS job uses `macos-14`, which is ARM, so that target is covered by proxy;
`aarch64-unknown-linux-musl` is not covered at all.

One green run on a platform is not the same as a stable one. Windows has been green once.
