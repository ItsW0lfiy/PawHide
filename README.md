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

### GUI identity and window grouping (development update)

- `PawHideUI.exe` uses a compact, fixed-size purple/charcoal window, with its own draggable title bar, minimize and close controls, and **no maximize**.
- The approved Hidden Paw emblem is bundled as a PNG and Windows `.ico` resource; the executable, taskbar and UI header use the same identity.
- Application rows show the executable's extracted Windows icon in a 34×34 square on the left. Icon extraction runs off the UI thread and may fall back to a placeholder for restricted/system processes.
- Each application group offers **Hide / Show** directly on the row. Expanding is only for individually targeting windows (e.g., two Wolfmark documents).
- Windows shell (`explorer.exe`) bulk protection remains disabled in the UI for safety. Folder-aware protection is a separate future feature, not implemented here.
- AutoHide rules are unchanged. Only capture affinity is changed; other capture apps such as ShareX will also respect exclusion.

Build with `cargo build --release` (the CLI, agent, **and** UI are default members). `cargo build --release -p pawhide` builds only the CLI and its dependencies; `-p pawhide-ui` builds just the UI and its dependencies. The installer/distribution must put `pawhide.exe`, `pawhide_agent.dll`, and optionally `PawHideUI.exe` in the same folder for shared AutoHide controls.

**Validation:** Windows-specific GUI layout, icon extraction and resource compilation require testing on Windows. Source parsing on Linux is not equivalent to a Windows build or real capture test.


## v0.3.2 development: bounded protection, Explorer, folders

- **GUI single-instance:** launching PawHideUI again raises the existing window rather than spawning another GUI.
- **Timeout / failure handling:** manual GUI protection and AutoHide use short-lived CLI helpers with an eight-second timeout. This keeps the manager/monitor responsive if a target stalls DLL injection. A timed-out remote operation may still be pending, so **do not interpret a timeout as confirmed protection**. AutoHide waits two minutes before retrying a failed HWND.
- **Roblox:** DLL injection into RobloxPlayerBeta.exe is blocked deliberately; PawHide does not bypass anti-tamper systems and cannot protect that window using the current technique.
- **Explorer:** the GUI allows selecting individual file Explorer windows (CabinetWClass/ExploreWClass), while all-Explorer group protection remains disabled. Desktop/taskbar windows are blocked. Folder windows may still refuse injection; errors are reported safely.
- **Icons:** try each window's own icon before its executable icon, with fallback for classic GDI icons lacking a usable alpha channel.
- **Protected-folder rules (foundation):** configure canonical folder paths with `pawhide folders add "D:\Private"`, `remove`, `list`, and `match "D:\Private\file.md"`, or the **Folders** GUI page. A cooperating application can invoke `pawhide folders notify 0xHWND "D:\Private\file.md"` after opening a document. If the path is inside a configured folder, PawHide attempts to exclude **that window**, even if the application is absent from AutoHide rules. Rules are path-boundary checked, resolve symlinks, and never infer a file's location from just its window title. **Wolfmark is not yet connected to this notification interface, so folder rules are not globally automatic.** The notification only adds protection; moving out of a folder doesn't automatically unhide an already protected window.

**Windows capture caveat:** even verified affinity is best-effort and cannot guarantee secrecy against every capture method. Test in your own screen-share before depending on it.
