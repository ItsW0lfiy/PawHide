# PawHide 🐾

**PawHide** is a Windows-only, Rust-based CLI for excluding selected application windows from compatible screen captures while keeping them visible on your monitor.

> **Experimental.** Tested source ancestor ScreenHide v0.1.1 successfully excluded PicView in the project owner's Discord full-desktop share. The renamed CLI and automatic monitor still need Windows validation. Treat failures, startup races and unknown status as **not protected**.

## Build

Requires Windows 10 version 2004+ / Windows 11, Rust nightly and MSVC build tools / SDK.

```powershell
cargo build --release --workspace
cd target/release
./pawhide.exe list
```

Keep `pawhide.exe` and `pawhide_agent.dll` together. Rust nightly is project-scoped via `rust-toolchain.toml` because `dll-syringe` 0.18.2 uses an unstable Rust feature.

## CLI

```powershell
pawhide list                      # Compact per-application overview
pawhide list --windows            # Individual windows, handles and titles
pawhide list --all --windows      # Include tool/overlay windows
pawhide hide PicView.exe          # Exclude every visible PicView window
pawhide show PicView.exe          # Restore capture visibility
pawhide status PicView.exe        # Show OS affinity for matching windows
pawhide status                    # Summary of visible windows
pawhide hide 0x1127CE            # Exact HWND from list --windows

pawhide auto add PicView.exe      # Remember application name, not PID
pawhide auto remove PicView.exe
pawhide auto list
pawhide auto start                # Launch background monitor
pawhide auto status               # Also available as: pawhide auto
pawhide auto stop
```

Commands affecting multiple windows exit nonzero if **any** target window fails or can't be verified. `show` does not remove the persistent AutoHide rule; use `auto remove` before `show` if the monitor is active.

Configuration lives at `%LOCALAPPDATA%\PawHide\config.json`. It contains only application names, for example:

```json
{ "applications": ["picview.exe"] }
```

The monitor uses out-of-context WinEvent notifications plus periodic reconciliation, checks for already-excluded windows before attempting another remote call, and logs failures to `%LOCALAPPDATA%\PawHide\auto.log`. `auto stop` stops future monitoring but does not automatically unprotect windows already excluded.

## Security and limitations

- `SetWindowDisplayAffinity` only affects capture methods that respect it. **Verify from another Discord viewer** before using sensitive content. Status readback does not prove Discord behavior.
- Application creation, event delivery and the remote agent call are asynchronous. **AutoHide cannot guarantee zero-frame exposure.** Activate and verify protection *before* sharing or showing sensitive content.
- PawHide deliberately refuses blanket `explorer.exe` protection because Explorer may own desktop and taskbar windows. Advanced manual window-handle targeting requires `--allow-shell`, which can destabilize the Windows shell. Folder-specific AutoHide is not implemented yet.
- This backend **injects a native DLL into other processes**. Injection can fail or hang and can destabilize the target. It is not suitable for protected/elevated processes without matching privileges. The current build targets 64-bit Windows apps; x86 apps are not supported by this package.
- This is **not** DRM circumvention, encryption, antivirus bypass, or protection from someone physically looking at the monitor. Titles, notifications, thumbnails, other unprotected windows and some capture pipelines may still reveal content.
- The auto monitor starts only when requested; Windows-login startup will be added after validation. The monitor does not detect Discord's sharing status.

## Design

Rust workspace with `cli` (application commands, resolver, AutoHide monitor) and `agent` (tiny DLL exporting `PawhideApply`). The agent checks that the requested HWND belongs to its process before calling the Windows API. No external service and no Electron/WebView2.

Microsoft: [SetWindowDisplayAffinity](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity) · [SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook)
