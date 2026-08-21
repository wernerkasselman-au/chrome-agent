# One command vocabulary: design spec

Not built. A narrow fix for one defect, independently landable, and a prerequisite for
nothing. `dispatch-unification.md` proposes collapsing the two dispatch surfaces; this
proposes only that they stop disagreeing about which words exist. The expensive half of
that document (reconciling clap's flag names with pipe's JSON keys) is not needed here.

## What is built

Four lists key off the same command-name strings, and nothing connects them.

| Where | What it decides | Arms |
|---|---|---|
| `pipe.rs::dispatch` | which names pipe mode accepts | 35 |
| `pipe_dispatch.rs::dispatch_single` | which names batch accepts | 34 |
| `pipe_report::mutates_page` | which names get a change report | 20 strings |
| `cli.rs` clap attributes | which names the CLI accepts | per variant |

The first two are the same list. Their arm sets differ by exactly one entry: `pipe.rs` has
`"batch"`, because a batch may run inside a pipe session but not inside another batch.
Every other arm is identical, and each dispatches to the same `dispatch_*` function.

`pipe_report.rs` states the intended contract in its module docs:

> keeps `mutates_page` the only thing a new command has to be added to

That is already untrue. A new command must be added to both dispatch matches and to
`mutates_page`, and nothing checks that the three agree.

## The defect, in two observed symptoms

**One: three classifications with nothing to classify.** `mutates_page` returns `true` for
`tap`, `double_click` and `double-click`. Neither dispatch match has an arm for them.

```
$ echo '{"cmd":"tap","uid":"n1"}' | chrome-agent pipe
{"error":"Unknown command: tap","ok":false}
```

Unreachable, therefore harmless. Harmless in that direction only. The reverse gap, a name
a dispatcher accepts that `mutates_page` does not know, produces a command that dispatches,
mutates the page, and answers `ok:true` carrying no `changed`, no `delta` and no verdict,
because `attach_change_report` never runs for it. The response is then indistinguishable
from `--verdict off`, which is exactly the four-way ambiguity `verdict.rs` was written to
remove: the absence of those fields would once again mean reporting disabled, no baseline,
the read failed, or the page genuinely did not move.

**Two: an error that says something false.** Because the two dispatch matches share only a
catch-all, a `batch` nested inside a `batch` falls through to the same arm an unknown word
does:

```
$ echo '{"cmd":"batch","commands":[{"cmd":"batch","commands":[]}]}' | chrome-agent pipe
{"ok":false,"results":[{"error":"Unknown command: batch","ok":false}]}
```

`batch` is a known command. It is not nestable. The response states the first thing, and
the caller's recovery for "unknown command" (check the spelling, consult the guide) is not
the recovery for "not valid here" (hoist the commands into the outer batch). This is the
same class the rest of the codebase spends its effort on: a true-sounding answer that sends
the reader the wrong way.

## Proposal: one canonical identity, matched exhaustively

Introduce a plain enum that names each command once, independent of spelling, and make
every list downstream a match over it rather than over a string.

```rust
// src/verb.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb { Assert, Back, Batch, Check, Click, Console, Dblclick, /* ... */ }

impl Verb {
    /// Every spelling this verb answers to. The one place an alias is written.
    pub const fn names(self) -> &'static [&'static str] {
        match self {
            Self::Click    => &["click", "tap"],
            Self::Dblclick => &["dblclick", "double_click", "double-click"],
            Self::FillForm => &["fill-form", "fill_form", "fillform"],
            Self::Goto     => &["goto"],
            // ...
        }
    }

    /// Whether the page may have moved because of this verb.
    pub const fn mutates_page(self) -> bool {
        match self {
            Self::Click | Self::Dblclick | Self::Fill | Self::FillForm
            | Self::Type | Self::Press | Self::Select | Self::Check
            | Self::Uncheck | Self::Upload | Self::Drag | Self::Hover
            | Self::Scroll | Self::FillAndSubmit => true,

            // `goto` is deliberately excluded: `landed` is self-describing, and a
            // `navigated` verdict on a command whose purpose is to navigate says nothing.
            Self::Goto | Self::Back | Self::Forward | Self::Inspect | Self::Assert
            | Self::Batch /* ... */ => false,
        }
    }
}
```

Both dispatch sites then parse once and match on `Verb`. The string appears exactly twice
in the whole program: in `names()`, and in the error printed when `FromStr` fails.

### The single most important constraint

**Neither match may carry a `_` arm.** A wildcard in `mutates_page` silently classifies
every future command as non-mutating, which is the unsafe direction described above, and it
reintroduces the defect while looking like a tidier version of the fix. The absence of the
wildcard is the entire enforcement mechanism. It is worth a comment at both matches saying
so, because it is the kind of line a later cleanup removes.

## What becomes impossible

- **A dispatchable name that nobody classified.** Classification happens on the verb, not
  the spelling, so it cannot be missed for one alias of a command that has several. This is
  the failure that would have cost a silent change report.
- **A classified name nobody dispatches.** The `tap` entries stop being expressible: a verb
  either has arms in both dispatch matches or the code does not compile.
- **The two dispatch surfaces drifting.** Both become exhaustive over `Verb`, so adding a
  command breaks compilation in both places until it is handled in both.
- **`batch` reading as unknown.** `dispatch_single` is forced to say something about
  `Verb::Batch` explicitly. The obvious thing to say is that it is not nestable, which
  replaces a false error with a true one at no extra cost.

## What this does not fix

Argument extraction is still written twice, once against clap's typed fields and once
against `serde_json::Value`. `run.rs::run` keeps its fan-out complexity score of 302. Those
are what `dispatch-unification.md` is for. This proposal deliberately stops at the
vocabulary, because the vocabulary is where the silent failure lives and it can be fixed
without touching a single argument.

## Completeness of the verb list

`names()` and `mutates_page()` are exhaustive, so the compiler guarantees every verb
declares spellings and a classification. `FromStr` needs to iterate the verbs, and a
hand-written `ALL` array is the one list a compiler cannot check.

Recommended: a small `macro_rules!` that takes the variant list once and generates the
enum plus `ALL`. Dependency-free, and it confines generation to the boring half.
`mutates_page` stays hand-written and exhaustive, so the semantic decision (does this move
the page) remains readable in source rather than hidden in a table. Splitting it that way
keeps `grep mutates_page` useful, which matters in a codebase whose design notes are read
as often as its code.

Rejected: hand-maintaining `ALL` and guarding it with a test. Every formulation of that
test is circular, since it can only iterate the list it is trying to prove complete.

## Tests worth adding with it

- **Every name round-trips.** For each verb, each of its `names()` parses back to it.
- **The two surfaces accept the same vocabulary**, with `Batch` the one documented
  exception. This is the test that would have caught `tap` on the day it was written.
- **The CLI and pipe agree on spellings**, or the difference is stated. `tap` is a clap
  alias today and not a pipe name; that may be intentional, but nothing records it.
- **A nested batch is refused by name.** Asserting on the error text, so the improvement in
  symptom two cannot silently regress.

## Migration

One slice, mechanical, no behaviour change except the nested-batch wording:

1. Add `src/verb.rs` with the enum, `names()`, `mutates_page()`, `FromStr`.
2. Parse `cmd_name` into a `Verb` at the top of `pipe::dispatch` and
   `pipe_dispatch::dispatch_single`; leave the arm bodies untouched, changing only what
   they match on.
3. Point the four `mutates_page` call sites at the method.
4. Delete `pipe_report::mutates_page` and the three dead alias entries with it.
5. Update the `pipe_report.rs` module note, which currently promises something the code did
   not deliver.

Step 2 is the whole diff. Steps 4 and 5 are deletions.

## Open question for the maintainer

Whether `tap`, `double_click` and `double-click` are meant to be pipe-addressable. If yes,
`mutates_page` was right and the dispatchers are what is missing, and this change should
add the spellings to `names()`. If no, they should be dropped. Nothing in the repository
records the intent, and `mutates_page` is the only evidence either way. The proposal works
under either answer; it just needs one.
