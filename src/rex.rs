//! [`Host`] for Rex (superlogical's terminal multiplexer), through the
//! `rex` CLI.
//!
//! The tool runs in a floating layer that the Rex launcher
//! (`rex/zextract.lua`) opens. Inside that layer `REX_BLOCK` is the
//! layer's own block, never the pane the user was in, so the launcher
//! passes the source explicitly:
//!
//! | Env var | Meaning |
//! |---|---|
//! | `REX_SESSION` | session to work in (set by Rex in every terminal) |
//! | `<PREFIX>_SOURCE_BLOCK` | the pane to grab from and insert into (required) |
//! | `<PREFIX>_SOURCE_WINDOW` | its window (tab); found from the session if unset |
//! | `REX_CLI` | path of the `rex` CLI (default: `rex` on `PATH`) |

use std::cell::RefCell;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{Host, PaneInfo, ReadFormat, ReadSource, SourceRef};
#[cfg(feature = "navigate")]
use crate::navigate::{HostNavigate, Jump, JumpOutcome};
#[cfg(feature = "topology")]
use crate::topology::{
    AgentEntry, AgentStatus, HostTopology, PaneEntry, SessionInfo, TopologySnapshot, WindowInfo,
};

/// Runs one `rex` CLI invocation and returns its stdout. A trait so
/// tests can check the exact arguments without a Rex server.
pub trait RexRunner {
    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String>;
}

/// The real runner: spawns the `rex` CLI.
pub struct CliRunner {
    pub cli: String,
}

impl RexRunner for CliRunner {
    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
        let mut child = Command::new(&self.cli)
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!(
                    "cannot run the rex CLI ({}): {e} - set REX_CLI to its path",
                    self.cli
                )
            })?;
        if let Some(text) = stdin {
            if let Some(mut pipe) = child.stdin.take() {
                pipe.write_all(text.as_bytes())
                    .map_err(|e| format!("rex {}: writing stdin failed: {e}", args[0]))?;
            }
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("rex {}: {e}", args[0]))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = err.trim().trim_start_matches("Error: ");
            return Err(format!("rex {} failed: {err}", args[0]));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

pub struct RexHost {
    runner: Box<dyn RexRunner>,
    session: String,
    source: SourceRef,
    /// Cached topology snapshot: ONE `rex do` script run collects all
    /// sessions, windows, blocks (with titles, cwd and program status)
    /// — the four list methods read from here instead of each paying a
    /// round trip. Invalidated by this host's own mutations, and by a
    /// short TTL so changes made elsewhere (another window, the app)
    /// show up too.
    #[cfg(feature = "topology")]
    snapshot: RefCell<Option<(Instant, TopologySnapshot)>>,
}

impl RexHost {
    /// Build from the environment the Rex launcher sets (see the module
    /// docs). `prefix` is the tool's env prefix, e.g. `"ZEXTRACT"`.
    pub fn from_env(prefix: &str) -> Result<Self, String> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let session =
            var("REX_SESSION").ok_or("REX_SESSION is not set (not running inside Rex?)")?;
        let block_var = format!("{prefix}_SOURCE_BLOCK");
        let pane_id = var(&block_var).ok_or_else(|| {
            format!(
                "{block_var} is not set: open this through the Rex launcher \
                 (rex/zextract.lua), which passes the pane to grab from"
            )
        })?;
        let tab_id = var(&format!("{prefix}_SOURCE_WINDOW")).unwrap_or_default();
        let cli = var("REX_CLI").unwrap_or_else(|| "rex".to_string());
        Ok(Self::new(
            Box::new(CliRunner { cli }),
            session,
            SourceRef { pane_id, tab_id },
        ))
    }

    pub fn new(runner: Box<dyn RexRunner>, session: String, source: SourceRef) -> Self {
        Self {
            runner,
            session,
            source,
            #[cfg(feature = "topology")]
            snapshot: RefCell::new(None),
        }
    }

    fn rex(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
        self.runner.run(args, stdin)
    }

    /// The terminal's visible row count (`block call … size`).
    fn rows(&self, pane_id: &str) -> Result<usize, String> {
        let out = self.rex(
            &["block", "call", "-s", &self.session, "-b", pane_id, "size"],
            None,
        )?;
        let v: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| format!("rex size: bad JSON: {e}"))?;
        v.get("rows")
            .and_then(|r| r.as_u64())
            .map(|r| r as usize)
            .ok_or_else(|| "rex size: no \"rows\" field".to_string())
    }

    /// The terminal title (`block call … title`), empty if unknown.
    fn title(&self, pane_id: &str) -> String {
        self.rex(
            &["block", "call", "-s", &self.session, "-b", pane_id, "title"],
            None,
        )
        .ok()
        .and_then(|out| serde_json::from_str::<serde_json::Value>(&out).ok())
        .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(str::to_string))
        .unwrap_or_default()
    }
}

impl Host for RexHost {
    fn name(&self) -> &'static str {
        "rex"
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
        let session = self.session.as_str();
        let fmt: Vec<&str> = match format {
            ReadFormat::Plain => vec!["--format", "text"],
            ReadFormat::Ansi => vec!["--format", "vt"],
        };
        match source {
            ReadSource::Scrollback => {
                let text = self.rex(
                    &[
                        "capture", "-s", session, "-b", pane_id, "--unwrap", "--trim", fmt[0],
                        fmt[1],
                    ],
                    None,
                )?;
                Ok(last_lines(&text, lines.map(|n| n as usize)))
            }
            ReadSource::Viewport => {
                // The screen is the bottom `rows` rows of the capture.
                // Screen rows are wrapped rows, so no --unwrap here.
                let text = self.rex(
                    &["capture", "-s", session, "-b", pane_id, fmt[0], fmt[1]],
                    None,
                )?;
                let rows = self.rows(pane_id).ok();
                let cap = match (rows, lines) {
                    (Some(r), Some(n)) => Some(r.min(n as usize)),
                    (Some(r), None) => Some(r),
                    (None, n) => n.map(|n| n as usize),
                };
                Ok(last_lines(&text, cap))
            }
        }
    }

    fn list_tab_panes(&self) -> Result<Vec<PaneInfo>, String> {
        let out = self.rex(&["session", "inspect", &self.session, "--json"], None)?;
        let view: serde_json::Value = serde_json::from_str(&out)
            .map_err(|e| format!("rex session inspect: bad JSON: {e}"))?;
        let panes = tab_blocks(&view, &self.source.tab_id, &self.source.pane_id);
        Ok(panes
            .into_iter()
            .map(|(pane_id, label)| {
                let title = pane_title(&label, || self.title(&pane_id), &pane_id);
                PaneInfo { pane_id, title }
            })
            .collect())
    }

    fn insert_text(&self, pane_id: &str, text: &str) -> Result<(), String> {
        // Through stdin, so text starting with "-" or containing
        // newlines arrives exactly as given.
        self.rex(&["send", "-s", &self.session, "-b", pane_id], Some(text))
            .map(|_| ())
    }
}

// ── topology (feature-gated) ─────────────────────────────────────────────────
//
// One server-side Lua script (`rex do -`, piped on stdin) collects the
// whole topology in a single round trip: session.list, each session's
// view (via session.attach, which returns it inline), and per block
// the terminal title, pwd and program status (block.method). Verified
// live (rex 0.1.0-preview.19, 2026-11): ~50 ms for several sessions —
// the four list methods each re-running it would blow any popup's
// build budget, hence the cached snapshot.

#[cfg(feature = "topology")]
const SNAPSHOT_TTL: Duration = Duration::from_millis(250);

#[cfg(feature = "topology")]
const TOPOLOGY_SCRIPT: &str = r#"
local function block_call(session_id, block_id, method)
  local ok, r = pcall(rex.call, "block.method", {
    session_id = session_id, block_id = block_id, method = method,
  })
  if ok then return r end
  return nil
end

local out = { sessions = {} }
local ok, ls = pcall(rex.call, "session.list", {})
if not ok then return { error = tostring(ls) } end

for _, s in ipairs(ls.sessions or {}) do
  local sid = s.session_id
  -- session.attach returns the current view inline; detach right after.
  local ok_a, attach = pcall(rex.call, "session.attach", { session_id = sid })
  if ok_a then pcall(rex.call, "session.detach", { session_id = sid }) end
  local view = ok_a and attach and attach.view or nil
  if view == nil then
    out.sessions[#out.sessions + 1] =
      { session_id = sid, label = s.label, error = tostring(attach) }
  else
    local entry = {
      session_id = sid,
      label = view.label or s.label,
      active_window_id = view.active_window_id,
      windows = {},
    }
    for _, w in ipairs(view.windows or {}) do
      local window = {
        window_id = w.window_id,
        label = w.label,
        focused_block_id = w.focused_block_id,
        blocks = {},
      }
      for _, layer in ipairs(w.layers or {}) do
        if layer.kind == "tiled" then
          for _, b in ipairs(layer.blocks or {}) do
            window.blocks[#window.blocks + 1] = {
              block_id = b.block_id,
              label = b.label,
              title = block_call(sid, b.block_id, "title"),
              pwd = block_call(sid, b.block_id, "pwd"),
              program_status = block_call(sid, b.block_id, "program_status"),
            }
          end
        end
      end
      entry.windows[#entry.windows + 1] = window
    end
    out.sessions[#out.sessions + 1] = entry
  end
end

return out
"#;

#[cfg(feature = "topology")]
impl RexHost {
    /// Drop the cached snapshot (after a mutation through this host).
    fn invalidate_snapshot(&self) {
        *self.snapshot.borrow_mut() = None;
    }

    /// The topology snapshot, from cache when fresh, else one script run.
    fn snapshot(&self) -> Result<TopologySnapshot, String> {
        if let Some((at, snap)) = self.snapshot.borrow().as_ref() {
            if at.elapsed() < SNAPSHOT_TTL {
                return Ok(snap.clone());
            }
        }
        let script = TOPOLOGY_SCRIPT.replace("__SOURCE__", &self.session);
        let out = self.rex(&["do", "-"], Some(&script))?;
        let v: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| format!("rex do topology: bad JSON: {e}"))?;
        if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
            return Err(format!("rex do topology: {err}"));
        }
        let snap = parse_topology(&v, &self.session, &self.source.pane_id)?;
        *self.snapshot.borrow_mut() = Some((Instant::now(), snap.clone()));
        Ok(snap)
    }

    /// The session a block belongs to, from the (fresh) snapshot.
    fn session_of_block(&self, block_id: &str) -> Result<String, String> {
        self.snapshot()?
            .panes
            .iter()
            .find(|p| p.pane_id == block_id)
            .map(|p| p.session_id.clone())
            .ok_or_else(|| format!("no session holds block {block_id}"))
    }

    /// The session a window belongs to, from the (fresh) snapshot.
    fn session_of_window(&self, window_id: &str) -> Result<String, String> {
        self.snapshot()?
            .windows
            .iter()
            .find(|w| w.window_id == window_id)
            .map(|w| w.session_id.clone())
            .ok_or_else(|| format!("no session holds window {window_id}"))
    }
}

/// Parse the script's JSON into the snapshot. `source_session` marks the
/// active session, `source_block` the pane the user is in (Rex's
/// focused_block_id is per window; the user's own block is the useful
/// "focused" marker for a switcher).
#[cfg(feature = "topology")]
fn parse_topology(
    v: &serde_json::Value,
    source_session: &str,
    source_block: &str,
) -> Result<TopologySnapshot, String> {
    let sessions = v
        .get("sessions")
        .and_then(|s| s.as_array())
        .ok_or_else(|| "rex do topology: no sessions[]".to_string())?;
    let mut snap = TopologySnapshot::default();
    for s in sessions {
        let session_id = str_field(s, "session_id");
        if session_id.is_empty() {
            continue;
        }
        snap.sessions.push(SessionInfo {
            session_id: session_id.clone(),
            label: str_field(s, "label"),
            active: session_id == source_session,
        });
        for w in s
            .get("windows")
            .and_then(|w| w.as_array())
            .into_iter()
            .flatten()
        {
            let window_id = str_field(w, "window_id");
            if window_id.is_empty() {
                continue;
            }
            snap.windows.push(WindowInfo {
                window_id: window_id.clone(),
                session_id: session_id.clone(),
                label: str_field(w, "label"),
                // The script doesn't carry a per-window active flag;
                // the block list below gets focus from the source block.
                active: false,
            });
            for b in w
                .get("blocks")
                .and_then(|b| b.as_array())
                .into_iter()
                .flatten()
            {
                let block_id = str_field(b, "block_id");
                if block_id.is_empty() {
                    continue;
                }
                let title = block_title(b);
                let cwd = block_pwd(b);
                let pane = PaneEntry {
                    pane_id: block_id.clone(),
                    session_id: session_id.clone(),
                    window_id: window_id.clone(),
                    title: title.clone().unwrap_or_else(|| str_field(b, "label")),
                    is_focused: !source_block.is_empty() && block_id == source_block,
                    cwd,
                };
                if let Some(agent) = block_agent(b, &pane, &title) {
                    snap.agents.push(agent);
                }
                snap.panes.push(pane);
            }
        }
    }
    Ok(snap)
}

/// A block's terminal title (`block_call … title` → {title: "..."}).
#[cfg(feature = "topology")]
fn block_title(b: &serde_json::Value) -> Option<String> {
    let t = b
        .get("title")
        .and_then(|t| t.get("title"))
        .and_then(|t| t.as_str())?;
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// A block's working directory (`block_call … pwd` → {pwd: "file://…"}).
/// Rex reports a file URL ("file://hostname/path"); keep the path.
#[cfg(feature = "topology")]
fn block_pwd(b: &serde_json::Value) -> Option<String> {
    let p = b
        .get("pwd")
        .and_then(|p| p.get("pwd"))
        .and_then(|p| p.as_str())?;
    let path = if let Some(rest) = p.strip_prefix("file://") {
        // file://authority/path — the path starts at the first '/'.
        match rest.find('/') {
            Some(i) => &rest[i..],
            None => "",
        }
    } else {
        p
    };
    let path = path.trim();
    (!path.is_empty()).then(|| path.to_string())
}

/// A block's agent entry when its program status reports records
/// (`block_call … program_status` → {records: [...]}; gopher-lua also
/// serializes an empty records table as {} — both empty shapes mean
/// "no agent"). The record's `status` maps blocked→waiting (an agent
/// blocked on the user), working→working, idle→idle.
#[cfg(feature = "topology")]
fn block_agent(
    b: &serde_json::Value,
    pane: &PaneEntry,
    title: &Option<String>,
) -> Option<AgentEntry> {
    let records = b
        .get("program_status")
        .and_then(|p| p.get("records"))
        .and_then(|r| r.as_array().cloned())
        .or_else(|| {
            b.get("program_status")
                .and_then(|p| p.get("records"))
                .and_then(|r| r.as_object())
                .map(|m| m.values().cloned().collect::<Vec<_>>())
        })?;
    let first = records.first()?;
    let status = match first.get("status").and_then(|s| s.as_str()) {
        Some("working") => AgentStatus::Working,
        Some("blocked") => AgentStatus::Waiting,
        Some("idle") => AgentStatus::Idle,
        _ => AgentStatus::Unknown,
    };
    Some(AgentEntry {
        pane_id: pane.pane_id.clone(),
        terminal_id: pane.pane_id.clone(),
        name: first
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("agent")
            .to_string(),
        title: title.clone().unwrap_or_else(|| pane.title.clone()),
        status,
    })
}

#[cfg(feature = "topology")]
fn str_field(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|s| s.as_str()).unwrap_or("").to_string()
}

#[cfg(feature = "topology")]
impl HostTopology for RexHost {
    fn list_sessions(&self) -> Result<Vec<SessionInfo>, String> {
        Ok(self.snapshot()?.sessions)
    }

    fn list_windows(&self) -> Result<Vec<WindowInfo>, String> {
        Ok(self.snapshot()?.windows)
    }

    fn list_all_panes(&self) -> Result<Vec<PaneEntry>, String> {
        Ok(self.snapshot()?.panes)
    }

    fn list_agents(&self) -> Result<Vec<AgentEntry>, String> {
        Ok(self.snapshot()?.agents)
    }

    fn topology_snapshot(&self) -> Result<TopologySnapshot, String> {
        self.snapshot()
    }
}

// ── navigate (feature-gated) ─────────────────────────────────────────────────
//
// The jump is DELEGATED: the app's view (selected session, tab strip)
// is client-side, and only an app-invoked action may queue the client
// actions (session.select / client.tab.*) — the launcher does that,
// fed by the jump record the tool wrote (see herdr-nav's rex-migration
// R0 spike). Closes and renames are plain CLI calls; the session of a
// block/window id comes from the snapshot.

#[cfg(feature = "navigate")]
impl HostNavigate for RexHost {
    fn jump(&self, _jump: Jump) -> Result<JumpOutcome, String> {
        Ok(JumpOutcome::Delegated)
    }

    fn close_pane(&self, pane_id: &str) -> Result<(), String> {
        let session = self.session_of_block(pane_id)?;
        let r = self
            .rex(&["block", "close", "-s", &session, pane_id], None)
            .map(|_| ());
        self.invalidate_snapshot();
        r
    }

    fn close_window(&self, window_id: &str) -> Result<(), String> {
        let session = self.session_of_window(window_id)?;
        let r = self
            .rex(&["window", "close", "-s", &session, window_id], None)
            .map(|_| ());
        self.invalidate_snapshot();
        r
    }

    fn close_session(&self, session_id: &str) -> Result<(), String> {
        let r = self.rex(&["session", "kill", session_id], None).map(|_| ());
        self.invalidate_snapshot();
        r
    }

    fn rename_pane(&self, pane_id: &str, label: &str) -> Result<(), String> {
        let session = self.session_of_block(pane_id)?;
        let args = serde_json::json!({"block_id": pane_id, "label": label}).to_string();
        let r = self
            .rex(
                &[
                    "api",
                    "call",
                    "-s",
                    &session,
                    "session.set_block_label",
                    &args,
                ],
                None,
            )
            .map(|_| ());
        self.invalidate_snapshot();
        r
    }

    fn rename_window(&self, window_id: &str, label: &str) -> Result<(), String> {
        let session = self.session_of_window(window_id)?;
        let r = self
            .rex(
                &["window", "rename", "-s", &session, window_id, label],
                None,
            )
            .map(|_| ());
        self.invalidate_snapshot();
        r
    }
}

/// Keep the last `n` lines of `text` (all of it for `None`).
fn last_lines(text: &str, n: Option<usize>) -> String {
    let Some(n) = n else {
        return text.to_string();
    };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    lines[lines.len().saturating_sub(n)..].concat()
}

/// The terminal blocks of the source window's tiled layers, as
/// `(block_id, label)`, the source block first and the rest in Rex's
/// order. Floating layers (the tool's own popup among them) are left
/// out. The window is `window_hint` if given and present, else the
/// window holding `source_block`, else the session's active window.
fn tab_blocks(
    view: &serde_json::Value,
    window_hint: &str,
    source_block: &str,
) -> Vec<(String, String)> {
    let windows = view
        .get("windows")
        .and_then(|w| w.as_array())
        .cloned()
        .unwrap_or_default();
    let str_of = |v: &serde_json::Value, k: &str| {
        v.get(k)
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let blocks_of = |w: &serde_json::Value, tiled_only: bool| -> Vec<serde_json::Value> {
        w.get("layers")
            .and_then(|l| l.as_array())
            .into_iter()
            .flatten()
            .filter(|layer| !tiled_only || str_of(layer, "kind") == "tiled")
            .flat_map(|layer| {
                layer
                    .get("blocks")
                    .and_then(|b| b.as_array())
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    };
    let window = windows
        .iter()
        .find(|w| !window_hint.is_empty() && str_of(w, "window_id") == window_hint)
        .or_else(|| {
            windows.iter().find(|w| {
                blocks_of(w, false)
                    .iter()
                    .any(|b| str_of(b, "block_id") == source_block)
            })
        })
        .or_else(|| {
            let active = str_of(view, "active_window_id");
            windows.iter().find(|w| str_of(w, "window_id") == active)
        });
    let Some(window) = window else {
        return Vec::new();
    };
    let mut blocks: Vec<(String, String)> = blocks_of(window, true)
        .iter()
        .filter(|b| str_of(b, "creator_name").starts_with("com.superlogical.terminal"))
        .map(|b| (str_of(b, "block_id"), str_of(b, "label")))
        .filter(|(id, _)| !id.is_empty())
        .collect();
    blocks.sort_by_key(|(id, _)| id != source_block);
    blocks
}

/// A pane's title: a label the user set, else the terminal title, else
/// the generic label, else `pane <id>`. Rex labels every new shell
/// block "shell", which says nothing; the terminal title ("~/src",
/// "nvim …") is more useful then. `title` is only called when needed.
fn pane_title(label: &str, title: impl FnOnce() -> String, pane_id: &str) -> String {
    if !label.is_empty() && label != "shell" {
        return label.to_string();
    }
    let t = title();
    if !t.is_empty() {
        return t;
    }
    if !label.is_empty() {
        return label.to_string();
    }
    format!("pane {pane_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    const S: &str = "session:01a1ec33-7bf2-7de6-939b-0b2e3c830260";
    const W1: &str = "window:01a1ec33-7c27-7910-a246-dd905785be2b";
    const W2: &str = "window:01a1ec33-7cab-7e30-a0e4-17635520f676";
    const B1: &str = "block:01a1ec33-7bf2-7ee8-9dd0-5e4037155b99";
    const B2: &str = "block:01a1ec33-7c70-772f-8d2e-9ba203f9e761";
    const B3: &str = "block:01a1ec33-7c9d-72d6-a30f-3ad1663414ca";
    const POPUP: &str = "block:01a1ec33-9897-7111-b5bd-4fa7659f4c90";

    fn fixture() -> serde_json::Value {
        // Recorded from a live Rex session (rex 0.1.0-preview.20): two
        // windows; W1 has two tiled shells and a floating "popup".
        serde_json::from_str(include_str!("../tests/fixtures/rex/session-inspect.json")).unwrap()
    }

    type Call = (Vec<String>, Option<String>);

    /// Records every call and answers from a queue of canned replies.
    #[derive(Clone, Default)]
    struct FakeRunner {
        calls: Rc<RefCell<Vec<Call>>>,
        replies: Rc<RefCell<VecDeque<Result<String, String>>>>,
    }

    impl FakeRunner {
        fn reply(self, r: Result<&str, &str>) -> Self {
            self.replies
                .borrow_mut()
                .push_back(r.map(str::to_string).map_err(str::to_string));
            self
        }
    }

    impl RexRunner for FakeRunner {
        fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
            self.calls.borrow_mut().push((
                args.iter().map(|a| a.to_string()).collect(),
                stdin.map(str::to_string),
            ));
            self.replies
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))
        }
    }

    fn host(runner: &FakeRunner, window: &str) -> RexHost {
        RexHost::new(
            Box::new(runner.clone()),
            S.to_string(),
            SourceRef {
                pane_id: B2.to_string(),
                tab_id: window.to_string(),
            },
        )
    }

    fn args(call: &Call) -> Vec<&str> {
        call.0.iter().map(String::as_str).collect()
    }

    #[test]
    fn tab_blocks_lists_tiled_terminals_of_the_window_source_first() {
        let got = tab_blocks(&fixture(), W1, B2);
        let ids: Vec<&str> = got.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec![B2, B1]);
        assert!(!ids.contains(&POPUP), "floating popup must be left out");
    }

    #[test]
    fn tab_blocks_finds_the_window_from_the_source_block() {
        let ids: Vec<String> = tab_blocks(&fixture(), "", B3)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, vec![B3.to_string()]);
    }

    #[test]
    fn tab_blocks_falls_back_to_the_active_window() {
        let ids: Vec<String> = tab_blocks(&fixture(), "window:nope", "block:nope")
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, vec![B1.to_string(), B2.to_string()]);
    }

    #[test]
    fn tab_blocks_window_hint_wins() {
        let ids: Vec<String> = tab_blocks(&fixture(), W2, B1)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, vec![B3.to_string()]);
    }

    #[test]
    fn pane_title_rules() {
        assert_eq!(pane_title("editor", || unreachable!(), "b"), "editor");
        assert_eq!(pane_title("shell", || "~/src".into(), "b"), "~/src");
        assert_eq!(pane_title("shell", String::new, "b"), "shell");
        assert_eq!(pane_title("", String::new, "b"), "pane b");
    }

    #[test]
    fn last_lines_keeps_the_newest() {
        assert_eq!(last_lines("a\nb\nc\n", Some(2)), "b\nc\n");
        assert_eq!(last_lines("a\nb\nc", Some(2)), "b\nc");
        assert_eq!(last_lines("a\nb\n", Some(10)), "a\nb\n");
        assert_eq!(last_lines("a\nb\n", None), "a\nb\n");
        assert_eq!(last_lines("", Some(3)), "");
    }

    #[test]
    fn scrollback_read_captures_unwrapped_and_caps_lines() {
        let runner = FakeRunner::default().reply(Ok("1\n2\n3\n4\n"));
        let text = host(&runner, W1)
            .read_pane(B1, ReadSource::Scrollback, ReadFormat::Plain, Some(2))
            .unwrap();
        assert_eq!(text, "3\n4\n");
        let calls = runner.calls.borrow();
        assert_eq!(
            args(&calls[0]),
            vec!["capture", "-s", S, "-b", B1, "--unwrap", "--trim", "--format", "text"]
        );
    }

    #[test]
    fn viewport_read_keeps_the_bottom_rows() {
        let runner = FakeRunner::default()
            .reply(Ok("old\nr1\nr2\n"))
            .reply(Ok(r#"{"columns":80,"rows":2}"#));
        let text = host(&runner, W1)
            .read_pane(B1, ReadSource::Viewport, ReadFormat::Plain, None)
            .unwrap();
        assert_eq!(text, "r1\nr2\n");
        let calls = runner.calls.borrow();
        assert_eq!(
            args(&calls[0]),
            vec!["capture", "-s", S, "-b", B1, "--format", "text"]
        );
        assert_eq!(
            args(&calls[1]),
            vec!["block", "call", "-s", S, "-b", B1, "size"]
        );
    }

    #[test]
    fn insert_sends_text_through_stdin() {
        let runner = FakeRunner::default();
        host(&runner, W1).insert_text(B2, "-rf /tmp/x").unwrap();
        let calls = runner.calls.borrow();
        assert_eq!(args(&calls[0]), vec!["send", "-s", S, "-b", B2]);
        assert_eq!(calls[0].1.as_deref(), Some("-rf /tmp/x"));
    }

    #[test]
    fn list_tab_panes_uses_terminal_titles_for_generic_labels() {
        let runner = FakeRunner::default()
            .reply(Ok(include_str!(
                "../tests/fixtures/rex/session-inspect.json"
            )))
            .reply(Ok(r#"{"title":"~/src/zextract"}"#))
            .reply(Err("no title"));
        let panes = host(&runner, W1).list_tab_panes().unwrap();
        assert_eq!(
            panes,
            vec![
                PaneInfo {
                    pane_id: B2.into(),
                    title: "~/src/zextract".into()
                },
                PaneInfo {
                    pane_id: B1.into(),
                    title: "shell".into()
                },
            ]
        );
        let calls = runner.calls.borrow();
        assert_eq!(args(&calls[0]), vec!["session", "inspect", S, "--json"]);
    }

    #[test]
    fn cli_errors_are_passed_on() {
        let runner = FakeRunner::default().reply(Err("rex capture failed: block not found"));
        let err = host(&runner, W1)
            .read_pane(B1, ReadSource::Scrollback, ReadFormat::Plain, None)
            .unwrap_err();
        assert_eq!(err, "rex capture failed: block not found");
    }
    // ── topology + navigate (the `rex do` snapshot) ────────────────────

    const TOPOLOGY: &str = include_str!("../tests/fixtures/rex/topology-do.json");

    fn topology_host(runner: &FakeRunner) -> RexHost {
        RexHost::new(
            Box::new(runner.clone()),
            S.to_string(),
            SourceRef {
                pane_id: B1.to_string(),
                tab_id: W1.to_string(),
            },
        )
    }

    #[test]
    fn topology_snapshot_parses_titles_cwd_and_active() {
        let runner = FakeRunner::default().reply(Ok(TOPOLOGY));
        let host = topology_host(&runner);
        let snap = host.topology_snapshot().unwrap();
        // Recorded fixture: 3 sessions (rolling-hollow, nav-r0-target,
        // selftest), 4 windows, 6 blocks.
        assert_eq!(snap.sessions.len(), 3);
        assert_eq!(snap.windows.len(), 4);
        assert_eq!(snap.panes.len(), 6);
        // The source session is the active one; the source block the
        // focused pane. The fixture's sessions don't include our S, so
        // nothing is active here — assert that contract instead.
        assert!(snap.sessions.iter().all(|s| !s.active));
        assert!(snap.panes.iter().all(|p| !p.is_focused));
        // Terminal titles replace Rex's uninformative "shell" labels.
        assert!(snap
            .panes
            .iter()
            .any(|p| p.title == "~/p/m/w/h/herdr-flash"));
        // cwd from the file:// URL: path only, no scheme or host.
        assert!(snap.panes.iter().any(|p| p.cwd.as_deref()
            == Some(
                "/Users/stefan.marx/projekteHome/marxworld/workEnvironment/herdr/herdr-flash"
            )));
        // No program status reported anywhere → no agents (the fixture
        // carries records as {} — gopher-lua's empty-table shape).
        assert!(snap.agents.is_empty());
    }

    #[test]
    fn topology_snapshot_caches_until_invalidated() {
        let runner = FakeRunner::default().reply(Ok(TOPOLOGY));
        let host = topology_host(&runner);
        let _ = host.topology_snapshot().unwrap();
        let _ = host.list_all_panes().unwrap();
        let _ = host.list_sessions().unwrap();
        // One `rex do -` run serves every list method (the cached
        // snapshot); stdin carries the script.
        let calls = runner.calls.borrow();
        let do_calls: Vec<&Call> = calls
            .iter()
            .filter(|(args, _)| args.first().map(String::as_str) == Some("do"))
            .collect();
        assert_eq!(do_calls.len(), 1);
        assert_eq!(do_calls[0].0, vec!["do", "-"]);
        assert!(do_calls[0].1.as_deref().unwrap().contains("session.attach"));
    }

    #[test]
    fn close_pane_resolves_session_then_invalidates() {
        // The fixture's first block lives in rolling-hollow's window; the
        // snapshot maps block→session for the close call.
        let runner = FakeRunner::default()
            .reply(Ok(TOPOLOGY)) // snapshot for session_of_block
            .reply(Ok("")) //       the close itself
            .reply(Ok(TOPOLOGY)); // the post-close snapshot (cache was dropped)
        let host = topology_host(&runner);
        let target = "block:01a1da11-70e7-71a5-8205-a8e0868feaca";
        host.close_pane(target).unwrap();
        {
            let calls = runner.calls.borrow();
            let close = calls
                .iter()
                .find(|(args, _)| args.first().map(String::as_str) == Some("block"))
                .unwrap();
            assert_eq!(
                close.0,
                vec![
                    "block".to_string(),
                    "close".to_string(),
                    "-s".to_string(),
                    "session:01a1da11-70e7-708c-9cb3-986c917a8c85".to_string(),
                    target.to_string(),
                ]
            );
        }
        // The close invalidated the cache: the next snapshot runs the
        // script again.
        let _ = host.topology_snapshot().unwrap();
        assert_eq!(
            runner
                .calls
                .borrow()
                .iter()
                .filter(|(args, _)| args.first().map(String::as_str) == Some("do"))
                .count(),
            2
        );
    }

    #[test]
    fn jump_is_delegated_and_renames_close_via_cli() {
        let runner = FakeRunner::default()
            .reply(Ok(TOPOLOGY)) // snapshot for session_of_window
            .reply(Ok("")); // window rename
        let host = topology_host(&runner);
        // The app's view is client-side; the launcher carries the jump.
        assert_eq!(
            host.jump(Jump::FocusPane {
                pane_id: B1.to_string()
            })
            .unwrap(),
            JumpOutcome::Delegated
        );
        host.rename_window("window:01a1da11-7120-7c2d-a1ec-0dbcf977330d", "renamed")
            .unwrap();
        let calls = runner.calls.borrow();
        let rename = calls
            .iter()
            .find(|(args, _)| args.first().map(String::as_str) == Some("window"))
            .unwrap();
        assert_eq!(
            rename.0[1..],
            vec![
                "rename".to_string(),
                "-s".to_string(),
                "session:01a1da11-70e7-708c-9cb3-986c917a8c85".to_string(),
                "window:01a1da11-7120-7c2d-a1ec-0dbcf977330d".to_string(),
                "renamed".to_string(),
            ]
        );
    }

    /// Live check against a running Rex server: set REX_CLI (and
    /// optionally REX_SESSION) and run with --ignored. Verifies the
    /// script runs through `rex do -` (stdin) and the JSON parses.
    #[test]
    #[ignore = "needs a running Rex server; set REX_CLI"]
    fn live_rex_topology_snapshot() {
        let cli = std::env::var("REX_CLI").unwrap_or_else(|_| "rex".to_string());
        let session = std::env::var("REX_SESSION").unwrap_or_default();
        let host = RexHost::new(
            Box::new(CliRunner { cli }),
            session,
            SourceRef {
                pane_id: String::new(),
                tab_id: String::new(),
            },
        );
        let snap = host.topology_snapshot().unwrap();
        eprintln!("sessions: {}", snap.sessions.len());
        for s in &snap.sessions {
            eprintln!(
                "  {} ({} panes){}",
                s.label,
                snap.panes
                    .iter()
                    .filter(|p| p.session_id == s.session_id)
                    .count(),
                if s.active { " [active]" } else { "" }
            );
            for p in snap.panes.iter().filter(|p| p.session_id == s.session_id) {
                eprintln!("    - {} cwd={:?} title={:?}", p.pane_id, p.cwd, p.title);
            }
        }
        assert!(!snap.sessions.is_empty());
    }
}
