# PawHide 🐾

**PawHide** is a Windows desktop screen-capture privacy utility written in Rust. It marks selected application windows as excluded from compatible captures while leaving them visible on your local monitor.

PawHide has a **CLI-first development priority** and an **optional native egui UI**, `PawHideUI.exe`. Both are built by the default Cargo workspace build. The GUI is an alternative way to manage windows, **not a replacement for the CLI**.

## Status

The v0.2 CLI/manual injection and AutoHide were tested successfully on the author's Windows desktop with PicView, Wolfmark and Discord. **The v0.3 refactor and UI are new and still need Windows compilation and testing.** Treat any failed or unknown status as unprotected. Protection does not provide guaranteed privacy against every capture method.

## Build

Windows 10 2004+ or Windows 11, x64 MSVC toolchain + Windows SDK. This project currently requires **project-scoped Rust nightly** because `dll-syringe` 0.18.2 uses an unstable feature.

```powershell
cargo build --release
cargo test --workspace
```

The default build produces `pawhide.exe`, `PawHideUI.exe`, and `pawhide_agent.dll`. Keep `pawhide.exe` and `pawhide_agent.dll` in the same folder. The UI uses the same DLL for protection, and expects `pawhide.exe` alongside it only for controlling the AutoHide worker (other UI features use the shared library directly).

Build a single frontend when iterating:

```powershell
cargo build --release -p pawhide
cargo build --release -p pawhide-ui
```

## CLI

```powershell
pawhide list                           # Grouped applications and window counts
pawhide list Wolfmark.exe              # Individual windows belonging to an application
pawhide list --windows                 # Show all individual windows
pawhide list --all --windows           # Include utility/overlay windows
pawhide list --json                    # Machine-readable window inventory
pawhide inspect PicView.exe            # Detailed title, HWND, PID, affinity
pawhide inspect PicView.exe --json

pawhide hide PicView.exe               # Exclude all matching windows
pawhide show PicView.exe               # Restore normal capture visibility
pawhide hide Wolfmark.exe --title Concept.md
pawhide hide --pick                    # Interactive numbered window selection
pawhide show --pick
pawhide hide 0x1127CE --verbose        # Specific HWND (diagnostics)
pawhide status
pawhide status PicView.exe
pawhide status PicView.exe --json

pawhide auto add PicView.exe           # Persistent, executable-based rules
pawhide auto remove PicView.exe
pawhide auto list
pawhide auto status
pawhide auto start                     # Start background monitor
pawhide auto sync                      # Apply rules to current windows once
pawhide auto stop                      # Stop monitoring; existing exclusions remain

pawhide startup enable                 # Start AutoHide at user login
pawhide startup status
pawhide startup disable
pawhide config path
pawhide config show
pawhide config validate
pawhide config edit
pawhide logs path
pawhide logs show --lines 50
pawhide logs follow
pawhide version
```

Use `--title` to match a specific window by partial title. It intentionally refuses ambiguous multiple matches unless you use `--pick`; the numbered picker is a **terminal selector**, not a mouse-based desktop picker. The GUI offers per-window selection. For automated scripts, use the `--json` options and exit status instead of parsing human-readable output.

## UI

`PawHideUI.exe` is a separately built egui/eframe application with a dark native window, per-application expandable lists, individual window hide/show controls, an AutoHide rule editor, and settings information. It shares `pawhide-core` with the CLI. The AutoHide Start/Stop buttons invoke the primary CLI beside it, keeping a single worker implementation rather than two competing monitors.

## Configuration and Windows startup

Rules are stored at `%LOCALAPPDATA%\PawHide\config.json`:

```json
{"applications":["picview.exe"]}
```

`startup enable` registers **only a per-user PawHide entry** in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. It does not install a service or require admin privileges. Run `auto add` before enabling startup. Startup is disabled by default.

`auto start` launches a background monitor that responds to window create/show events, with periodic reconciliation. AutoHide is asynchronous: **it cannot guarantee zero-frame exposure of a newly opened window**. Hide and verify sensitive windows *before* starting Discord sharing. `auto stop` does not undo existing protection; `auto remove` does not unhide an already protected window.

## Limitations and security

- `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` affects **all compatible capture applications**, including ShareX, not only Discord. PawHide does not inject into Discord or bypass any capture security.
- The external DLL agent is injected into selected processes. Injection can fail, freeze, or destabilize a target; avoid privileged/security-sensitive processes. The released agent and CLI target x64 Windows applications.
- Shell-wide `explorer.exe` targeting is blocked to protect the desktop/taskbar. Folder-aware Explorer rules and tab-aware browser protection **are not implemented**.
- Status readback only confirms Windows affinity, not that Discord or a different capture pipeline hides the window.
- PawHide does not hide notification text, taskbar previews, logs, titles or other unprotected content. It is not a substitute for privacy-conscious sharing choices.

## Workspace

- `core/` — shared Rust library for discovery, protection, and configuration
- `cli/` — primary `pawhide.exe` commands and AutoHide worker
- `ui/` — optional `PawHideUI.exe` built with egui/eframe
- `agent/` — minimal remote DLL for setting affinity in the target process

No WebView2, Electron, network API, telemetry or persistent server. A local IPC API is a possible future extension, **not implemented in this release**.
