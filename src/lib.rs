//! muxhost — the shared multiplexer host layer for tools that run
//! inside terminal multiplexers (herdr, Rex).
//!
//! One [`Host`] trait, implemented per multiplexer, so a tool's
//! matchers, pickers and actions stay host-agnostic. Consumers so far:
//! [herdr-zextract](https://github.com/codingfragments/herdr-zextract),
//! [herdr-flash](https://github.com/codingfragments/herdr-flash),
//! [herdr-nav](https://github.com/codingfragments/herdr-nav).
//!
//! Design rules (from the zextract port, see its `rex-migration.md`):
//! - Use only `std`, `serde_json` and (for [`clipboard`]) `arboard`.
//!   Nothing from a consuming crate; no tool types in signatures.
//! - Errors are plain `String`s, and the crate does not log.
//! - Env var names are generic: the tool prefix is passed to [`detect`]
//!   (`detect("ZEXTRACT")` reads `ZEXTRACT_HOST`, …); shared names stay
//!   unprefixed (`REX_CLI`, `REX_SESSION`, `HERDR_*`).
//! - Enums are `#[non_exhaustive]` and optional capabilities get default
//!   methods, so additions don't break existing implementations or
//!   downgrades (a tool built against an older muxhost still compiles
//!   against a newer one — new methods have defaults, new enum fields
//!   are non-exhaustive).

pub mod clipboard;
pub mod herdr;
pub mod rex;
mod socket_client;

/// In-memory [`Host`] for unit tests in consuming crates: canned pane
/// texts, a tab listing, and a record of every read and insert.
pub mod fake;

/// Which part of a pane's buffer to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReadSource {
    /// Only what the pane currently shows on screen.
    Viewport,
    /// The scrollback, soft-wrapped lines joined, newest line last.
    Scrollback,
}

/// How a pane's text is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReadFormat {
    /// Plain text, styles stripped.
    Plain,
    /// Raw terminal escape sequences (SGR colors and friends); how the
    /// host spells it is up to the host (herdr `ansi`, Rex `vt`).
    Ansi,
}

/// The pane (and its tab) the tool was launched from: the one to grab
/// from by default and to insert text back into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub pane_id: String,
    /// Empty when the host could not tell (e.g. herdr's dev-testing
    /// fallback): tab-wide operations then find no panes.
    pub tab_id: String,
}

/// One pane of the source tab, as listed by [`Host::list_tab_panes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneInfo {
    pub pane_id: String,
    /// Human-readable title: the pane's label, else its terminal title,
    /// else a generated `pane <id>`.
    pub title: String,
}

/// The source pane's scroll position, where the host can tell. Screen
/// rows, not logical lines: a wrapped line occupies several rows.
/// `None` from hosts that scroll client-side (Rex).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollInfo {
    /// 0 = at the very bottom (showing the newest output); K > 0 = the
    /// viewport's bottom edge is K rows above the scrollback bottom.
    pub offset_from_bottom: u64,
    /// Total scrollback size in screen rows.
    pub max_offset_from_bottom: u64,
    /// How many rows the source pane's viewport shows.
    pub viewport_rows: u64,
}

/// What a host can do beyond the basics; check before relying on an
/// optional capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Caps {
    /// The host reports viewport scroll offsets ([`Host::scroll_info`]).
    pub scroll_follow: bool,
    /// The host's popup panes run the keyboard in legacy mode (key
    /// repeat/deadup quirks; distinct key events need workarounds).
    pub legacy_keyboard: bool,
}

/// The multiplexer operations the tools need.
pub trait Host {
    /// Short host name, e.g. `"herdr"`.
    fn name(&self) -> &'static str;

    /// The pane the tool was launched from.
    fn source(&self) -> &SourceRef;

    /// Read `pane_id`'s text in `format`. `lines` caps the read to the
    /// newest N lines (`None` = everything the source holds).
    fn read_pane(
        &self,
        pane_id: &str,
        source: ReadSource,
        format: ReadFormat,
        lines: Option<u32>,
    ) -> Result<String, String>;

    /// Every pane on the source tab, the source pane first, the rest in
    /// the host's own order. Floating/popup panes (including the tool's
    /// own) are left out.
    fn list_tab_panes(&self) -> Result<Vec<PaneInfo>, String>;

    /// Type `text` into `pane_id` as if the user had typed it.
    fn insert_text(&self, pane_id: &str, text: &str) -> Result<(), String>;

    /// The source pane's scroll position, where the host can tell.
    /// `Ok(None)` = the host has no scroll offset (degrade gracefully).
    fn scroll_info(&self, _pane_id: &str) -> Result<Option<ScrollInfo>, String> {
        Ok(None)
    }

    /// What this host can do.
    fn caps(&self) -> Caps {
        Caps::default()
    }
}

/// Pick the host from the environment.
///
/// `prefix` is the tool's env prefix (e.g. `"ZEXTRACT"`):
/// 1. An explicit `<prefix>_HOST=herdr|rex` wins. Launchers set it, so a
///    herdr running inside a Rex terminal (or the reverse) can't be
///    mistaken for the other host.
/// 2. Otherwise `HERDR_SOCKET_PATH` means herdr,
/// 3. else `REX_SESSION` means Rex.
pub fn detect(prefix: &str) -> Result<Box<dyn Host>, String> {
    let host_var = format!("{prefix}_HOST");
    match std::env::var(&host_var).ok().as_deref() {
        Some("herdr") => return Ok(Box::new(herdr::HerdrHost::from_env()?)),
        Some("rex") => return Ok(Box::new(rex::RexHost::from_env(prefix)?)),
        Some(other) if !other.is_empty() => {
            return Err(format!(
                "{host_var}={other:?} is not a supported host (supported: herdr, rex)"
            ));
        }
        _ => {}
    }
    if std::env::var_os("HERDR_SOCKET_PATH").is_some() {
        return Ok(Box::new(herdr::HerdrHost::from_env()?));
    }
    if std::env::var_os("REX_SESSION").is_some() {
        return Ok(Box::new(rex::RexHost::from_env(prefix)?));
    }
    Err(format!(
        "no multiplexer host detected: neither HERDR_SOCKET_PATH nor REX_SESSION is set \
         (or set {host_var}=herdr|rex)"
    ))
}
