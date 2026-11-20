# Architecture

muxhost is the multiplexer layer shared by three terminal tools:
[herdr-zextract](https://github.com/codingfragments/herdr-zextract),
[herdr-flash](https://github.com/codingfragments/herdr-flash) and
[herdr-nav](https://github.com/codingfragments/herdr-nav). They all run
as popups inside a multiplexer, either [herdr](https://herdr.dev) or
[Rex](https://rex.dev). The tools' own logic (matchers, pickers,
trees) uses muxhost's traits and never calls herdr or Rex directly, so
one binary runs under both.

This page covers the shape of the crate. Each feature has its own page:

| Page | Covers |
|---|---|
| [`hosts.md`](hosts.md) | each host: how it talks to the multiplexer, its environment, how it is built, its escape hatch, its quirks |
| [`core.md`](core.md) | the `Host` trait, `Caps`, host detection, the clipboard |
| [`topology.md`](topology.md) | the session/window/pane/agent tree (`topology` feature) |
| [`navigate.md`](navigate.md) | jump, close, rename (`navigate` feature), including what a tool must do after a Rex jump |
| [`rex-environment.md`](rex-environment.md) | what a Rex launcher needs from the user's config (the `rex` CLI path, the login `PATH`), with a copyable `lib/env.lua` sample and troubleshooting |
| [`testing.md`](testing.md) | `FakeHost`, the Rex runner fake, fixtures, the live test, the feature matrix |

## Layers

```
 ┌──────────────────────────────────────────────────────────────┐
 │ tool (zextract · flash · nav)                                │
 │   tool-local glue, through the escape hatches                │
 │   (e.g. herdr-nav src/host_ext/)                             │
 ├──────────────────────────────────────────────────────────────┤
 │ muxhost                                                      │
 │   core:      Host · Caps · detect/detect_kind · clipboard    │
 │   topology:  HostTopology · TopologySnapshot                 │
 │   navigate:  HostNavigate · Jump · JumpOutcome               │
 ├──────────────────────────┬───────────────────────────────────┤
 │ HerdrHost                │ RexHost                           │
 │   JSON socket,           │   `rex` CLI via RexRunner;        │
 │   one connection per     │   topology from one server-side   │
 │   request                │   `rex do` Lua script, cached     │
 └──────────────────────────┴───────────────────────────────────┘
```

What goes where: something **more than one tool needs** belongs in
muxhost. Something **only one tool needs** stays in that tool and
reaches the host through the escape hatches (`HerdrHost::raw_request`,
`RexHost::run`). herdr-nav's template building, capture and plugin
calls are examples.

## Features

| Cargo feature | Adds | Needed by |
|---|---|---|
| *(none, core)* | `Host`, `Caps`, `SourceRef`, `PaneInfo`, `ScrollInfo`, `ReadSource`/`ReadFormat`, `HostKind`, `detect`/`detect_kind`, `clipboard`, `fake` | flash, zextract, nav |
| `topology` | `HostTopology`, `SessionInfo`, `WindowInfo`, `PaneEntry`, `AgentEntry`, `AgentStatus`, `TopologySnapshot` | nav |
| `navigate` (turns on `topology`) | `HostNavigate`, `Jump`, `JumpOutcome` | nav |

Features exist so the core-only consumers don't compile code they don't
use. Every combination a consumer can pin (core, `topology`,
`--all-features`) is built, linted and tested in CI. A gap there once
left CI red from v0.3.0 to v0.3.5 without anyone noticing.

## Names

muxhost uses neutral names and maps them to each host's own terms:

| muxhost | herdr | Rex |
|---|---|---|
| session | workspace | session |
| window | tab | window |
| pane | pane | block |
| popup the tool runs in | plugin pane, `placement = "popup"` | floating layer |

## Design rules

1. **Few dependencies.** `std`, `serde_json`, and `arboard` for the
   clipboard. No async runtime: the tools are short-lived processes
   that run in the foreground.
2. **No tool types in signatures.** IDs are plain `String`s, so the
   crate never depends on a consumer.
3. **Errors are user-readable `String`s.** The crate does not log; a
   tool shows the message (nav puts it in its footer).
4. **Environment variable names are generic.** A tool passes its own
   prefix (`detect("NAV")` reads `NAV_HOST`, `NAV_SOURCE_BLOCK`).
   Variables owned by a multiplexer keep their own names (`HERDR_*`,
   `REX_SESSION`, `REX_CLI`).
5. **Additions never break a consumer.** New trait methods get default
   implementations and public enums are `#[non_exhaustive]`, so a tool
   pinned to an older tag still compiles against a newer muxhost.
6. **Hosts aren't thread-safe.** `RexHost` caches its topology in a
   `RefCell`. The tools are single-threaded event loops, so they don't
   need to share a host across threads.

## Capabilities instead of `cfg(host)`

What only one host can do is reported at runtime through `Caps` (see
[`core.md`](core.md#caps)), not through compile-time switches. One
binary serves both hosts, and the tool decides what to show:
nav hides its Plugins group when `!caps().plugins`.

## Escape hatches

Each host exposes one raw call for tool-local glue:

| Host | Escape hatch | Same contract as normal calls |
|---|---|---|
| herdr | `HerdrHost::raw_request(method, params)` | one fresh socket connection per call |
| Rex | `RexHost::run(args, stdin)` and `RexHost::session()` | same runner, so tests can fake it; drops the topology cache afterwards |

A tool should put its escape-hatch calls behind its **own local
traits** (herdr-nav: `TemplateHost`, `HostExt`), so that porting to a
new host means filling in one trait.

## Versions and releases

Consumers pin a **git tag**, for example
`muxhost = { git = "https://github.com/codingfragments/muxhost", tag = "v0.3.6" }`.
Any API change gets a version bump and a tag. There are no binary
releases; the tag is the release. History: [`CHANGELOG.md`](../CHANGELOG.md).
