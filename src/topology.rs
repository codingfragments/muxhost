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

/// The whole topology in one answer: sessions, their windows, their
/// panes, and the agent panes. Built for tree-style switchers that
/// need everything at once — under Rex one script run collects all of
/// it (see `HostTopology::topology_snapshot`), where the four list
/// methods would each pay the round trip.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TopologySnapshot {
    pub sessions: Vec<SessionInfo>,
    pub windows: Vec<WindowInfo>,
    pub panes: Vec<PaneEntry>,
    pub agents: Vec<AgentEntry>,
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

    /// Everything above in one call. The default assembles it from the
    /// four list methods (herdr's four socket calls are cheap); hosts
    /// where a round trip is expensive (Rex: one `rex do` script)
    /// override this and the list methods read from the same snapshot.
    fn topology_snapshot(&self) -> Result<TopologySnapshot, String> {
        Ok(TopologySnapshot {
            sessions: self.list_sessions()?,
            windows: self.list_windows()?,
            panes: self.list_all_panes()?,
            agents: self.list_agents()?,
        })
    }
}
