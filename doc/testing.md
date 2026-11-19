# Testing

## What is tested where

| Area | How |
|---|---|
| herdr calls | the pure parsers (`panes_on_tab`, `pane_title`, `launch_context`, …) fed with socket replies recorded from herdr 0.8 |
| Rex calls | a **recording fake runner**: each test queues CLI replies and checks the exact argument lists. Fixtures for the formats: `tests/fixtures/rex/session-inspect.json` (`rex session inspect --json`) and `topology-do.json` (the topology script's output) |
| detection | `detect_kind_from`, the pure decision with env values passed in (no env mutation, no test races) |
| consumers' logic | `fake::FakeHost`, below |
| a real Rex server | the ignored live test, below |

## The feature matrix

Feature-gated code only builds with its feature on, so every
combination is checked. CI and `just check` run the same matrix:

```sh
cargo test                       # core only (flash, zextract)
cargo test --features topology
cargo test --all-features        # navigate (nav)
# same three for: cargo clippy --all-targets <features> -- -D warnings
```

Tests that use topology or navigate APIs live in gated modules (for
example `rex::tests::snapshot`, gated on `topology`). Read and insert
tests allow for the block-session lookup, which only runs with
`topology`: `.lookup()` queues an empty snapshot reply, and `L` is the
index of the first real CLI call.

## `fake::FakeHost`, for consumers

An in-memory `Host` for a tool's own unit tests:

| Field / method | Purpose |
|---|---|
| `FakeHost::new(source_pane, tab_id)` | empty host with that source |
| `.with_pane(id, title, text)` | adds a pane to the tab and gives it text |
| `texts`, `tab` | canned pane text; what `list_tab_panes` returns |
| `reads`, `inserts` | every `read_pane` / `insert_text`, recorded |
| `insert_error` | makes `insert_text` fail |
| *(topology)* `FakeHost::with_topology(sessions, windows, panes, agents)`, plus `sessions`, `windows`, `all_panes`, `agents` | canned topology |
| *(navigate)* `jumps`, `jump_fail`, `closes`, `renames` | recorded navigation; `jump_fail` makes `jump` fail. A successful fake jump returns `Executed`. |

herdr-nav's tree tests run against it, with nav's own traits
implemented for `FakeHost` in nav's tests.

## The Rex runner fake

`RexRunner` is a public trait, so a consumer can fake the CLI for its
own Rex glue the same way muxhost's tests do:

```rust
#[derive(Clone, Default)]
struct FakeRunner {
    calls: Rc<RefCell<Vec<(Vec<String>, Option<String>)>>>,
    replies: Rc<RefCell<VecDeque<Result<String, String>>>>,
}
impl RexRunner for FakeRunner { /* push the call, pop the next reply */ }
let host = RexHost::new(Box::new(runner.clone()), "session:s1".into(), source);
```

Keep the state behind `Rc`. `RexHost::new` takes the runner by value,
so without the `Rc` the test's copy wouldn't see the calls.

## Live test against a real Rex server

```sh
REX_CLI="$HOME/Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex" \
REX_SESSION="session:<an id from rex ls --json>" \
cargo test --all-features live_rex -- --ignored --nocapture
```

It runs the topology script through `rex do -` and prints each
session's panes with their cwd and titles.

## Re-recording fixtures

When Rex's output changes:

```sh
rex session inspect <session> --json > tests/fixtures/rex/session-inspect.json
rex do herdr-nav/tests/fixtures/rex/nav-s4-topology.lua | python3 -m json.tool > tests/fixtures/rex/topology-do.json
```

Then update the counts and ids that the tests assert on.
