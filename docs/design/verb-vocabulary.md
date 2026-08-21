# One pipe vocabulary: design spec

Not built. A narrow fix for one class of drift, independently landable.
`dispatch-unification.md` proposes collapsing the CLI and JSON surfaces; this proposes only
that the JSON surfaces stop disagreeing about which words exist, and it does not need any
of that document's expensive half.

Revised after review. The first draft claimed more than the fix delivers, and named the
symptom wrongly. Both corrections are recorded below rather than quietly edited out,
because the overclaim is the more instructive half.

## What is built

Command-name strings are matched in four places, and nothing connects them.

| Where | Decides | Size |
|---|---|---|
| `pipe.rs::dispatch` (`src/pipe.rs:237`) | which names pipe accepts | 35 arms |
| `pipe_dispatch.rs::dispatch_single` (`src/pipe_dispatch.rs:740`) | which names batch accepts | 34 arms |
| `pipe_report::mutates_page` (`src/pipe_report.rs:226`) | which names get a change report | 20 spellings |
| `cli.rs` clap attributes | which names the CLI accepts | per variant |

The first two are the same list. Their arm sets differ by exactly one entry: `pipe.rs:278`
has `"batch"`, because a batch may run inside a pipe session but not inside another batch.
Every other arm dispatches to the same `dispatch_*` function.

Five spellings are hand-maintained across three of those matches: `fill-form` /
`fill_form` / `fillform`, and `fill_and_submit` / `fill-and-submit`, at `src/pipe.rs:254`
and `:274`, `src/pipe_dispatch.rs:768` and `:775`, and `src/pipe_report.rs:233` and `:234`.

`pipe_report.rs` states the intended contract in its own module docs, "keeps
`mutates_page` the only thing a new command has to be added to". That is already untrue: a
new command must be added to both dispatch matches and to `mutates_page`.

## The defect, in two observed symptoms

**One: three classifications with nothing to classify.** `mutates_page` returns `true` for
`tap`, `double_click` and `double-click`. Neither dispatch match has an arm for them.

```
$ echo '{"cmd":"tap","uid":"n1"}' | chrome-agent pipe
{"error":"Unknown command: tap","ok":false}
```

Unreachable, therefore harmless. Harmless in that direction only.

**What the reverse gap actually costs.** The first draft of this document said a
dispatchable mutating name absent from `mutates_page` would be "indistinguishable from
`--verdict off`". That is wrong, and the code says so plainly at `src/pipe.rs:295`:

```rust
// `--verdict off` is a decision, not an observation. Saying so costs two fields and no
// page read, and it is the difference between "I did not look" and "nothing moved".
if !report.changes && crate::pipe_dispatch::mutates_page(cmd_name) {
```

A classified command with reporting off still gets a verdict, `not_checked` /
`reporting_disabled`. An unclassified command gets **no verdict field at all**, which is
how a read answers. So the failure is not a command that declines to observe. It is a
mutating command wearing the response shape of `inspect` or `tabs`. Still a false claim
about what happened, and still the thing to prevent, but a different false claim than the
one first written down.

**Two: an error that says something false.** Because the two dispatch matches share only a
catch-all, a `batch` nested inside a `batch` falls through the same arm an unknown word
does (`src/pipe_dispatch.rs:778`):

```
$ echo '{"cmd":"batch","commands":[{"cmd":"batch","commands":[]}]}' | chrome-agent pipe
{"ok":false,"results":[{"error":"Unknown command: batch","ok":false}]}
```

`batch` is a known command that is merely not nestable. The recovery for "unknown command"
(check the spelling) is not the recovery for "not valid here" (hoist the commands into the
outer batch).

## Decision: delete the three names

The first draft left this open. It should not have, because the repository does record the
intent for `tap`, in two places:

- `#[command(alias = "tap")]` on `Command::Click` at `src/cli.rs:170`
- the alias note in `CLAUDE.md`: `navigate/open/go, snap/snapshot/tree, js/execute, capture, tap`

And the decisive fact is the convention around it: **pipe accepts none of clap's
convenience aliases.** Not `navigate`, `open`, `go`, `snap`, `snapshot`, `tree`, `js`,
`execute`, `capture`, or `tap`. Every one is CLI-only, and pipe matches canonical names
only. `tap`'s absence from pipe is that convention holding, not an omission.

`double_click` and `double-click` have no support anywhere: no clap alias on
`Command::Dblclick` (`src/cli.rs:254`), and the README, `skills/chrome-agent/SKILL.md` and
the tests all use `dblclick`.

So: delete all three from the classification, add none of them to the pipe vocabulary, keep
`tap` as the CLI alias it already is. Adding only `tap` to pipe would create arbitrary
partial parity while the other convenience aliases stayed CLI-only.

## Proposal: one canonical pipe identity, matched exhaustively

Introduce an enum naming each pipe command once, independent of spelling, and make every
list downstream a match over it rather than over a string.

Call it **`PipeVerb`**, not `Verb`. It is the identity of a wire-protocol command, and the
CLI's `Command` is a different thing with a different shape (see below). A name that
implied one shared notion of "the commands" would be claiming a unification that does not
exist.

```rust
// src/pipe_verb.rs
pub enum PipeVerb { Assert, Back, Batch, Check, Click, /* ... */ }

impl PipeVerb {
    /// Every spelling this verb answers to. The one place a pipe alias is written.
    pub const fn names(self) -> &'static [&'static str] { /* exhaustive */ }

    /// Whether this verb owes the caller an action change report.
    pub const fn requires_change_report(self) -> bool { /* exhaustive */ }
}
```

**`requires_change_report`, not `mutates_page`.** The rename is not cosmetic; see the
misclassification hole below. The question the code actually asks is whether a report is
owed, and the current name invites answering a different question.

### The enforcement mechanism

Neither match may carry a `_` arm. A wildcard in the classification silently answers "no
report" for every future command, which is the unsafe direction, while looking like a
tidier version of the fix.

Do not rely on a comment asking future maintainers not to add one. Deny it:

```rust
#![deny(clippy::wildcard_enum_match_arm)]
```

CI already runs `clippy -D warnings` with pedantic and nursery enabled, so a lint is
enforcement and a comment is a request.

Likewise, `names()` being exhaustive does not stop it returning `&[]`, and two variants can
claim the same spelling. Generate `FromStr` as a direct string match rather than by
iterating, and let `#[deny(unreachable_patterns)]` reject a duplicate spelling at compile
time.

## What this fixes, precisely

- A dispatchable name nobody classified. Classification happens on the verb, not the
  spelling, so it cannot be missed for one alias of a command that has several.
- A classified name nobody dispatches. The `tap` entries stop being expressible.
- The two JSON dispatch surfaces drifting, since both become exhaustive over `PipeVerb`.
- `batch` reading as unknown, because `dispatch_single` is forced to say something explicit
  about `PipeVerb::Batch`, and the obvious thing to say is true.

## What this does not fix, and must not be claimed to

Three holes survive it. The first draft implied the exhaustive match delivered the safety
property outright. It does not.

**Errors bypass the report hook entirely.** Both dispatchers return before the report and
verdict are attached (`src/pipe.rs:285`, `src/pipe_dispatch.rs:783`):

```rust
match result {
    Ok(v) => v,
    Err(e) => { /* build {"ok":false,"error":...} */ return obj; }
}
```

So a correctly classified mutating verb can mutate and still answer with no report and no
verdict. This is reachable today: `dispatch_fill_and_submit` fills each field, clicks
submit at `src/pipe_dispatch_actions.rs:46`, and can then fail its requested wait through
`?`. The submit happened; the response says nothing about it. `fill-form` has the same
shape, interleaving validation and mutation in one loop.

No enum fixes a control-flow hole. The hook has to run for a report-bearing verb even when
the dispatcher returns `Err`, unless there is positive evidence nothing was dispatched.
That distinction (refused before dispatch, versus may already have mutated) probably wants
to be in the dispatcher's return type.

**The classification is already semantically wrong.** Exhaustiveness guarantees somebody
wrote a boolean, not that the boolean is right. `eval` executes arbitrary caller-supplied
JavaScript (`src/pipe_dispatch.rs:258`) and is absent from `mutates_page`, so it can click,
submit, navigate or rewrite the DOM and owe no report. `back` and `forward` genuinely
navigate (`src/pipe_dispatch.rs:409`, `:434`) and are classified false. Some of those may
be deliberate, in the way `goto` is deliberately excluded because `landed` is
self-describing. None of them is written down.

**CLI reporting is a fifth, implicit classification.** On the CLI the decision is made by
which `run.rs` arm calls `output_action`, for example at `src/run.rs:242` and `:274`, while
`wait` deliberately prints plainly at `src/run.rs:722`. `PipeVerb` does not see that, so a
new CLI action can still omit the verdict machinery.

## Why a third type is justified

The obvious objection is that adding `PipeVerb` beside clap's `Command` creates one more
thing to keep in sync. It does, and it is still right, because the two are not the same
shape and the evidence is unambiguous:

- `navigate_and_read` and `fill_and_submit` exist in pipe (`src/pipe.rs:273`) and have **no
  `Command` variant at all**.
- CLI `fill-form` takes positional `uid=value` strings (`src/cli.rs:207`); pipe takes an
  array of `{uid,value}` objects (`src/pipe_dispatch_actions.rs:89`).
- CLI `Batch` carries only `stop_on_error` and reads stdin (`src/cli.rs:589`); pipe's batch
  carries a `commands` array (`src/pipe_dispatch.rs:688`).
- CLI `assert` is a clap subcommand enum (`src/cli.rs:493`); pipe parses a flattened shape
  (`src/pipe_dispatch_actions.rs:281`).

`PipeVerb` is honest about being the wire protocol's vocabulary. The longer-term shape, if
the two are ever unified, is a shared operation type that `CliCommand` and `PipeRequest`
both convert into, not one of them pretending to be the other.

## Tests worth adding

- Every name round-trips: for each verb, each of its `names()` parses back to it.
- The two JSON surfaces accept the same vocabulary, with `Batch` the one documented
  exception.
- **CLI versus pipe vocabulary, from the real parser.** `clap::CommandFactory` can
  enumerate the actual subcommand names and aliases, so the test compares generated output
  rather than a hand-copied list. Policy it should assert: canonical names shared,
  convenience aliases CLI-only, the two pipe-only composites and the CLI-only
  process-management commands documented. This is the test that answers the `tap` question
  permanently instead of one alias at a time.
- A nested batch is refused by name, asserting on the error text.

Note what none of these would have caught. A test over names `dispatch_single` accepts can
never examine `tap`, because `tap` is precisely a name it does not accept. Finding that
class of dead entry needs the test to start from the classification, not from the
dispatcher.

## Migration

1. Add `src/pipe_verb.rs`: the enum, `names()`, `requires_change_report()`, `FromStr`.
2. Parse `cmd_name` into a `PipeVerb` at the top of `pipe::dispatch` and
   `pipe_dispatch::dispatch_single`; leave the arm bodies untouched.
3. **Collapse the two JSON dispatchers.** This is cheaper than it looks: `run_batch`
   already calls `dispatch_single` (`src/pipe_dispatch_actions.rs:322`). Have `pipe::dispatch`
   special-case top-level `Batch` and delegate every other verb to `dispatch_single`, with
   nested `Batch` answering that batch is not nestable.
4. Point the four `mutates_page` call sites at the method; delete `pipe_report::mutates_page`
   and the three dead entries with it.
5. Correct the `pipe_report.rs` module note, which promises something the code did not
   deliver.

Steps 3 and 4 are the substance. Step 5 is a deletion.

The two holes above (error paths, and the `eval` / `back` / `forward` classification) are
deliberately **not** in this migration. They are separate defects that this change makes
easier to state but does not resolve, and folding them in would hide two behaviour changes
inside a refactor.
