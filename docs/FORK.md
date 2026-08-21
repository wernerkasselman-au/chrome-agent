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

## Verification status

Run on Linux, against a real Chrome:

- 608 tests, 40 binaries, **0 skips** under `CHROME_AGENT_REQUIRE_CHROME=1`
- `clippy --all-targets -- -D warnings` clean, with pedantic and nursery enabled
- The static musl artifact builds, links statically, and drives a live site
- A 61-command pipe session against a live site with no drift or failures

Cross-compiled and lint-gated for `x86_64-pc-windows-msvc`:

- `cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings` clean

One gap, and it needs a click rather than code:

**CI has never run on this fork.** GitHub gates fork workflows behind a one-time enable in
the Actions tab that is not reachable from the API. Everything above was verified on one
developer machine. `ci.yml` now carries a `windows-2022` job and a `workflow_dispatch`
trigger, so enabling Actions once gets both Linux and Windows validation.

Until that first Windows run, the lock change remains reasoned rather than observed:
`File::lock` maps to `LockFileEx`, and Linux behaviour is provably unchanged, but nothing
has executed it there.

The Windows job deliberately does NOT set `CHROME_AGENT_REQUIRE_CHROME` yet. The suite has
never run on that platform, so the first green run is what tells us which tests are actually
portable; turning skips into failures before that is assuming the answer. Tighten it once
there is a baseline.

Untested still: macOS, `--stealth`, `--connect`, `--copy-cookies`, downloads and PDF against
real sites, iframes on real pages, and the npm packaging path.

