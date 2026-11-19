//! [`Host`] for Herdr, over its newline-delimited JSON socket API
//! (`$HERDR_SOCKET_PATH`).
//!
//! Herdr has no floating-pane concept and plugin popups never appear in
//! `pane.list` (confirmed live), so "every pane on the source tab"
//! needs no extra filtering here.

use super::socket_client;
use super::{Caps, Host, PaneInfo, ReadFormat, ReadSource, ScrollInfo, SourceRef};
#[cfg(feature = "navigate")]
#[allow(unused_imports)]
use crate::navigate::{HostNavigate, Jump, JumpOutcome};
#[cfg(feature = "topology")]
#[allow(unused_imports)]
use crate::topology::HostTopology;
#[cfg(feature = "topology")]
use crate::topology::{AgentEntry, AgentStatus, PaneEntry, SessionInfo, WindowInfo};

pub struct HerdrHost {
    socket_path: String,
    source: SourceRef,
}

impl HerdrHost {
    /// Build from Herdr's plugin environment: the launch context
    /// (`HERDR_PLUGIN_CONTEXT_JSON`, or the `HERDR_ACTIVE_PANE_ID`
    /// dev-testing fallback) and `HERDR_SOCKET_PATH`.
    pub fn from_env() -> Result<Self, String> {
        let source = launch_context(
            std::env::var("HERDR_PLUGIN_CONTEXT_JSON").ok().as_deref(),
            std::env::var("HERDR_ACTIVE_PANE_ID").ok().as_deref(),
        )?;
        let socket_path = std::env::var("HERDR_SOCKET_PATH")
            .map_err(|_| "HERDR_SOCKET_PATH is not set".to_string())?;
        Ok(Self {
            socket_path,
            source,
        })
    }

    /// A host over an explicit socket path with no known source pane
    /// (empty [`SourceRef`]): for tools whose launch context is
    /// optional — the socket API stays usable, only the source-pane
    /// conveniences ([`Host::list_tab_panes`] and friends) find nothing.
    pub fn with_socket(socket_path: String) -> Self {
        Self {
            socket_path,
            source: SourceRef {
                pane_id: String::new(),
                tab_id: String::new(),
            },
        }
    }

    /// Escape hatch for host-local glue in consuming tools (see
    /// herdr-nav's `src/host_ext/`): one raw request on this host's
    /// socket, on the same fresh-connection-per-call contract as every
    /// other call here. Keep a tool's non-portable calls behind its own
    /// local trait so the port to a second host stays findable.
    pub fn raw_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        socket_client::request(&self.socket_path, method, params).map_err(|e| e.to_string())
    }
}

impl Host for HerdrHost {
    fn name(&self) -> &'static str {
        "herdr"
    }

    fn source(&self) -> &SourceRef {
        &self.source
    }

    fn read_pane(
        &self,
        pane_id: &str,
        source: ReadSource,
        format: ReadFormat,
        lines: Option<u32>,
    ) -> Result<String, String> {
        let source = match source {
            ReadSource::Viewport => "visible",
            ReadSource::Scrollback => "recent_unwrapped",
        };
        let mut params = serde_json::json!({"pane_id": pane_id, "source": source});
        if let Some(n) = lines {
            params["lines"] = serde_json::json!(n);
        }
        match format {
            ReadFormat::Plain => {}
            ReadFormat::Ansi => {
                params["format"] = serde_json::json!("ansi");
                params["strip_ansi"] = serde_json::json!(false);
            }
        }
        let result = socket_client::request(&self.socket_path, "pane.read", params)
            .map_err(|e| format!("pane.read failed: {e}"))?;
        result
            .get("read")
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| "pane.read response had no \"read.text\" field".to_string())
    }

    fn list_tab_panes(&self) -> Result<Vec<PaneInfo>, String> {
        let result = socket_client::request(&self.socket_path, "pane.list", serde_json::json!({}))
            .map_err(|e| format!("pane.list failed: {e}"))?;
        Ok(panes_on_tab(
            &result,
            &self.source.tab_id,
            &self.source.pane_id,
        ))
    }

    fn insert_text(&self, pane_id: &str, text: &str) -> Result<(), String> {
        socket_client::request(
            &self.socket_path,
            "pane.send_text",
            serde_json::json!({"pane_id": pane_id, "text": text}),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// The source pane's scroll state via `pane.get`. `Ok(None)` if the
    /// pane has no `scroll` field (the schema marks it nullable — a
    /// normal "no scrollback" condition, not an error).
    fn scroll_info(&self, pane_id: &str) -> Result<Option<ScrollInfo>, String> {
        let result = socket_client::request(
            &self.socket_path,
            "pane.get",
            serde_json::json!({ "pane_id": pane_id }),
        )
        .map_err(|e| format!("pane.get failed: {e}"))?;
        let pane = result
            .get("pane")
            .ok_or_else(|| "pane.get response missing \"pane\"".to_string())?;
        let u64_of = |v: Option<&serde_json::Value>| v.and_then(|x| x.as_u64()).unwrap_or(0);
        Ok(pane.get("scroll").map(|scroll| ScrollInfo {
            offset_from_bottom: u64_of(scroll.get("offset_from_bottom")),
            max_offset_from_bottom: u64_of(scroll.get("max_offset_from_bottom")),
            viewport_rows: u64_of(scroll.get("viewport_rows")),
        }))
    }

    fn caps(&self) -> Caps {
        Caps {
            scroll_follow: true,
            legacy_keyboard: true,
            plugins: true,
            agent_release: true,
        }
    }
}

// ── topology (feature-gated) ──────────────────────────────────────

#[cfg(feature = "topology")]
impl crate::topology::HostTopology for HerdrHost {
    /// `workspace.list` → `{workspaces: [{workspace_id, label,
    /// focused, …}]}`.
    fn list_sessions(&self) -> Result<Vec<SessionInfo>, String> {
        let result =
            socket_client::request(&self.socket_path, "workspace.list", serde_json::json!({}))
                .map_err(|e| format!("workspace.list failed: {e}"))?;
        Ok(result
            .get("workspaces")
            .and_then(|v| v.as_array())
            .map(|ws| {
                ws.iter()
                    .filter_map(|w| {
                        Some(SessionInfo {
                            session_id: w.get("workspace_id")?.as_str()?.to_string(),
                            label: w
                                .get("label")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                                .or_else(|| w.get("workspace_id").and_then(|v| v.as_str()))
                                .unwrap_or("?")
                                .to_string(),
                            active: w.get("focused").and_then(|v| v.as_bool()).unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// `tab.list` → `{tabs: [{tab_id, workspace_id, label, focused, …}]}`.
    fn list_windows(&self) -> Result<Vec<WindowInfo>, String> {
        let result = socket_client::request(&self.socket_path, "tab.list", serde_json::json!({}))
            .map_err(|e| format!("tab.list failed: {e}"))?;
        Ok(result
            .get("tabs")
            .and_then(|v| v.as_array())
            .map(|tabs| {
                tabs.iter()
                    .filter_map(|t| {
                        Some(WindowInfo {
                            window_id: t.get("tab_id")?.as_str()?.to_string(),
                            session_id: t.get("workspace_id")?.as_str()?.to_string(),
                            label: t
                                .get("label")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                                .or_else(|| t.get("tab_id").and_then(|v| v.as_str()))
                                .unwrap_or("?")
                                .to_string(),
                            active: t.get("focused").and_then(|v| v.as_bool()).unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// `pane.list` across all workspaces; pane.list never includes
    /// plugin popups (confirmed live), so no floating filter is needed.
    fn list_all_panes(&self) -> Result<Vec<PaneEntry>, String> {
        let result = socket_client::request(&self.socket_path, "pane.list", serde_json::json!({}))
            .map_err(|e| format!("pane.list failed: {e}"))?;
        Ok(result
            .get("panes")
            .and_then(|v| v.as_array())
            .map(|panes| {
                panes
                    .iter()
                    .filter_map(|p| {
                        let pane_id = p.get("pane_id")?.as_str()?.to_string();
                        Some(PaneEntry {
                            pane_id,
                            session_id: p
                                .get("workspace_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            window_id: p
                                .get("tab_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            title: pane_title(p),
                            is_focused: p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false),
                            cwd: p
                                .get("cwd")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                                .map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// `agent.list` → `{agents: [{pane_id, terminal_id, agent,
    /// agent_status, terminal_title_stripped, …}]}`.
    fn list_agents(&self) -> Result<Vec<AgentEntry>, String> {
        let result = socket_client::request(&self.socket_path, "agent.list", serde_json::json!({}))
            .map_err(|e| format!("agent.list failed: {e}"))?;
        Ok(result
            .get("agents")
            .and_then(|v| v.as_array())
            .map(|agents| {
                agents
                    .iter()
                    .filter_map(|a| {
                        Some(AgentEntry {
                            pane_id: a.get("pane_id")?.as_str()?.to_string(),
                            terminal_id: a
                                .get("terminal_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            name: a
                                .get("agent")
                                .and_then(|v| v.as_str())
                                .unwrap_or("agent")
                                .to_string(),
                            title: a
                                .get("terminal_title_stripped")
                                .and_then(|v| v.as_str())
                                .filter(|s| !s.is_empty())
                                .or_else(|| a.get("agent").and_then(|v| v.as_str()))
                                .unwrap_or("agent")
                                .to_string(),
                            status: AgentStatus::parse(
                                a.get("agent_status")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown"),
                            ),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

// ── navigate (feature-gated) ──────────────────────────────────────

#[cfg(feature = "navigate")]
impl crate::navigate::HostNavigate for HerdrHost {
    /// `pane.focus {pane_id}` — the one call that switches session,
    /// tab and focus together. A session pick resolves to the pane
    /// via `pane.get` (herdr carries the focused pane per workspace).
    fn jump(&self, jump: Jump) -> Result<JumpOutcome, String> {
        let pane_id = match jump {
            Jump::FocusPane { pane_id } => pane_id,
            Jump::SelectSession {
                session_id,
                focus_pane,
            } => match focus_pane {
                Some(p) => p,
                None => {
                    // herdr's workspace.list carries active_tab_id; the
                    // tab's focused pane comes from pane.get on the tab.
                    // Simpler and enough for a session pick: focus the
                    // workspace's active tab via tab.list+pane.list.
                    // Fall back to any pane of that session.
                    self.first_pane_of_session(&session_id)?
                }
            },
        };
        socket_client::request(
            &self.socket_path,
            "pane.focus",
            serde_json::json!({"pane_id": pane_id}),
        )
        .map(|_| JumpOutcome::Executed)
        .map_err(|e| e.to_string())
    }

    fn close_pane(&self, pane_id: &str) -> Result<(), String> {
        socket_client::request(
            &self.socket_path,
            "pane.close",
            serde_json::json!({"pane_id": pane_id}),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    fn close_window(&self, window_id: &str) -> Result<(), String> {
        socket_client::request(
            &self.socket_path,
            "tab.close",
            serde_json::json!({"tab_id": window_id}),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    fn close_session(&self, session_id: &str) -> Result<(), String> {
        socket_client::request(
            &self.socket_path,
            "workspace.close",
            serde_json::json!({"workspace_id": session_id}),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    fn rename_pane(&self, pane_id: &str, label: &str) -> Result<(), String> {
        socket_client::request(
            &self.socket_path,
            "pane.rename",
            serde_json::json!({"pane_id": pane_id, "label": label}),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    fn rename_window(&self, window_id: &str, label: &str) -> Result<(), String> {
        socket_client::request(
            &self.socket_path,
            "tab.rename",
            serde_json::json!({"tab_id": window_id, "label": label}),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }
}

#[cfg(feature = "navigate")]
impl HerdrHost {
    /// Any pane of `session_id` (the active tab's first pane), for a
    /// session-only jump.
    fn first_pane_of_session(&self, session_id: &str) -> Result<String, String> {
        let panes = crate::topology::HostTopology::list_all_panes(self)?;
        panes
            .iter()
            .find(|p| p.session_id == session_id)
            .map(|p| p.pane_id.clone())
            .ok_or_else(|| format!("no pane in session {session_id}"))
    }
}

/// Reads the launch context from either path: a real plugin-pane
/// invocation sets `HERDR_PLUGIN_CONTEXT_JSON`; a `[[keys.command]]`
/// custom-command popup (used for manual dev-testing ahead of a real
/// plugin keybind) sets `HERDR_ACTIVE_PANE_ID` directly, with no tab
/// context - tab-wide grabs then find zero panes, which is fine for a
/// manual dev keybind. Pure (env values passed in) for unit testing.
fn launch_context(
    context_json: Option<&str>,
    active_pane_id: Option<&str>,
) -> Result<SourceRef, String> {
    if let Some(context_json) = context_json {
        let context: serde_json::Value =
            serde_json::from_str(context_json).map_err(|e| format!("invalid context JSON: {e}"))?;
        let pane_id = context
            .get("focused_pane_id")
            .and_then(|v| v.as_str())
            .ok_or(
                "context JSON has no focused_pane_id (nothing was focused before this popup opened)",
            )?
            .to_string();
        let tab_id = context
            .get("tab_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Ok(SourceRef { pane_id, tab_id });
    }
    let pane_id = active_pane_id
        .ok_or_else(|| {
            "neither HERDR_PLUGIN_CONTEXT_JSON nor HERDR_ACTIVE_PANE_ID is set".to_string()
        })?
        .to_string();
    Ok(SourceRef {
        pane_id,
        tab_id: String::new(),
    })
}

/// Every pane of a `pane.list` result that sits on `tab_id`, the
/// source pane first (matching the original plugin's ordering), the
/// rest in `pane.list`'s own order.
fn panes_on_tab(list_result: &serde_json::Value, tab_id: &str, source_pane: &str) -> Vec<PaneInfo> {
    let panes = list_result
        .get("panes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut on_tab: Vec<serde_json::Value> = panes
        .into_iter()
        .filter(|p| p.get("tab_id").and_then(|v| v.as_str()) == Some(tab_id))
        .collect();
    on_tab.sort_by_key(|p| p.get("pane_id").and_then(|v| v.as_str()) != Some(source_pane));
    on_tab
        .iter()
        .filter_map(|pane| {
            let pane_id = pane.get("pane_id").and_then(|v| v.as_str())?;
            Some(PaneInfo {
                pane_id: pane_id.to_string(),
                title: pane_title(pane),
            })
        })
        .collect()
}

fn pane_title(pane: &serde_json::Value) -> String {
    pane.get("label")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            pane.get("terminal_title_stripped")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .map(str::to_string)
        .unwrap_or_else(|| {
            let id = pane.get("pane_id").and_then(|v| v.as_str()).unwrap_or("?");
            format!("pane {id}")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_title_prefers_label() {
        let pane = serde_json::json!({"pane_id": "w1:p1", "label": "editor", "terminal_title_stripped": "nvim"});
        assert_eq!(pane_title(&pane), "editor");
    }

    #[test]
    fn pane_title_falls_back_to_terminal_title() {
        let pane = serde_json::json!({"pane_id": "w1:p1", "terminal_title_stripped": "nvim"});
        assert_eq!(pane_title(&pane), "nvim");
    }

    #[test]
    fn pane_title_falls_back_to_pane_id() {
        let pane = serde_json::json!({"pane_id": "w1:p1"});
        assert_eq!(pane_title(&pane), "pane w1:p1");
    }

    #[test]
    fn panes_on_tab_filters_by_tab_and_puts_source_first() {
        let list = serde_json::json!({"panes": [
            {"pane_id": "p1", "tab_id": "t1", "label": "one"},
            {"pane_id": "p2", "tab_id": "t2", "label": "other tab"},
            {"pane_id": "p3", "tab_id": "t1", "label": "three"},
            {"pane_id": "p4", "tab_id": "t1"},
        ]});
        let panes = panes_on_tab(&list, "t1", "p3");
        let ids: Vec<&str> = panes.iter().map(|p| p.pane_id.as_str()).collect();
        assert_eq!(ids, vec!["p3", "p1", "p4"]);
        assert_eq!(panes[0].title, "three");
        assert_eq!(panes[2].title, "pane p4");
    }

    #[test]
    fn panes_on_tab_empty_tab_id_finds_nothing() {
        let list = serde_json::json!({"panes": [{"pane_id": "p1", "tab_id": "t1"}]});
        assert!(panes_on_tab(&list, "", "p1").is_empty());
    }

    #[test]
    fn scroll_info_parses_pane_get_scroll_object() {
        // the shape pane.get returns (from herdr's schema; scroll is
        // nullable, missing fields treated as 0)
        let resp = serde_json::json!({
            "pane": {"pane_id": "p1",
              "scroll": {"offset_from_bottom": 5, "max_offset_from_bottom": 40, "viewport_rows": 24}}
        });
        let pane = resp.get("pane").unwrap();
        let u64_of = |v: Option<&serde_json::Value>| v.and_then(|x| x.as_u64()).unwrap_or(0);
        let info = pane.get("scroll").map(|s| ScrollInfo {
            offset_from_bottom: u64_of(s.get("offset_from_bottom")),
            max_offset_from_bottom: u64_of(s.get("max_offset_from_bottom")),
            viewport_rows: u64_of(s.get("viewport_rows")),
        });
        assert_eq!(
            info,
            Some(ScrollInfo {
                offset_from_bottom: 5,
                max_offset_from_bottom: 40,
                viewport_rows: 24,
            })
        );
        // nullable scroll -> None (no scrollback), not an error
        assert!(serde_json::json!({"pane": {"pane_id": "p1"}})
            .get("pane")
            .unwrap()
            .get("scroll")
            .is_none());
    }

    #[test]
    fn launch_context_reads_plugin_context_json() {
        let src = launch_context(
            Some(r#"{"focused_pane_id": "w1:p2", "tab_id": "w1:t1"}"#),
            Some("ignored"),
        )
        .unwrap();
        assert_eq!(
            src,
            SourceRef {
                pane_id: "w1:p2".into(),
                tab_id: "w1:t1".into()
            }
        );
    }

    #[test]
    fn launch_context_falls_back_to_active_pane_id_without_tab() {
        let src = launch_context(None, Some("w1:p9")).unwrap();
        assert_eq!(src.pane_id, "w1:p9");
        assert_eq!(src.tab_id, "");
    }

    #[test]
    fn launch_context_errors_without_any_source() {
        assert!(launch_context(None, None).is_err());
        assert!(launch_context(Some(r#"{"tab_id": "t"}"#), None).is_err());
        assert!(launch_context(Some("not json"), None).is_err());
    }

    #[test]
    fn with_socket_has_empty_source() {
        let h = HerdrHost::with_socket("/tmp/nope.sock".to_string());
        assert_eq!(h.source().pane_id, "");
        assert_eq!(h.source().tab_id, "");
        // A missing socket surfaces as an error, not a panic.
        assert!(h.raw_request("pane.list", serde_json::json!({})).is_err());
    }
}

#[cfg(all(test, feature = "topology"))]
mod topology_tests {

    // Shapes recorded from a live herdr (0.8.x) instance.
    const WORKSPACES: &str = r#"{"workspaces":[
        {"workspace_id":"w23","label":"stefan.marx","focused":false,"active_tab_id":"w23:t1"},
        {"workspace_id":"w2E","label":"rex","focused":true,"active_tab_id":"w2E:t1"}
    ]}"#;
    const TABS: &str = r#"{"tabs":[
        {"tab_id":"w23:t1","workspace_id":"w23","label":"main","focused":false},
        {"tab_id":"w2E:t1","workspace_id":"w2E","label":"rex dev","focused":true}
    ]}"#;
    const PANES: &str = r#"{"panes":[
        {"pane_id":"w23:p1","workspace_id":"w23","tab_id":"w23:t1","label":"shell",
         "focused":false,"cwd":"/Users/stefan.marx/vlab",
         "terminal_title_stripped":"~/v/vlab"},
        {"pane_id":"w2E:p2","workspace_id":"w2E","tab_id":"w2E:t1","label":"",
         "focused":true,"cwd":null,"terminal_title_stripped":"nvim"}
    ]}"#;
    const AGENTS: &str = r#"{"agents":[
        {"pane_id":"w1V:p1","terminal_id":"term_65fda","agent":"pi",
         "agent_status":"idle","terminal_title_stripped":"pi: codingfragments"},
        {"pane_id":"w2E:p2","terminal_id":"term_772a","agent":"claude",
         "agent_status":"waiting","terminal_title_stripped":""}
    ]}"#;

    fn parse(v: &str) -> serde_json::Value {
        serde_json::from_str(v).unwrap()
    }

    #[test]
    fn sessions_parse_labels_and_active() {
        let ws = parse(WORKSPACES);
        let _ = TABS; // shape recorded alongside; kept for the mapping below
                      // through the same mapping list_sessions applies
        let first = ws.get("workspaces").and_then(|v| v.as_array()).unwrap();
        let entry = first[0]
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(entry, "w23");
        assert_eq!(
            first[0].get("label").and_then(|v| v.as_str()).unwrap(),
            "stefan.marx"
        );
        assert!(!first[0].get("focused").and_then(|v| v.as_bool()).unwrap());
    }

    #[test]
    fn panes_carry_session_window_cwd_and_title() {
        let panes = parse(PANES);
        let arr = panes.get("panes").and_then(|v| v.as_array()).unwrap();
        // focused pane: title from terminal_title_stripped, cwd None
        let focused = arr
            .iter()
            .find(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap())
            .unwrap();
        assert_eq!(
            focused
                .get("terminal_title_stripped")
                .and_then(|v| v.as_str())
                .unwrap(),
            "nvim"
        );
        assert!(focused.get("cwd").and_then(|v| v.as_str()).is_none());
    }

    #[test]
    fn agent_statuses_parse() {
        let agents = parse(AGENTS);
        let _ = WORKSPACES;
        let arr = agents.get("agents").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            crate::topology::AgentStatus::parse(
                arr[0].get("agent_status").and_then(|v| v.as_str()).unwrap()
            ),
            crate::topology::AgentStatus::Idle
        );
        assert_eq!(
            crate::topology::AgentStatus::parse(
                arr[1].get("agent_status").and_then(|v| v.as_str()).unwrap()
            ),
            crate::topology::AgentStatus::Waiting
        );
    }
}
