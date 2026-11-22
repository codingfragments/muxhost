# Changelog

Consumers pin by git tag; every entry below is a tag.

## Unreleased
- **Fixed:** Rex `program_status` records are parsed from the `state`
  field — the field Rex actually reports (verified live, rex
  0.1.0-preview.22; before, every agent fell through to `Unknown`
  because the spike looked for a `status` field, which no record
  carries). The spike-era `status` spelling stays as a fallback.
- **New:** `AgentStatus::Done` and `AgentStatus::Error` — Rex OSC 7501
  has finished (`done`) and failed (`error`) states and they survive a
  run's end; `parse`/`as_str` map `"done"` and `"error"`. Agent names
  come from the record's `app` (pi, claude-code, …) instead of the
  default `agent`.
- **Docs:** `doc/rex-environment.md` (what a Rex launcher needs from the
  user's config: the `rex` CLI path and the login `PATH`, how to set it
  up, how to check it, troubleshooting) and a copyable sample,
  `examples/rex/lib/env.lua`. No code change.

## v0.3.6
- **New:** `HostKind` and `detect_kind(prefix)`, the one place the
  detection order is defined (`<PREFIX>_HOST`, then
  `HERDR_SOCKET_PATH`, then `REX_SESSION`: the innermost multiplexer
  first). `detect()` uses it. herdr-nav used its own order (Rex
  first) until now.
- **Fix (Rex):** `WindowInfo.active` was always `false`. It now marks
  the user's window: the one holding the source block, else the
  session's `active_window_id`.
- **Fix:** CI only built without features, and that build didn't
  compile, so CI was red from v0.3.0 to v0.3.5. CI and `just check`
  now cover core, `topology` and `--all-features`.
- **Docs:** `doc/` reference (architecture, hosts, core, topology,
  navigate, testing), this changelog.

## v0.3.5
- Rex: a user-set block label beats the terminal title (Rex's default
  `shell` never does).
- Docs: the `Delegated` contract.

## v0.3.4
- Rex: block calls (`capture`, `send`) use the block's **own** session
  (`block_session`). A pane in another session used to fail with "block
  target not found".

## v0.3.3
- Rex: `run` drops the topology cache, so a session just created by
  tool glue is visible to the next jump.
- `navigate` turns on `topology`.

## v0.3.2
- Rex: `jump` runs the server side itself (`rex focus -s S B`) before
  returning `Delegated`.

## v0.3.1
- Rex: `RexHost::run` and `session()`, the Rex escape hatch.

## v0.3.0
- Rex `topology` (one `rex do` snapshot, cached 250 ms) and `navigate`
  (`Delegated` jumps, CLI closes and renames).
- `HostTopology::topology_snapshot()`.

## v0.2.1
- herdr: `HerdrHost::raw_request` (escape hatch) and `with_socket`
  (a socket without a launch context).

## v0.2.0
- `topology` and `navigate` for herdr, with `FakeHost` support.

## v0.1.0
- `Host` (herdr + Rex), `Caps`, `detect`, `clipboard`, `FakeHost`,
  taken over from herdr-zextract.
