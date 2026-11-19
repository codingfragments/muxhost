//! Navigation: moving the user somewhere and shaping the layout
//! (behind the `navigate` feature).
//!
//! `jump` is deliberately an enum with an outcome: herdr executes the
//! focus directly; under Rex the jump is SPLIT — `RexHost::jump`
//! performs the SERVER side itself (`rex focus` on the target block,
//! attaching to its session) and reports [`JumpOutcome::Delegated`],
//! which means "the server state is done; the app's VIEW (selected
//! session, tab strip) is client-side and the calling tool must move
//! it" — via `rex -C <client> do session.select session_id=…
//! window_id=…` (Remote Control). The tool side is responsible for
//! the ordering traps: the select must land AFTER its popup's layer
//! closes (a select made while the layer is open is undone by the
//! app's focus return), and the block focus must be re-asserted
//! AFTER the select (the app may focus the window's remembered
//! block). See herdr-nav's `host_ext::jump` and `doc/rex.md` for the
//! full verified sequence (rex-migration.md R0/R4).

use crate::Host;

/// Where the user should land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Jump {
    /// Focus a pane. herdr's `pane.focus` does the whole switch
    /// (session + window + pane) in one call.
    FocusPane { pane_id: String },
    /// Switch to a session, optionally landing on a specific pane in
    /// it (its active window's focused pane when `None`).
    SelectSession {
        session_id: String,
        focus_pane: Option<String>,
    },
}

/// What [`HostNavigate::jump`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpOutcome {
    /// The focus was executed.
    Executed,
    /// The jump was recorded for the launcher to carry out (Rex: the
    /// app follows `session.select` / `client.tab.*`, which only the
    /// app-invoked action may queue).
    Delegated,
}

/// Focus and layout operations.
pub trait HostNavigate: Host {
    fn jump(&self, jump: Jump) -> Result<JumpOutcome, String>;

    /// Close a pane. May refuse (last pane of a window, etc.) with a
    /// `String` saying why.
    fn close_pane(&self, pane_id: &str) -> Result<(), String>;
    fn close_window(&self, window_id: &str) -> Result<(), String>;
    fn close_session(&self, session_id: &str) -> Result<(), String>;

    fn rename_pane(&self, pane_id: &str, label: &str) -> Result<(), String>;
    fn rename_window(&self, window_id: &str, label: &str) -> Result<(), String>;
}
