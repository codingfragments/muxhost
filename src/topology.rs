//! Topology: everything a tree-style switcher needs to know about the
//! multiplexer's layout — sessions, windows, panes and agents — in
//! host-neutral terms (behind the `topology` feature).
//!
//! herdr spellings map as: workspace → session, tab → window,
//! pane → pane. Rex: session/window/block.

use crate::Host;

/// An agent's status as the host reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    /// Waiting on the user (a question, a prompt).
    Waiting,
    /// Busy running.
    Working,
    /// Sitting idle.
    Idle,
    /// Anything else or unreported.
    Unknown,
}

impl AgentStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "waiting" | "blocked" => Self::Waiting,
            "working" => Self::Working,
            "idle" => Self::Idle,
            _ => Self::Unknown,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Working => "working",
            Self::Idle => "idle",
            Self::Unknown => "unknown",
        }
    }
}

/// A session (herdr: workspace; Rex: session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: String,
    pub label: String,
    /// The session the user is in (the source pane's session).
    pub active: bool,
}

/// A window (herdr: tab; Rex: window) with its owning session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub window_id: String,
    pub session_id: String,
    pub label: String,
    /// The window the user is in.
    pub active: bool,
}

/// One pane anywhere in the multiplexer (herdr: pane.list across all
/// workspaces; Rex: every session's tiled blocks). Floating/popup panes
/// are left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneEntry {
    pub pane_id: String,
    pub session_id: String,
    pub window_id: String,
    /// Display title: a user-set label, else the terminal title, else a
    /// generated `pane <id>`.
    pub title: String,
    /// The pane the user is in.
    pub is_focused: bool,
    /// The pane's working directory, where the host reports one.
    pub cwd: Option<String>,
}

/// A pane with an agent running in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEntry {
    pub pane_id: String,
    /// A stable key for the agent's terminal (herdr `terminal_id`).
    pub terminal_id: String,
    /// The agent's name ("pi", "claude", …).
    pub name: String,
    /// Display title (the terminal's stripped title).
    pub title: String,
    pub status: AgentStatus,
}

/// Layout knowledge for tree-style switchers.
pub trait HostTopology: Host {
    /// Every session, in the host's order.
    fn list_sessions(&self) -> Result<Vec<SessionInfo>, String>;

    /// Every window of every session.
    fn list_windows(&self) -> Result<Vec<WindowInfo>, String>;

    /// Every pane of every session, floating panes excluded.
    fn list_all_panes(&self) -> Result<Vec<PaneEntry>, String>;

    /// Every pane running an agent, with its status.
    fn list_agents(&self) -> Result<Vec<AgentEntry>, String>;
}
