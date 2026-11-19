//! In-memory [`Host`] for unit tests: canned pane texts, a tab listing,
//! and a record of every read and insert.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{Host, PaneInfo, ReadFormat, ReadSource, SourceRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadCall {
    pub pane_id: String,
    pub source: ReadSource,
    pub lines: Option<u32>,
}

pub struct FakeHost {
    pub source: SourceRef,
    /// Text per pane id. A pane listed in `tab` but missing here fails
    /// to read (as a pane closed mid-grab would).
    pub texts: HashMap<String, String>,
    /// What `list_tab_panes` returns, in order.
    pub tab: Vec<PaneInfo>,
    pub reads: RefCell<Vec<ReadCall>>,
    pub inserts: RefCell<Vec<(String, String)>>,
    /// When set, `insert_text` fails with this message.
    pub insert_error: Option<String>,

    // ── topology + navigate state (feature-gated use) ─────────────
    #[cfg(feature = "topology")]
    pub sessions: Vec<crate::topology::SessionInfo>,
    #[cfg(feature = "topology")]
    pub windows: Vec<crate::topology::WindowInfo>,
    #[cfg(feature = "topology")]
    pub all_panes: Vec<crate::topology::PaneEntry>,
    #[cfg(feature = "topology")]
    pub agents: Vec<crate::topology::AgentEntry>,
    #[cfg(feature = "navigate")]
    pub jumps: RefCell<Vec<crate::navigate::Jump>>,
    #[cfg(feature = "navigate")]
    pub jump_fail: Option<String>,
    #[cfg(feature = "navigate")]
    pub closes: RefCell<Vec<String>>,
    #[cfg(feature = "navigate")]
    pub renames: RefCell<Vec<String>>,
}

impl FakeHost {
    pub fn new(source_pane: &str, tab_id: &str) -> Self {
        Self {
            source: SourceRef {
                pane_id: source_pane.to_string(),
                tab_id: tab_id.to_string(),
            },
            texts: HashMap::new(),
            tab: Vec::new(),
            reads: RefCell::new(Vec::new()),
            inserts: RefCell::new(Vec::new()),
            insert_error: None,
            #[cfg(feature = "topology")]
            sessions: Vec::new(),
            #[cfg(feature = "topology")]
            windows: Vec::new(),
            #[cfg(feature = "topology")]
            all_panes: Vec::new(),
            #[cfg(feature = "topology")]
            agents: Vec::new(),
            #[cfg(feature = "navigate")]
            jumps: RefCell::new(Vec::new()),
            #[cfg(feature = "navigate")]
            jump_fail: None,
            #[cfg(feature = "navigate")]
            closes: RefCell::new(Vec::new()),
            #[cfg(feature = "navigate")]
            renames: RefCell::new(Vec::new()),
        }
    }

    /// Add a pane with `text`, also listing it on the tab.
    pub fn with_pane(mut self, pane_id: &str, title: &str, text: &str) -> Self {
        self.texts.insert(pane_id.to_string(), text.to_string());
        self.tab.push(PaneInfo {
            pane_id: pane_id.to_string(),
            title: title.to_string(),
        });
        self
    }
}

impl Host for FakeHost {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn source(&self) -> &SourceRef {
        &self.source
    }

    fn read_pane(
        &self,
        pane_id: &str,
        source: ReadSource,
        _format: ReadFormat,
        lines: Option<u32>,
    ) -> Result<String, String> {
        self.reads.borrow_mut().push(ReadCall {
            pane_id: pane_id.to_string(),
            source,
            lines,
        });
        self.texts
            .get(pane_id)
            .cloned()
            .ok_or_else(|| format!("no such pane: {pane_id}"))
    }

    fn list_tab_panes(&self) -> Result<Vec<PaneInfo>, String> {
        Ok(self.tab.clone())
    }

    fn insert_text(&self, pane_id: &str, text: &str) -> Result<(), String> {
        if let Some(e) = &self.insert_error {
            return Err(e.clone());
        }
        self.inserts
            .borrow_mut()
            .push((pane_id.to_string(), text.to_string()));
        Ok(())
    }
}

// ── topology + navigate (feature-gated mirrors of the real hosts) ──

#[cfg(feature = "topology")]
impl crate::topology::HostTopology for FakeHost {
    fn list_sessions(&self) -> Result<Vec<crate::topology::SessionInfo>, String> {
        Ok(self.sessions.clone())
    }
    fn list_windows(&self) -> Result<Vec<crate::topology::WindowInfo>, String> {
        Ok(self.windows.clone())
    }
    fn list_all_panes(&self) -> Result<Vec<crate::topology::PaneEntry>, String> {
        Ok(self.all_panes.clone())
    }
    fn list_agents(&self) -> Result<Vec<crate::topology::AgentEntry>, String> {
        Ok(self.agents.clone())
    }
}

#[cfg(feature = "topology")]
impl FakeHost {
    /// A fake with canned sessions/windows/panes/agents.
    pub fn with_topology(
        sessions: Vec<crate::topology::SessionInfo>,
        windows: Vec<crate::topology::WindowInfo>,
        all_panes: Vec<crate::topology::PaneEntry>,
        agents: Vec<crate::topology::AgentEntry>,
    ) -> Self {
        let mut f = Self::new("p1", "t1");
        f.sessions = sessions;
        f.windows = windows;
        f.all_panes = all_panes;
        f.agents = agents;
        f
    }
}

#[cfg(feature = "navigate")]
impl crate::navigate::HostNavigate for FakeHost {
    fn jump(&self, jump: crate::navigate::Jump) -> Result<crate::navigate::JumpOutcome, String> {
        use crate::navigate::JumpOutcome;
        self.jumps.borrow_mut().push(jump);
        match self.jump_fail {
            Some(ref e) => Err(e.clone()),
            None => Ok(JumpOutcome::Executed),
        }
    }
    fn close_pane(&self, pane_id: &str) -> Result<(), String> {
        self.closes.borrow_mut().push(format!("pane:{pane_id}"));
        Ok(())
    }
    fn close_window(&self, window_id: &str) -> Result<(), String> {
        self.closes.borrow_mut().push(format!("window:{window_id}"));
        Ok(())
    }
    fn close_session(&self, session_id: &str) -> Result<(), String> {
        self.closes
            .borrow_mut()
            .push(format!("session:{session_id}"));
        Ok(())
    }
    fn rename_pane(&self, pane_id: &str, label: &str) -> Result<(), String> {
        self.renames
            .borrow_mut()
            .push(format!("pane:{pane_id}={label}"));
        Ok(())
    }
    fn rename_window(&self, window_id: &str, label: &str) -> Result<(), String> {
        self.renames
            .borrow_mut()
            .push(format!("window:{window_id}={label}"));
        Ok(())
    }
}
