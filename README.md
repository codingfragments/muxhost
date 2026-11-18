# muxhost

The shared multiplexer host layer for the codingfragments terminal tools
([herdr-zextract](https://github.com/codingfragments/herdr-zextract),
[herdr-flash](https://github.com/codingfragments/herdr-flash),
[herdr-nav](https://github.com/codingfragments/herdr-nav)): one `Host`
trait over [herdr](https://herdr.dev) and
[Rex](https://rex.dev), so a tool's own code stays host-agnostic.

```rust
use muxhost::{detect, Host, ReadFormat, ReadSource};

let host: Box<dyn Host> = detect("ZEXTRACT")?;      // env-based host pick
let text = host.read_pane(                          // plain scrollback read
    &host.source().pane_id, ReadSource::Scrollback, ReadFormat::Plain, Some(150),
)?;
host.insert_text(&host.source().pane_id, "typed")?; // as if the user typed it
if let Some(scroll) = host.scroll_info(&host.source().pane_id)? { /* herdr only */ }
```

## The trait

| Method | Meaning | herdr | Rex |
|---|---|---|---|
| `name` / `source` | which host, and the pane the tool launched from | — | — |
| `read_pane` | pane text, `ReadSource` (viewport/scrollback) × `ReadFormat` (plain/ansi) × optional line cap | `pane.read` | `rex capture` |
| `list_tab_panes` | the source tab's panes, source first, floating popups excluded | `pane.list` | `rex session inspect` |
| `insert_text` | type text into a pane | `pane.send_text` | `rex send` |
| `scroll_info` *(optional)* | the source pane's viewport scroll offset | `pane.get` | not available → `None` |
| `caps` *(optional)* | what the host can do (scroll follow, legacy keyboard quirks) | — | — |

`clipboard::copy` copies to the system clipboard with an OSC 52
fallback for headless/remote hosts. `fake::FakeHost` is an in-memory
host for unit tests.

## Environment

`detect(prefix)` reads `<PREFIX>_HOST` (explicit; launchers set it),
else `HERDR_SOCKET_PATH` → herdr, else `REX_SESSION` → Rex. Rex hosts
also take `REX_CLI` (absolute path of the `rex` CLI), `REX_SESSION`,
and the tool's `<PREFIX>_SOURCE_BLOCK` / `<PREFIX>_SOURCE_WINDOW`.

## Consumers

- herdr-zextract since v0.7.1 (match extraction picker)
- herdr-flash since v0.5.0 (scrollback view, jump selection)
- herdr-nav since v0.7.0 (target switcher)

Dependencies pin this crate by git tag:

```toml
muxhost = { git = "https://github.com/codingfragments/muxhost", tag = "v0.1.0" }
```

## Development

```sh
cargo test          # 22 unit tests (host parsing, args, fixtures)
cargo clippy --all-targets -- -D warnings
```

MIT.
