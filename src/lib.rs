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

#[cfg(feature = "navigate")]
pub mod navigate;
#[cfg(feature = "topology")]
pub mod topology;

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
    /// The host has a plugin system (`plugin.list` / action invoke).
    pub plugins: bool,
    /// The host can release an agent from its pane (`pane.release_agent`).
    pub agent_release: bool,
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

/// Which multiplexer a tool runs under (see [`detect_kind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostKind {
    Herdr,
    Rex,
}

/// Decide the host from the environment, without building it. The ONE
/// place the detection order lives — tools with their own host
/// construction (herdr-nav's `host_ext::detect`) call this so every
/// consumer agrees.
///
/// `prefix` is the tool's env prefix (e.g. `"ZEXTRACT"`):
/// 1. An explicit `<prefix>_HOST=herdr|rex` wins. Launchers set it, so a
///    herdr running inside a Rex terminal (or the reverse) can't be
///    mistaken for the other host. Any other non-empty value is an
///    error.
/// 2. Otherwise `HERDR_SOCKET_PATH` means herdr,
/// 3. else `REX_SESSION` means Rex.
///
/// Why herdr before Rex: the innermost multiplexer hosts the tool. A
/// herdr started in a Rex terminal has BOTH variables (herdr injects
/// `HERDR_SOCKET_PATH` into its panes; `REX_SESSION` is inherited from
/// the outer Rex terminal). The reverse nesting doesn't leak: the Rex
/// server runs under launchd, so its terminals don't inherit a herdr
/// pane's environment.
pub fn detect_kind(prefix: &str) -> Result<HostKind, String> {
    let host_var = format!("{prefix}_HOST");
    let explicit = std::env::var(&host_var).ok();
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    detect_kind_from(
        explicit.as_deref(),
        set("HERDR_SOCKET_PATH"),
        set("REX_SESSION"),
    )
    .map_err(|e| e.replace("<PREFIX>_HOST", &host_var))
}

/// The pure decision behind [`detect_kind`] (env values passed in, for
/// unit tests).
fn detect_kind_from(
    explicit: Option<&str>,
    herdr_socket: bool,
    rex_session: bool,
) -> Result<HostKind, String> {
    match explicit {
        Some("herdr") => return Ok(HostKind::Herdr),
        Some("rex") => return Ok(HostKind::Rex),
        Some(other) if !other.is_empty() => {
            return Err(format!(
                "<PREFIX>_HOST={other:?} is not a supported host (supported: herdr, rex)"
            ));
        }
        _ => {}
    }
    if herdr_socket {
        return Ok(HostKind::Herdr);
    }
    if rex_session {
        return Ok(HostKind::Rex);
    }
    Err(
        "no multiplexer host detected: neither HERDR_SOCKET_PATH nor REX_SESSION is set \
         (or set <PREFIX>_HOST=herdr|rex)"
            .to_string(),
    )
}

/// Pick the host from the environment and build it ([`detect_kind`]
/// decides which).
pub fn detect(prefix: &str) -> Result<Box<dyn Host>, String> {
    match detect_kind(prefix)? {
        HostKind::Herdr => Ok(Box::new(herdr::HerdrHost::from_env()?)),
        HostKind::Rex => Ok(Box::new(rex::RexHost::from_env(prefix)?)),
    }
}

#[cfg(test)]
mod detect_tests {
    use super::*;

    #[test]
    fn explicit_host_wins() {
        assert_eq!(
            detect_kind_from(Some("rex"), true, false),
            Ok(HostKind::Rex)
        );
        assert_eq!(
            detect_kind_from(Some("herdr"), false, true),
            Ok(HostKind::Herdr)
        );
    }

    #[test]
    fn herdr_inside_rex_is_herdr() {
        // A herdr pane inside a Rex terminal carries both variables;
        // the innermost multiplexer (herdr) hosts the tool.
        assert_eq!(detect_kind_from(None, true, true), Ok(HostKind::Herdr));
    }

    #[test]
    fn falls_back_to_rex_then_errors() {
        assert_eq!(detect_kind_from(None, false, true), Ok(HostKind::Rex));
        assert!(detect_kind_from(None, false, false).is_err());
        // An empty explicit value counts as unset.
        assert_eq!(detect_kind_from(Some(""), false, true), Ok(HostKind::Rex));
    }

    #[test]
    fn unknown_explicit_host_is_an_error() {
        let e = detect_kind_from(Some("tmux"), true, true).unwrap_err();
        assert!(e.contains("tmux"));
    }
}
