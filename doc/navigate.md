# Navigate: jump, close, rename

> Module: `navigate` · Feature: `navigate` (turns on `topology`) ·
> Used by: nav

## Intent

Take the user somewhere (any pane, in any session) and change the
layout (close, rename). The two hosts differ here more than anywhere
else. herdr switches workspace, tab and pane in **one server call**.
Under Rex, the server can only move focus. Which session the macOS
**app** shows is decided by the app, and only a client action can
change it. So `jump` reports **how far it got**, and under Rex the tool
does the rest.

## API

| Item | Meaning |
|---|---|
| `Jump::FocusPane { pane_id }` | land on this pane, in whatever session it is in |
| `Jump::SelectSession { session_id, focus_pane: Option<String> }` | switch to this session, optionally to a given pane |
| `JumpOutcome::Executed` | the host did the whole jump; nothing is left for the tool to do |
| `JumpOutcome::Delegated` | the host did the **server side** (the target pane is focused in its session); **the app's view is the tool's job** (see the contract below) |

### The `HostNavigate` trait (extends `Host`)

| Method | Contract |
|---|---|
| `jump(jump) -> Result<JumpOutcome, String>` | see above |
| `close_pane(pane)` / `close_window(window)` / `close_session(session)` | close it, and everything in it. The host may refuse (for example the last pane of a window) and returns why. |
| `rename_pane(pane, label)` / `rename_window(window, label)` | set the label the user sees |

## herdr implementation

| Method | Socket call | Outcome |
|---|---|---|
| `jump FocusPane` | `pane.focus {pane_id}`: switches workspace, tab and pane in one call | `Executed` |
| `jump SelectSession` | the given pane, else the session's first pane from `list_all_panes`, then `pane.focus` | `Executed` |
| `close_pane` / `close_window` / `close_session` | `pane.close {pane_id}` / `tab.close {tab_id}` / `workspace.close {workspace_id}` | |
| `rename_pane` / `rename_window` | `pane.rename {pane_id, label}` / `tab.rename {tab_id, label}` | |

## Rex implementation

### Why the jump is split

| Who | Can do | Can't do |
|---|---|---|
| the Rex **server** (what the CLI talks to) | focus a block in its session (`rex focus`); make its window the session's active window | make the app show a different session or tab |
| the Rex **app** (macOS client) | `session.select {session_id, window_id?}`, `client.tab.*` | — |
| a **CLI process** (the tool) | the server side, and client actions **only through Remote Control**: `rex -C <client> do <action>`, enabled once in the app's settings | queue client actions directly (`rex.client.queue` works only in an app-invoked config action) |

### `RexHost::jump`: the server side

1. Work out the target: `FocusPane` → the block's session, from the
   snapshot. `SelectSession` → the given session and the optional pane.
2. If there is a block: `rex focus -s SESSION BLOCK`, a child CLI call
   that attaches to the target session. (`session.focus_block` from a
   connection attached elsewhere silently does nothing.) That session's
   active window becomes the block's window, and its focused block
   becomes the block.
3. Return **`Delegated`**. A `SelectSession` without a pane does
   nothing on the server and is also `Delegated`.

### Closes and renames

The session is resolved from the snapshot, and the cache is dropped
afterwards.

| Method | CLI call |
|---|---|
| `close_pane` | `rex block close -s S B` |
| `close_window` | `rex window close -s S W` |
| `close_session` | `rex session kill S` |
| `rename_pane` | `rex api call -s S session.set_block_label '{"block_id":B,"label":L}'` |
| `rename_window` | `rex window rename -s S W LABEL` |

## Consumer contract: carrying out a `Delegated` jump

On `Delegated`, the tool has to make the **app** show the target. This
sequence was worked out live, and each step is there because a simpler
one failed. herdr-nav (`src/host_ext/mod.rs`, `host_ext::jump`) is the
reference implementation.

**Inputs:** the app's client id (the launcher passes it, e.g.
`NAV_CLIENT_ID`), and the target's session **and window**, both from
`list_all_panes()`.

| # | Step | Why |
|---|---|---|
| 1 | `host.jump(..)` runs the server side → `Delegated` | the target is focused in its session |
| 2 | **Synchronous** `rex -C <client> do session.select session_id=S window_id=W` | **to catch errors**: a refusal (Remote Control off, unknown client) exits non-zero, and the tool can stay open and say so. It does **not** move the app reliably (step 4). |
| 3 | The tool exits | its popup layer closes |
| 4 | — | the app moves focus back to the pane under the popup when the layer closes, **which undoes any select made while the layer was open** |
| 5 | **About 0.5 s later**, from a **detached** child (`nohup sh -c … &`, outside the popup's process group, so closing the layer doesn't kill it): `session.select session_id=S window_id=W`, **then** `rex focus -s S B` | now in a clean state, the select sticks. `window_id` is needed because the tab strip follows client actions, not server focus. The focus comes **last**, because the app's select lands on the window's previously focused block. |

The tool should also log each step somewhere. Its popup is gone
moments later, so a log file is the only evidence of a failed jump
(nav writes `/tmp/herdr-nav-jump.log`).

**Why the launcher doesn't do this.** An app-invoked Lua action could
queue `session.select` itself, with no Remote Control. But it would
have to wait for the popup to close, and **a blocking action holds the
app's Lua for that whole time: the popup gets no key presses.** So
launchers open the layer and return at once, and the tool does the jump
through Remote Control.

## Tests

herdr: the call shapes are the same as the socket calls in the other
features. Rex (`rex.rs::tests::snapshot`): `jump` runs exactly
`focus -s <block's session> <block>` and returns `Delegated`;
`close_pane` resolves the session from the snapshot and drops the
cache (the next snapshot runs the script again); `rename_window`
arguments. `FakeHost` records jumps, closes and renames for consumers'
tests (see [`testing.md`](testing.md)).

## Known limits

- The app-side steps (2–5) depend on **Remote Control** being enabled
  in the Rex app. Without it, only the server side happens.
- The 0.5 s delay in step 5 is a fixed guess at how long the layer
  takes to close. It worked in every live test but isn't confirmed.
- herdr `SelectSession` without a pane lands on the session's **first**
  pane, not its last-focused one.
