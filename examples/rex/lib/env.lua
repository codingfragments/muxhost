-- ~/.config/rex/lib/env.lua — the shared environment for Rex launchers
-- (herdr-nav, herdr-flash, herdr-zextract, vim-herdr-navigation).
-- Canonical sample from the muxhost docs (doc/rex-environment.md): copy it
-- as it is. Exports M.path, M.editor, M.rex_cli.
--
-- Shared environment for actions: the user's login PATH, $EDITOR and
-- the rex CLI path. Resolved once per config load.
--
-- The Rex server is launched by the GUI app with a minimal launchd
-- PATH (no homebrew/mise), so commands are wrapped in
-- env PATH=<login PATH>. That PATH, and $EDITOR, are read from the
-- account's own LOGIN SHELL — whichever it is, wherever it lives:
--   1. find the login shell: the system's record (dscl on macOS,
--      getent on Linux), else $SHELL (not guaranteed to be set in the
--      server's environment, so it is only the fallback);
--   2. ask it once, with the right syntax for its family — fish keeps
--      $PATH as a list (`string join :`), bash/zsh/sh already have it
--      colon-joined. Both print marked lines (REXPATH= / REXEDITOR=),
--      so a login banner on stdout cannot be mistaken for the answer.
-- Caveat: `-l` reads the LOGIN files (fish: config.fish; zsh:
-- .zprofile/.zlogin; bash: .bash_profile) but not .zshrc/.bashrc. A
-- PATH set only there is missed. `-i` would catch it but runs the whole
-- interactive setup: measured with zsh here, 0.05 s -> 2.8 s plus 13
-- lines of stderr noise, for 20 more PATH entries. Put PATH/EDITOR in
-- the login files (or export them from there) to be found.
local M = {}

local HOME = os.getenv("HOME") or ""

-- What is used when the login shell cannot be asked: the usual system
-- locations plus the per-user ones a mise/pipx/cargo install lands in,
-- so a tool installed that way is still found.
local function default_path()
  local dirs = {}
  if HOME ~= "" then
    dirs[#dirs + 1] = HOME .. "/.local/share/mise/shims"
    dirs[#dirs + 1] = HOME .. "/.local/bin"
    dirs[#dirs + 1] = HOME .. "/.cargo/bin"
  end
  for _, d in ipairs({ "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin",
                       "/usr/sbin", "/sbin" }) do
    dirs[#dirs + 1] = d
  end
  return table.concat(dirs, ":")
end

local function is_executable(path)
  if not path or path == "" or path:sub(1, 1) ~= "/" then return false end
  local f = io.open(path, "r")
  if not f then return false end
  f:close()
  return true
end

local function trim(s) return ((s or ""):gsub("[%c%s]+$", ""):gsub("^%s+", "")) end

-- The account's login shell (an absolute, existing path) or nil.
function M.login_shell()
  local r = rex.exec{ "/bin/sh", "-c", [[
u=$(id -un 2>/dev/null)
s=$(dscl . -read "/Users/$u" UserShell 2>/dev/null | awk '{print $2}')
[ -n "$s" ] || s=$(getent passwd "$u" 2>/dev/null | cut -d: -f7)
printf '%s' "$s"
]] }
  if r and r.code == 0 then
    local s = trim(r.stdout)
    if is_executable(s) then return s end
  end
  local fallback = os.getenv("SHELL")
  if is_executable(fallback) then return fallback end
  return nil
end

-- Ask `shell` for its login PATH and EDITOR. Returns path, editor
-- (either may be nil when the shell did not give a usable answer).
function M.shell_env(shell)
  if not is_executable(shell) then return nil, nil end
  local script
  if (shell:match("[^/]+$") or "") == "fish" then
    script = [[printf 'REXPATH=%s\n' (string join : $PATH); printf 'REXEDITOR=%s\n' "$EDITOR"]]
  else
    script = [[printf 'REXPATH=%s\nREXEDITOR=%s\n' "$PATH" "$EDITOR"]]
  end
  local r = rex.exec{ shell, "-l", "-c", script }
  if not r or r.code ~= 0 or not r.stdout then return nil, nil end
  -- The LAST marker line wins: anything the shell's startup files
  -- print comes before the -c script's own output.
  local path, editor
  for v in r.stdout:gmatch("REXPATH=([^\n]*)") do path = v end
  for v in r.stdout:gmatch("REXEDITOR=([^\n]*)") do editor = v end
  path = path and trim(path) or nil
  editor = editor and trim(editor) or nil
  if path and not path:find("/", 1, true) then path = nil end -- not a PATH
  if editor == "" then editor = nil end
  return path, editor
end

M.path = default_path()
M.editor = "vim"
do
  local shell = M.login_shell()
  local path, editor = M.shell_env(shell)
  if path then M.path = path end
  if editor then M.editor = editor end
end

-- The rex CLI at an absolute path: the server's own PATH does not
-- include it, and config actions cannot invoke server actions (only
-- scripts can). Resolved without assuming an account, a home directory
-- or an install, first hit wins:
--   1. $REX_CLI, an explicit override;
--   2. the app-bundle location under THIS account's $HOME (macOS);
--   3. `rex` on the login PATH (a CLI the user installed themselves);
--   4. the binary of the server this config runs in: the server and the
--      CLI are the same executable, so the parent of a process the
--      config spawns IS the path (`ps`, or /proc on Linux).
-- Falls back to the bare name "rex" (found only if it is on PATH).
local BUNDLE_CLI = "/Library/Application Support/com.superlogical.rex/"
  .. "RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex"

local function executable(path)
  if not path or path == "" then return false end
  local f = io.open(path, "r")
  if not f then return false end
  f:close()
  return true
end

local function trim(s) return (s or ""):gsub("[%c%s]+$", ""):gsub("^%s+", "") end

-- `home` is a parameter so each step can be tested on its own.
function M.resolve_rex_cli(home, login_path)
  local explicit = os.getenv("REX_CLI")
  if executable(explicit) then return explicit end

  if home and home ~= "" and executable(home .. BUNDLE_CLI) then
    return home .. BUNDLE_CLI
  end

  local r = rex.exec{ "/usr/bin/env", "PATH=" .. login_path, "/bin/sh", "-c", "command -v rex" }
  if r and r.code == 0 then
    local p = trim(r.stdout)
    if executable(p) then return p end
  end

  r = rex.exec{ "/bin/sh", "-c",
    "readlink /proc/$PPID/exe 2>/dev/null || ps -o comm= -p $PPID" }
  if r and r.code == 0 then
    local p = trim(r.stdout)
    if p:match("/rex$") and executable(p) then return p end
  end

  return "rex"
end

M.rex_cli = M.resolve_rex_cli(os.getenv("HOME"), M.path)

return M
