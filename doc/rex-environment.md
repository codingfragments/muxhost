# The Rex environment for launchers

> For anyone installing a Rex plugin built on muxhost (herdr-nav,
> herdr-flash, herdr-zextract) or the pure-Lua vim-herdr-navigation.
> Each plugin's own `doc/rex.md` links here.

## What goes wrong without it

Every one of these plugins is a **Lua launcher** in your Rex config
(`~/.config/rex/plugins/<name>.lua`) that opens a floating layer and runs
a program in it. The launcher needs two facts that Rex does not hand it:

| It needs | Because | Without it |
|---|---|---|
| **The `rex` CLI's absolute path** | The tools call back into Rex (`rex capture`, `rex send`, `rex focus`, …), and the server's `PATH` does not contain the CLI. | The popup opens but the tool fails with `cannot run the rex CLI`. |
| **Your login `PATH`** | The Rex server is started by the macOS app with launchd's minimal environment. Inside its config, `PATH` is `/usr/bin:/bin:/usr/sbin:/sbin`: no Homebrew, no mise shims, no `~/.local/bin`. | The launcher's check for the plugin's binary fails and the popup says `<tool> not found on this host` even though the tool is installed. |

Measured on a real setup: the server's config context has `HOME` and
`USER` set, `SHELL` set here (not guaranteed), and `PATH` as above.

## What each launcher does on its own

All four launchers resolve both facts the same way:

```lua
local login_path = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
local rex_cli = "rex"
local ok, env = pcall(require, "lib.env")      -- your ~/.config/rex/lib/env.lua
if ok and env and env.path and env.path ~= "" then login_path = env.path end
if ok and env and env.rex_cli and env.rex_cli ~= "" then
  rex_cli = env.rex_cli
else
  -- no lib/env.lua: try the app bundle under THIS account's $HOME
  local bundle = (os.getenv("HOME") or "") .. "/Library/Application Support/"
    .. "com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex"
  local f = io.open(bundle, "r")
  if f then f:close() rex_cli = bundle end
end
```

| | With `lib/env.lua` | Without it |
|---|---|---|
| `rex` CLI | whatever `env.rex_cli` says | the bundle under `$HOME` (works for a standard macOS install, any account) |
| `PATH` | whatever `env.path` says | **the fixed list above**, which has *no* mise shims and no `~/.local/bin` |

So a standard macOS install finds the CLI with no setup, but a binary
installed with **mise** (`mise use -g github:codingfragments/herdr-nav`)
is **not found** until you provide a `lib/env.lua`. That is the one
setup step this page is about.

## Setting it up

Create `~/.config/rex/lib/env.lua` (one file for all the launchers).
Two options.

### Option A: the sample (recommended)

[`examples/rex/lib/env.lua`](../examples/rex/lib/env.lua) is a
self-contained file you can copy as it is:

```sh
mkdir -p ~/.config/rex/lib
curl -fsSL https://raw.githubusercontent.com/codingfragments/muxhost/main/examples/rex/lib/env.lua \
  -o ~/.config/rex/lib/env.lua
```

It exports three values, each resolved once per config load:

| Value | How it is found (first hit wins) |
|---|---|
| `rex_cli` | 1. `$REX_CLI` if it names an executable · 2. the app bundle under this account's `$HOME` · 3. `rex` on the login PATH · 4. the binary of the running server (the server and the CLI are the same executable, so the parent of a spawned process is the path) · else the bare name `rex` |
| `path` | your **login shell's** `PATH`, asked once: the shell is found from the system's record (`dscl` on macOS, `getent` on Linux), else `$SHELL`; fish is asked with `string join :`, bash/zsh/sh with `$PATH` as it is. If the shell can't be asked: the fixed list plus `~/.local/share/mise/shims`, `~/.local/bin` and `~/.cargo/bin` |
| `editor` | the login shell's `$EDITOR`, else `vim`. Used by editor-launching actions such as an `edit_scrollback` plugin; the muxhost plugins don't use it |

Nothing in it names an account or a home directory.

### Option B: fixed values

If you would rather not run your login shell at every config load (a
fish login costs about 1.7 s), write the values down:

```lua
-- ~/.config/rex/lib/env.lua
local home = os.getenv("HOME")
local M = {}
M.path = home .. "/.local/share/mise/shims:" .. home .. "/.local/bin:"
  .. "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
M.rex_cli = home .. "/Library/Application Support/com.superlogical.rex/"
  .. "RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex"
M.editor = "vim"
return M
```

Put every directory your tools live in on `path`: add `~/.cargo/bin`,
`/opt/dogbrew/bin` or whatever your install uses.

## Check it

Reload the config (`ctrl+s>shift+r`, or `rex api call server.config_reload`)
and ask the file what it resolves to:

```sh
rex do -e "local e = dofile(os.getenv('HOME')..'/.config/rex/lib/env.lua'); \
  return { rex_cli = e.rex_cli, editor = e.editor, path_has_mise = e.path:find('mise', 1, true) ~= nil }"
```

(`rex` is the CLI; if it is not on your `PATH`, use its full path, for
example the one option A resolves to.) `rex_cli` should be an existing
file, and `path_has_mise` should be `true` if your tools come from mise. Then open the plugin: if the popup
shows the tool, the environment is right.

## Troubleshooting

| What you see | Cause | Fix |
|---|---|---|
| Popup: `<tool> not found on this host` | the launcher's `PATH` lacks the directory the binary is in (typically mise shims) | provide `lib/env.lua` (above); confirm with the check that `path` contains the binary's directory |
| Popup opens, tool exits with `cannot run the rex CLI` | `rex_cli` is wrong or the CLI moved | set `$REX_CLI`, or fix `M.rex_cli`; the check shows the value |
| Popup: `<tool> is too old (host-api …)` | the binary is older than the launcher expects | update the binary (`mise up <tool>`) |
| `path` in the check is the short fixed list | the login shell could not be asked (missing, or its login files print errors) | run `<your shell> -l -c 'echo $PATH'` by hand; or use option B |
| `path` is missing a directory that your shell has in an interactive session | `-l` reads the login files, not `.zshrc`/`.bashrc` | export `PATH` from the login files, or use option B. (Asking an interactive shell would take 2.8 s for zsh in a measurement, with stderr noise.) |

## For plugin authors

A launcher should keep the same shape: `pcall(require, "lib.env")` first,
then an inline fallback that works for a standard install. Pass the
result to the tool as `REX_CLI` and run the tool as
`env PATH=<login path> …`, so the tool's own subprocesses (`rex`, `git`,
`zoxide`, …) resolve the same way. The tool side (`RexHost::from_env`)
reads `REX_CLI`; see [`hosts.md`](hosts.md#environment-1).
