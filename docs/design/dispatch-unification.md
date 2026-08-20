# Dispatch unification: design spec

Not built. This is a proposal, in the shape `verdict-taxonomy.md` uses: what is there
today, what the defect actually is, and a staged route with the invariants that may not
move. Nothing here is a request to redesign behaviour. Every response shape stays byte for
byte what it is.

**What is built**: two dispatch surfaces over one set of command modules. `run.rs::run`
matches a typed `Command` enum across 41 arms. `pipe_dispatch.rs::dispatch_single` matches
a `&str` pulled out of a JSON object. Both funnel into `src/commands/`. A third list,
`pipe_report::mutates_page`, keys off the same strings a third time.

**What is not**: any single place where adding a command is one edit the compiler checks.

## The defect is drift, not duplication

Duplication is the symptom and it is survivable. The defect is that nothing connects the
three lists, so they can disagree silently, and they already do.

`mutates_page` classifies `tap`, `double_click` and `double-click` as page-mutating.
`dispatch_single` has no arm for any of them. Measured against the built binary:

```
$ echo '{"cmd":"tap","uid":"n1"}' | chrome-agent pipe
{"error":"Unknown command: tap","ok":false}

$ echo '{"cmd":"double_click","uid":"n1"}' | chrome-agent pipe
{"error":"Unknown command: double_click","ok":false}
```

Those three entries are unreachable. That is the harmless direction, and it is harmless by
luck rather than by construction. The same gap the other way round, a dispatcher alias
absent from `mutates_page`, silently turns the change report off for that spelling: the
command runs, answers `ok:true`, and carries no `changed`, no `delta` and no verdict. It
would look exactly like `--verdict off`, which is the ambiguity `verdict.rs` exists to
remove.

`fill-form` / `fill_form` / `fillform` and `fill_and_submit` / `fill-and-submit` are
currently spelled correctly in both places. Six strings, maintained twice, by hand.

## What holds the two surfaces together today

Tests. There is a named parity test for each behaviour the split could break:

```
pipe_and_cli_agree_when_the_read_fails
pipe_says_value_kept_the_way_the_cli_does
pipe_says_not_kept_the_way_the_cli_does
pipe_spells_every_verdict_the_way_the_cli_does
pipe_names_the_node_for_every_targeted_command
pipe_echoes_the_resolved_uid_too
pipe_reports_the_lost_value_the_way_the_cli_does
pipe_reports_the_missing_baseline_too
pipe_says_it_did_not_look_when_the_report_is_off
cli_pipe_and_batch_report_the_same_landing
```

This works, and it is why the surfaces agree today. It is also the cost: every one of those
tests checks something a type could have checked, and the list only covers the behaviours
somebody thought of. `tap` is what the gap looks like when nobody thought of it.

## Proposal: the typed enum becomes the single source

`Command` already carries all 39 commands and every field, because clap needs it to. Add a
serde derive beside the clap derive and let pipe parse into the same type:

```rust
#[derive(Parser, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command { ... }
```

`{"cmd":"click","uid":"n1"}` then deserializes to `Command::Click { uid: Some("n1"), .. }`,
and three things follow:

1. **One dispatcher.** Argument extraction, defaults and response shaping happen once.
2. **`mutates_page` becomes an exhaustive method** on `Command`. A new variant nobody
   classifies is a compile error, not a silent `false`.
3. **Aliases live once, on the variant.** `#[command(alias = "tap")]` and
   `#[serde(alias = "tap")]` sit on the same line of the same enum. The drift above becomes
   unrepresentable.

### Why this direction

The opposite route, having the CLI build a JSON object and call `dispatch_single`, was
considered and is worse. It throws away clap's typed parse, its defaults, and the
`global = true` handling with the `local.or(global)` rule for `--timeout` and
`--max-depth`, and it puts a serialize plus a deserialize on the path of every CLI
invocation. JSON to typed adds one deserialize to a path that is already parsing JSON.

## Staging

A big-bang rewrite of the most behaviour-dense code in the repository is what
`review-findings.md` warns against. Each slice below stands alone and is separately
revertible.

**Slice 0, characterization, no behaviour change.** Add the serde derive and a test that
every command name `dispatch_single` accepts deserializes into the variant it currently
routes to, and that the six CLI-only commands (`close`, `daemon`, `pipe`, `replay`,
`status`, `stop`) are refused by pipe exactly as they are today. This proves the mapping is
total *before* anything moves, and it closes the `tap` class of gap permanently.

This slice is worth landing on its own merits even if nothing after it is built.

**Slice 1, pipe parses into `Command`.** `dispatch_single` deserializes, then calls the
existing per-command dispatchers with typed arguments. The string match is gone; both
dispatcher bodies remain.

**Slice 2, `mutates_page` becomes an exhaustive method.** Deletes the third list.

**Slice 3, collapse the dispatcher bodies one family at a time**, ordered by blast radius:
read-only commands first (`inspect`, `text`, `read`, `tabs`, `diff`), then targeted
actions, then the composites (`fill_and_submit`, `navigate_and_read`, `batch`).

**Slice 4, `run.rs::run` becomes a shell**: merge global flags, connect, dispatch, output.
Its fan-out complexity score is 302 against 59 for the next symbol in the repository. This
is the slice that moves that number, and it should be last, not first.

## Invariants

These are contracts, and no slice may move them. Each already has a test.

- `assert` exit codes: `0` held, `2` did not hold, `1` unanswerable. `NotHeld` travels
  through the error channel and `main` recognises it before its generic handler.
- `goto` stays out of `mutates_page` and out of the verdict machinery. `landed` is
  self-describing.
- The `value:{}` object has exactly one key, because `postcondition_from_response` reads
  exactly one. A second key for the same idea is a second reader that can fall out of step.
- `--verdict off` skips the post-action read entirely and restores the pre-0.8 output and
  latency.
- A failed post-action read is not a failed action. Pipe stated this policy first and the
  CLI was aligned to it; the merged path keeps pipe's.
- Text and JSON output shapes, including which lines text mode prints only when the page
  did not keep the write.
- `local.or(global)` for the two flags that cannot be `global = true` (`--timeout` and
  `--max-depth`), including `wait`'s own 10s default against the global 30s.

## Known risks

- **Field naming.** clap spells flags in kebab (`--max-depth`); pipe JSON uses snake
  (`max_depth`). Reconciling needs per-field serde attributes. Mechanical, wide, and
  compiler-checked, but it is the bulk of the diff.
- **Commands pipe must refuse.** Six variants have no pipe meaning. They need an explicit
  guard plus the Slice 0 test, not a silent fallthrough.
- **Unknown-field policy.** `deny_unknown_fields` would make pipe stricter than it is now.
  Decide deliberately; tolerating extra keys may be the kinder contract for an agent, and
  either way it is a behaviour change and belongs in its own slice.

## Recommendation

Land Slice 0. It is small, purely additive, and it closes the `tap` class of defect on its
own. Sequence Slices 1 through 4 after a feature slice rather than before one, because the
diff touches every command and reviewing it against a moving target is the expensive way to
do it.
