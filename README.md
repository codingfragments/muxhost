# muxhost

The multiplexer layer shared by the codingfragments terminal tools
([herdr-zextract](https://github.com/codingfragments/herdr-zextract),
[herdr-flash](https://github.com/codingfragments/herdr-flash),
[herdr-nav](https://github.com/codingfragments/herdr-nav)). One set of
traits covers both [herdr](https://herdr.dev) and [Rex](https://rex.dev),
so the tools' own code doesn't depend on which one it runs under.

```rust
use muxhost::{detect, Host, ReadFormat, ReadSource};

let host: Box<dyn Host> = detect("ZEXTRACT")?;      // picks the host from the env
let me = host.source().pane_id.clone();             // the pane the tool opened over
let text = host.read_pane(&me, ReadSource::Scrollback, ReadFormat::Plain, Some(150))?;
host.insert_text(&me, "typed")?;                    // as if the user typed it
if host.caps().scroll_follow { /* herdr only */ }
```

## Features

| Feature | What it adds | herdr | Rex | Docs |
|---|---|---|---|---|
| core | `Host`: read a pane, list the tab, insert text, scroll info; `Caps`; `detect`/`detect_kind`; `clipboard` (with OSC 52 fallback); `fake::FakeHost` | JSON socket, one connection per request | the `rex` CLI | [core](doc/core.md) |
| `topology` | `HostTopology`: every session, window, pane and agent, with active/focused markers; `topology_snapshot()` | 4 socket calls | **one** server-side `rex do` Lua script (~50 ms), cached | [topology](doc/topology.md) |
| `navigate` | `HostNavigate`: `jump` (`Executed` / `Delegated`), close, rename | `pane.focus` does it all | the server-side focus; **the tool moves the app's view** | [navigate](doc/navigate.md) |

Also: [architecture and design rules](doc/architecture.md) · [each
host's transport, environment and quirks](doc/hosts.md) ·
**[setting up the Rex environment for launchers](doc/rex-environment.md)**
(a sample `lib/env.lua` is in [`examples/rex/`](examples/rex/lib/env.lua)) ·
[testing](doc/testing.md) · [changelog](CHANGELOG.md).

## Pinning

Consumers pin a git tag, plus the features they need:

```toml
muxhost = { git = "https://github.com/codingfragments/muxhost", tag = "v0.4.1" }                                       # flash, zextract
muxhost = { git = "https://github.com/codingfragments/muxhost", tag = "v0.4.1", features = ["topology", "navigate"] }  # nav
```

| Consumer | Features | Since |
|---|---|---|
| herdr-zextract | core | v0.7.1 |
| herdr-flash | core | v0.5.0 |
| herdr-nav | `topology`, `navigate` | v0.7.0 (unreleased; the Rex migration) |

## Development

```sh
just check   # fmt + clippy -D warnings + tests, for core / topology / all-features
```

MIT.
