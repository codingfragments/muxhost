# muxhost: agent conventions (Claude Code, pi, …)

`muxhost` is the shared multiplexer host layer (one `Host` trait over
herdr and Rex) used by herdr-zextract, herdr-flash and herdr-nav.
Design rules: only `std`/`serde_json`/`arboard`; `String` errors, no
logging; generic env names (`detect(prefix)`); `#[non_exhaustive]`
enums and default-method capabilities so additions don't break
consumers. **Any API change needs a version bump + tag** — consumers
pin by git tag; keep `main` releasable at all times.

## Gitflow

- Always create a branch before any change, even one-line fixes.
- Branch prefixes: `bug/`, `feature/`, `phase/`, `docs/`,
  `release/<version>`. If unclear, ask.
- Every branch lands via a squash-merged PR to `main`; no direct
  commits to `main`.
- Commit freely on the branch; ask before every push, PR or merge.
- End each phase with: what to test, how to trigger it, works vs stub.

## Checks

- `just check` before every commit. It runs fmt, then clippy
  `-D warnings` and the tests for **every feature combination** (core,
  `topology`, `--all-features`), the same matrix as CI. Feature-gated
  tests go in gated modules.
- The Rex side is tested against recorded fixtures in
  `tests/fixtures/rex/` (`session-inspect.json`, `topology-do.json`);
  re-record them when the format changes (`doc/testing.md`).
- **The docs are part of the API.** Any change to a trait, a host's
  behaviour, an env var or the cache updates the matching `doc/*.md`
  page and `CHANGELOG.md` in the same PR. Each feature page keeps the
  same sections: Intent, API, herdr, Rex, Consumer contract, Tests,
  Known limits.
- Consumers each have their own repos and tests; a breaking trait
  change here means three repos to fix — prefer additive
  (default-method, `#[non_exhaustive]`) changes.

## Release

1. Code and docs PRs merge first.
2. The release PR bumps `Cargo.toml`, finishes the `CHANGELOG.md`
   entry, and updates the README pin examples and consumer table.
3. After merging, tag: `git tag -a v<x.y.z> -m "…" && git push origin v<x.y.z>`.
   Always `-a -m`: with `tag.gpgSign=true`, a plain `git tag` opens an
   editor and hangs a non-interactive shell. There's no binary release;
   the tag is what consumers pin.
