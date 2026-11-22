# Topology: sessions, windows, panes, agents

> Module: `topology` · Feature: `topology` · Used by: nav

## Intent

A switcher that shows every session, window and pane, plus the agents,
needs **all of it in one go**, with the user's own session, window and
pane marked. muxhost returns the same neutral shape on both hosts, so
the tool's tree code doesn't care which multiplexer it runs under.

## API

### Types

| Type | Fields | Meaning |
|---|---|---|
| `SessionInfo` | `session_id`, `label`, `active` | `active`: the session the user is in. At most one. |
| `WindowInfo` | `window_id`, `session_id`, `label`, `active` | `active`: the window the user is in. At most one, and only in the active session. |
| `PaneEntry` | `pane_id`, `session_id`, `window_id`, `title`, `is_focused`, `cwd` | every pane in every session, floating panes left out. `title`: user label → terminal title → generic. `is_focused`: the pane the user is in. `cwd`: the working directory, where the host reports one. |
| `AgentEntry` | `pane_id`, `terminal_id`, `name`, `title`, `status` | a pane running an agent |
| `AgentStatus` | `Waiting`, `Working`, `Done`, `Error`, `Idle`, `Unknown` | `Waiting` = waiting for the user, `Done` = finished, result ready, `Error` = failed and stopped. `parse` / `as_str` convert from and to `"waiting"`, `"working"`, `"done"`, `"error"`, `"idle"`, `"unknown"`. |
| `TopologySnapshot` | `sessions`, `windows`, `panes`, `agents` | all four at once |

### The `HostTopology` trait (extends `Host`)

| Method | Contract |
|---|---|
| `list_sessions()` | every session, in the host's order |
| `list_windows()` | every window of every session |
| `list_all_panes()` | every pane of every session |
| `list_agents()` | every agent pane |
| `topology_snapshot()` *(default)* | all four. The default calls the four lists; a host where a round trip is expensive overrides it, and the lists then read from the same snapshot. **Tree-building tools should use this.** |

## herdr implementation

herdr has a socket call for each list, and each takes milliseconds, so
`topology_snapshot` uses the default (four calls).

| List | Socket call | Mapping |
|---|---|---|
| `list_sessions` | `workspace.list` → `workspaces[]` | `workspace_id`; `label` (falls back to the id); `active` = `focused` |
| `list_windows` | `tab.list` → `tabs[]` | `tab_id`, `workspace_id`; `label` (falls back to the id); `active` = `focused` |
| `list_all_panes` | `pane.list` → `panes[]` | `workspace_id`, `tab_id`; title `label` → `terminal_title_stripped` → `pane <id>`; `is_focused` = `focused`; `cwd` (`null` or empty → `None`) |
| `list_agents` | `agent.list` → `agents[]` | `terminal_id`; `name` = `agent`; title `terminal_title_stripped` → `agent`; `status` from `agent_status` |

## Rex implementation

### Why one script

Building this from separate CLI calls (`rex ls`, then
`session inspect` per session, then `block call title`, `pwd` and
`program_status` per block) means one process spawn and server round
trip per call: dozens for an ordinary setup. A popup should build its
tree in under 150 ms (herdr-nav spike S4), and that doesn't fit.

So `RexHost` pipes **one Lua script to `rex do -`**, which runs on the
server, walks everything, and returns a single JSON document. It takes
**about 50 ms for several sessions** (measured live, rex
0.1.0-preview.19). A copy is kept as
herdr-nav's `tests/fixtures/rex/nav-s4-topology.lua`.

### The script (`TOPOLOGY_SCRIPT`)

```
session.list
for each session:
    session.attach { session_id }   -- returns the current view inline
    session.detach { session_id }
    for each window in view.windows:
        for each layer with kind == "tiled":     -- floating layers (popups) skipped
            for each block:
                block.method { method = "title" }
                block.method { method = "pwd" }
                block.method { method = "program_status" }
return { sessions = [ { session_id, label, active_window_id,
                        windows = [ { window_id, label, focused_block_id,
                                      blocks = [ { block_id, label, title, pwd, program_status } ] } ] } ] }
```

What the script relies on (each verified live):

- `rex.call` reaches every server method from a `rex do` script.
- `rex.session.view` returns nil without an attachment.
  `session.attach` returns the view inline, so the script attaches and
  detaches per session.
- Block methods use their **short** names (`title`, not
  `com.superlogical.terminal.title`).
- Every call is wrapped in `pcall`. A session whose attach fails comes
  back as `{session_id, label, error}` with no windows, and the tree
  shows it empty instead of failing. Only a failing `session.list`
  makes the whole snapshot an error.

### Parsing (`parse_topology`)

| Output | Rule |
|---|---|
| `SessionInfo.active` | `session_id == REX_SESSION` |
| `WindowInfo.active` | only in the active session: the window holding `<PREFIX>_SOURCE_BLOCK`, else the session's `active_window_id` |
| `PaneEntry.title` | **user label** if it isn't Rex's default `shell` → terminal title → `shell` → `pane <id>`. (Before v0.3.5 the terminal title won, so renamed panes showed as `~`.) |
| `PaneEntry.is_focused` | `block_id == <PREFIX>_SOURCE_BLOCK`: the pane the user is in. This is more useful to a switcher than each window's own `focused_block_id`. |
| `PaneEntry.cwd` | `pwd` returns a `file://<host>/<path>` URL. Everything from the first `/` after the host is kept; empty → `None`. |
| `AgentEntry` | a block whose `program_status.records` is non-empty (programs report status with OSC 7501). The first record's `state` (verified live, rex 0.1.0-preview.22; a spike-era `status` spelling is still accepted) maps via `AgentStatus::parse`: `blocked` → `Waiting`, `working` → `Working`, `done` → `Done`, `error` → `Error`, `idle` → `Idle`, anything else → `Unknown`. `name` from the record's `app` (then `name`, then `agent`); `terminal_id` = `block_id`. gopher-lua encodes an empty table as `{}`, so `records` may be an array or an object, and both empty forms mean "no agent". |

### The cache

One `RefCell<Option<(Instant, TopologySnapshot)>>` per host:

- All four list methods and `topology_snapshot` read from it.
- It is good for **250 ms** (`SNAPSHOT_TTL`), so a tool's tree build
  and its first preview share one script run, while changes made
  elsewhere (in the app, another window) show up quickly.
- It is **dropped right away** after every change this host makes:
  `close_*`, `rename_*`, and every `run` escape-hatch call. Without
  that, a session the tool had just created with `rex new` wasn't in
  the cache, and the jump to it failed with "no session holds block".
- `block_session(block)`, used by block calls in
  [`core.md`](core.md#rex-implementation), reads the same cache.

## Consumer contract

- Build trees from `topology_snapshot()`, not from the four lists
  (under Rex, one script run either way, but the intent is clearer).
- Don't hold a snapshot across user actions. Re-read after a change;
  under Rex the cache makes that cheap.
- Look panes up by `pane_id`, not by title: titles aren't unique.
- Expect empty sessions: a failed attach is an empty session, not an
  error.

## Tests

`rex.rs::tests::snapshot`, against the recorded `topology-do.json`
(three sessions, four windows, six blocks): parsing titles, cwd and
the active flags; the cache (one `rex do -` serves every list); the
user's window marked active (source block vs `active_window_id`);
user labels winning over terminal titles; `program_status` records
in the live OSC 7501 shape (`state` + `app`, plus `done` and `error`)
and the spike-era `status` fallback. There is also an ignored live
test, `live_rex_topology_snapshot`, see [`testing.md`](testing.md).
`herdr.rs::topology_tests` uses socket replies recorded from herdr 0.8.

## Known limits

- **Rex agents are verified live** against rex 0.1.0-preview.22 with
  a reporting pi (records use `state` and `app`). The spike-era
  defensive parsing stays as a fallback. `done` and `error` records
  survive a finished run, so finished and failed agents stay listed
  until something replaces the record.
- **Rex `REX_SESSION` must be an id.** A label still works for CLI
  calls but won't match `session_id`, so no session is marked active.
- **herdr `active` comes from herdr's `focused`,** which marks the
  focused workspace or tab, not necessarily the tool's source pane.
