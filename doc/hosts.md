# Hosts

How each host talks to its multiplexer, what it reads from the
environment, how to build one, and its quirks. What each trait method
does on each host is covered in the feature pages
([`core.md`](core.md), [`topology.md`](topology.md),
[`navigate.md`](navigate.md)).

---

## HerdrHost (`src/herdr.rs`)

### Transport

herdr serves newline-delimited JSON over a unix socket at
`$HERDR_SOCKET_PATH`. A request is `{"id","method","params"}` and the
reply is `{"result"}` or `{"error"}`.

**herdr closes the connection after every reply.** Reusing it gives
`BrokenPipe`, even a few milliseconds later. So the private
`socket_client::request` **opens a fresh `UnixStream` for every
request**, and every herdr call in the crate goes through it. That
makes it impossible to reintroduce the persistent-connection bug.
Errors come back as `herdr socket error: …` or I/O errors, turned into
`String`.

### Environment

| Variable | Set by | Used for |
|---|---|---|
| `HERDR_SOCKET_PATH` | herdr, in every pane | the socket (required) |
| `HERDR_PLUGIN_CONTEXT_JSON` | herdr, for plugin panes | launch context: `focused_pane_id` (required in it), `tab_id` |
| `HERDR_ACTIVE_PANE_ID` | `[[keys.command]]` popups | dev fallback for the source pane when there is no plugin context (no tab, so tab-wide reads find nothing) |

### Construction

| Constructor | When |
|---|---|
| `HerdrHost::from_env()` | the normal case: needs the socket and a launch context, else `Err` |
| `HerdrHost::with_socket(path)` | a socket without a launch context (empty `SourceRef`). For tools whose context is optional, e.g. herdr-nav's dev path. Everything except the source-pane helpers works. |

### Escape hatch

`raw_request(method, params) -> Result<Value, String>` makes one
request through the same client. herdr-nav uses it for
`workspace.create`, `tab.create`, `pane.split`, `layout.export`,
`pane.process_info`, `plugin.*`, `pane.send_keys`, `agent.*` and
`notification.show`.

### Capabilities

All on: `scroll_follow`, `legacy_keyboard`, `plugins`, `agent_release`.

### Quirks

- `pane.list` never includes plugin popups, so nothing has to be
  filtered out.
- The popup's keyboard runs in legacy mode, so one key tap can arrive
  as two `Press` events. Tools debounce identical keys (about 40 ms).
- `pane.split` splits the **active** pane, not the `pane_id` you pass,
  and `tab.create` doesn't make the new tab active. Template builders
  have to focus the target before each split (tool-side, through the
  escape hatch).

---

## RexHost (`src/rex.rs`)

### Transport

Rex has no socket API for tools. Everything runs through the **`rex`
CLI**, one process per call, behind the `RexRunner` trait:

```rust
pub trait RexRunner {
    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String>;
}
```

`CliRunner { cli }` is the real runner. It spawns the CLI, writes
`stdin` if given, and returns stdout. On a non-zero exit it returns
`rex <subcommand> failed: <stderr, without the "Error: " prefix>`. Tests
plug in a recording fake ([`testing.md`](testing.md)).

**Where the CLI lives:** the Rex server runs under launchd with a
minimal `PATH`, so plain `rex` usually isn't found from a popup.
Launchers pass the absolute path in `REX_CLI`, normally
`~/Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex`.

### Environment

A tool's popup is a Rex **floating layer**. Inside it, `REX_BLOCK` is
the popup's own block, not the pane the user was looking at. So the
**launcher** (an app-invoked Lua action) reads the user's pane before
opening the layer and passes it in:

| Variable | Set by | Used for |
|---|---|---|
| `REX_SESSION` | Rex, in every terminal (launchers pass it on) | the session the tool scopes to. **Must be a session id, not a label**: the topology compares it to `session_id` to mark the active session. |
| `<PREFIX>_SOURCE_BLOCK` | the launcher | the user's pane (required by `from_env`) |
| `<PREFIX>_SOURCE_WINDOW` | the launcher | its window (optional; worked out from the session if not set) |
| `REX_CLI` | the launcher | absolute path of the CLI (default `rex`) |

The launcher gets `REX_CLI` and the login `PATH` from the user's
`~/.config/rex/lib/env.lua`. Setting that up, with a sample, is in
[`rex-environment.md`](rex-environment.md).

### Construction

| Constructor | When |
|---|---|
| `RexHost::from_env(prefix)` | the normal case: `REX_SESSION` and `<PREFIX>_SOURCE_BLOCK` required |
| `RexHost::new(runner, session, source)` | tests, and tools that work without a source pane (herdr-nav builds one with an empty source when run by hand in a Rex terminal) |

### Escape hatch

| Method | |
|---|---|
| `run(args, stdin)` | one raw CLI call on this host's runner. **It drops the cached topology afterwards**, whatever the call was: glue calls like `rex new` or `rex split` change the topology, and the next jump has to see the new blocks. |
| `session()` | the session the host scopes to |
| `block_session(block)` *(topology)* | the session a block is in, from the snapshot, or `None` |

### Capabilities

None of the optional ones: `scroll_follow` (the app keeps the scroll
position on the client side, so the server can't report it),
`legacy_keyboard`, `plugins` (Rex has no plugin system) and
`agent_release` are all `false`.

### Quirks

- **Block calls need the block's own session.** `-s SESSION -b BLOCK`
  with a block from another session gives "block target not found". So
  `read_pane` and `insert_text` (and tool glue such as `pwd`, `split`,
  `send-key`) look up `block_session(block)` and fall back to the
  host's own session. With `topology` on and the cache older than
  250 ms, that lookup costs one snapshot run (about 50 ms).
- **`--format vt` uses `CSI n C` as run-length spaces.** `ansi-to-tui`
  drops the sequence, which shifts columns. Tools expand it into
  spaces before parsing (herdr-flash's `vt_prepare`, also used by nav).
- **`rex send` gets its text on stdin**, so text starting with `-` or
  containing newlines arrives exactly as given.
- **`rex split --ratio`** takes an **integer percent** (the share the
  new block gets). A fraction is a flag parse error.
- **Short block-method names.** In `rex do` scripts and
  `block.method`, block methods use their short names (`title`,
  `pwd`). The creator-prefixed names return nothing.
- **`rex.session.view` needs an attachment** in script context (it
  returns nil without one). `session.attach` returns the view inline.
- **Focus across sessions.** `session.focus_block` from a connection
  attached elsewhere silently does nothing; a child `rex focus -s S B`
  works.
- **The app's view is on the client side.** The server can move focus,
  but can't make the macOS app show a different session. That takes a
  client action (`session.select`) through Remote Control. See
  [`navigate.md`](navigate.md#rex-implementation).
