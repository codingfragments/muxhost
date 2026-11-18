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

- `just check` (fmt, clippy `-D warnings`, tests) before every commit.
- The Rex side is exercised against recorded `session inspect`
  fixtures in `tests/fixtures/`; re-record when the shape changes.
- Consumers each have their own repos and tests; a breaking trait
  change here means three repos to fix — prefer additive
  (default-method, `#[non_exhaustive]`) changes.

## Release

1. Code PRs merge first.
2. The release PR bumps `Cargo.toml` and updates the README consumer
   table.
3. After merging, tag (`git tag v<x.y.z> && git push origin v<x.y.z>`) —
   no binary release; the tag is what consumers pin.
