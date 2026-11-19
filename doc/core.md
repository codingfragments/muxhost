# Core: `Host`, `Caps`, detection, clipboard

> Module: crate root and `clipboard` · Feature: always on · Used by:
> zextract, flash, nav

## Intent

The minimum a popup tool needs from any multiplexer:

- the pane it was opened over,
- that pane's text (plain or coloured, the screen or the scrollback),
- the other panes on the same tab,
- typing text into a pane,
- the scroll position, where the host knows it,
- knowing which host it runs under, and what that host can do.

## API

### Types

| Type | Meaning |
|---|---|
| `SourceRef { pane_id, tab_id }` | the pane the tool opened over. `tab_id` is empty when the host can't tell (herdr's dev fallback); tab-wide calls then find nothing. |
| `PaneInfo { pane_id, title }` | one entry of `list_tab_panes` |
| `ReadSource::{Viewport, Scrollback}` | what is on screen / the history with soft-wrapped lines joined, newest last |
| `ReadFormat::{Plain, Ansi}` | styles stripped / raw escape sequences, for coloured rendering |
| `ScrollInfo { offset_from_bottom, max_offset_from_bottom, viewport_rows }` | screen rows, not logical lines. Offset 0 means at the bottom. |
| `Caps` | the optional capabilities, see below |
| `HostKind::{Herdr, Rex}` | the result of detection (`#[non_exhaustive]`) |

### The `Host` trait

| Method | Contract |
|---|---|
| `name() -> &'static str` | `"herdr"` or `"rex"` (tool fallbacks may add their own, e.g. nav's `"none"`). Tools use it for host-scoped data such as template dirs and config tables. |
| `source() -> &SourceRef` | the pane the tool opened over |
| `read_pane(pane, source, format, lines)` | that pane's text. `lines = Some(n)` keeps the newest `n` lines. Works for **any** pane, in any session. |
| `list_tab_panes()` | the panes on the source tab, **source first**, then in the host's order. Floating and popup panes (including the tool's own) are left out. |
| `insert_text(pane, text)` | type `text` as if the user typed it. Exact bytes: no escaping, newlines kept. |
| `scroll_info(pane)` *(default `Ok(None)`)* | the scroll position, or `None` when the host doesn't know it. `None` is normal, not an error. |
| `caps()` *(default all `false`)* | see below |

### `Caps`

| Flag | Meaning | herdr | Rex | What consumers do |
|---|---|---|---|---|
| `scroll_follow` | `scroll_info` returns real offsets | ✅ | ❌ | flash anchors the selection on the viewport, or uses the bottom |
| `legacy_keyboard` | the popup's keyboard runs in legacy mode (duplicate presses) | ✅ | ❌ | debounce identical consecutive keys |
| `plugins` | there is a plugin system (`plugin.list`, action invoke) | ✅ | ❌ | nav hides its Plugins group |
| `agent_release` | an agent can be released from its pane | ✅ | ❌ | nav's `^x` does nothing |

### Detection

| Function | Behaviour |
|---|---|
| `detect_kind(prefix) -> Result<HostKind, String>` | **The one place the detection order is defined.** 1. `<PREFIX>_HOST=herdr\|rex` wins; any other non-empty value is an error. 2. `HERDR_SOCKET_PATH` → herdr. 3. `REX_SESSION` → Rex. 4. Otherwise an error. |
| `detect(prefix) -> Result<Box<dyn Host>, String>` | `detect_kind`, then `HerdrHost::from_env()` or `RexHost::from_env(prefix)` |

**Why herdr comes before Rex:** the innermost multiplexer is the one
hosting the tool. A herdr started in a Rex terminal has both variables:
herdr sets `HERDR_SOCKET_PATH` in its panes, and `REX_SESSION` is
inherited from the outer terminal. The other nesting can't happen this
way, because the Rex server runs under launchd and its terminals don't
inherit a herdr pane's environment. Launchers still set `<PREFIX>_HOST`
explicitly, so none of this has to be guessed.

Tools that build their own host types (herdr-nav needs its own trait
mix and a fallback instead of an error) **call `detect_kind`** rather
than copying the order.

### `clipboard::copy(text)`

1. The system clipboard (`arboard`).
2. If that fails (a headless or remote Linux session behind Rex or
   ssh), an **OSC 52** sequence (`ESC ] 52 ; c ; <base64> BEL`) written
   to `/dev/tty`. The terminal (the Rex app, herdr's client, iTerm, …)
   passes it to the local clipboard if it supports OSC 52.
3. `Err` only if both fail; the message includes both reasons.

Base64 is a small built-in RFC 4648 implementation, so there is no
extra dependency.

## herdr implementation

| Method | Socket call | Details |
|---|---|---|
| `source` | launch context | see [`hosts.md`](hosts.md#environment) |
| `read_pane` | `pane.read {pane_id, source, lines?, format?, strip_ansi?}` | `Viewport` → `"visible"`, `Scrollback` → `"recent_unwrapped"`. `Ansi` adds `format: "ansi", strip_ansi: false`. Returns `read.text`. |
| `list_tab_panes` | `pane.list` | filtered by `tab_id`, source first. Title: `label` → `terminal_title_stripped` → `pane <id>`. |
| `insert_text` | `pane.send_text {pane_id, text}` | |
| `scroll_info` | `pane.get {pane_id}` | `pane.scroll` → `ScrollInfo`; missing fields count as 0. No `scroll` object → `None`. |
| `caps` | | all `true` |

## Rex implementation

| Method | CLI call | Details |
|---|---|---|
| `source` | env | `<PREFIX>_SOURCE_BLOCK` / `_SOURCE_WINDOW` |
| `read_pane` `Scrollback` | `rex capture -s S -b B --unwrap --trim --format text\|vt` | then keep the last `lines` lines. `S` is the **block's** session (with `topology`, else the host's). |
| `read_pane` `Viewport` | `rex capture -s S -b B --format text\|vt` + `rex block call -s S -b B size` | no `--unwrap`: screen rows are wrapped rows. Keeps the bottom `min(rows, lines)` rows. |
| `list_tab_panes` | `rex session inspect S --json` | the window is the `<PREFIX>_SOURCE_WINDOW` hint, else the window holding the source block, else the active window. Only **tiled** layers and terminal blocks (`creator_name` starts with `com.superlogical.terminal`); source first. Title: label (unless it is the generic `shell`) → terminal title (`block call … title`, fetched only when needed) → label → `pane <id>`. |
| `insert_text` | `rex send -s S -b B` | text on **stdin** |
| `scroll_info` / `caps` | | the defaults: `None`, all `false` |

## Consumer contract

- Treat `scroll_info() == Ok(None)` as normal and degrade gracefully.
- Check `caps()` before offering a host-specific action, rather than
  checking `name()`.
- Under Rex with `--format vt`, expand `CSI n C` into spaces before
  rendering (see [`hosts.md`](hosts.md#quirks)).
- Don't cache pane text: it is a snapshot taken at the time of the call.

## Tests

`herdr.rs`: title order, tab filtering and ordering, scroll parsing,
launch-context parsing. `rex.rs`: the exact CLI argument lists for
capture, viewport, send and errors (with a fake runner), and window
choice and title order against the recorded `session-inspect.json`.
`lib.rs`: the detection order (`detect_kind_from`, the pure version).
`clipboard.rs`: base64 and the OSC 52 sequence.

## Known limits

- herdr: the dev fallback (`HERDR_ACTIVE_PANE_ID`) has no tab, so
  `list_tab_panes` is empty.
- Rex: `list_tab_panes` inspects the host's **own** session, which is
  correct for a source pane but not for listing another session's tab.
- Rex: each `read_pane` is one process spawn, plus a snapshot run when
  the topology cache is cold. Fine for a preview that follows the
  cursor; too slow for many reads in a tight loop.
