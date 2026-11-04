# PawHide

![Original Hidden Paw emblem](ui/assets/pawhide.png)

PawHide is a native Rust utility for Windows. It excludes selected top-level windows from compatible screen captures while keeping them fully visible and usable on your physical monitor. The CLI, egui GUI, and affinity agent share the same capture-only purpose.

This is a **v0.3.3 development build**, not a v1.0 release. An `Excluded` status confirms Windows affinity readback, **not the actual output of Discord or every capture backend**. Verify a real test stream before sharing sensitive content. See [capture boundaries](docs/CAPTURE_BOUNDARY.md) and the [release checklist](docs/RELEASE_READINESS.md).

## Build and check on Windows

Requirements: Windows 10 2004+ or Windows 11, x64 MSVC Rust, and the Windows SDK. The project-scoped nightly toolchain is required by `dll-syringe` 0.18.2. Rustfmt and Clippy are declared in `rust-toolchain.toml`.

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked
cargo build --release --locked
cargo test --workspace --locked
```

Keep `target/release/pawhide.exe`, `PawHideUI.exe`, and `pawhide_agent.dll` together. Stop the background monitor and close PawHide before rebuilding files that are in use. A target application may retain an agent DLL until it exits; after an upgrade, restart affected applications if the agent export is incompatible. Do not forcibly unload code executing inside another process.

Windows CI performs formatting, linting, build, tests, installer fixture tests, and NSIS compilation. It uploads artifacts and **does not publish releases**, including on tag pushes. A Linux build cannot validate the Windows UI or capture engine.

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


## Native GUI

`PawHideUI.exe` opens at 900 x 580 logical pixels, resizes down to 800 x 520, follows Windows DPI scaling, and has no maximize button. The original Hidden Paw emblem is used throughout.

- **Windows:** searchable application table, status filters, sortable headings, executable icons, and expandable top-level windows. Explorer group actions stay blocked; expand to select individual folder windows. Long titles have tooltips.
- **Rules:** application AutoHide and protected Explorer folders. Saving a rule schedules monitoring without primary Start/Stop controls.
- **Capture:** native live monitor preview with automatic topology discovery, pause/resume, cursor control, and separate source, processed, and visible-preview frame rates.
- **Activity:** up to 200 session events with expandable diagnostics and a small tail of the local monitor log. No telemetry, network logging, or cloud service.
- **Settings:** general preferences, sign-in startup, capture quality/thumbnail, opt-in taskbar experiment, and About.

Protection, rule commands, and Explorer COM discovery run in disposable CLI helpers. Eight-second injection limits, cancellation, PID/HWND revalidation, and retry cooldowns prevent failures from blocking the interface. Killing a helper cannot retract a remote operation already started; its final affinity may remain uncertain.

Statuses are `Excluded`, `Visible`, `Partially excluded`, `Unsupported`, `Failed`, `Unknown`, or `Blacked / WDA_MONITOR`. Roblox anti-tamper targets and Discord capture clients are never injected. Unknown process identity is rejected. Floorp agent loading currently fails in the tested session; PawHide reports that failure without calling the window excluded.

## Automatic rules and ownership

`auto add` and `folders add` save rules and start the single per-session monitor. Opening PawHide also ensures configured monitoring is running, with bounded retries after startup failure. Monitoring is asynchronous and cannot guarantee zero-frame privacy for a newly opened window. CLI `auto start`, `stop`, `sync`, and diagnostics remain available to administrators and scripts. `auto stop` intentionally leaves existing exclusions in place.

```powershell
pawhide folders add "D:\Private"
pawhide folders remove "D:\Private"
pawhide folders list
pawhide folders match "D:\Private\document.md"
pawhide folders notify 0x12345 "D:\Private\document.md"
```

Explorer folder rules use ShellWindows filesystem locations, never titles. When a window leaves every matching rule, only PawHide-owned automatic affinity is restored, including a prior `WDA_MONITOR` state. Later manual exclusions take priority. HWND-owned properties survive monitor restarts and disappear when Windows destroys the window, avoiding stale PID/HWND tracking. `auto sync` also reconciles owned exclusions when no rules remain.

Tabs share an HWND and cannot be independently excluded. Any reported protected tab excludes the whole window. Missing locations, incomplete COM results, unavailable protected roots, or unaccounted native tab hosts retain automatic exclusion conservatively. Modern Explorer variants still require acceptance testing; uncertainty can intentionally leave a window excluded. An exclusion created by an older PawHide without ownership metadata cannot safely be classified as automatic: restore that window manually if appropriate.

Document applications need explicit file-open integration through `folders notify`. Wolfmark does not yet provide this callback. Document notifications only add protection; automatic document-close cleanup is not implemented.

The existing `%LOCALAPPDATA%\PawHide\config.json` application/folder format remains readable. Optional `preferences` fields default safely when absent. Writes use temporary-file replacement and retain the previous valid configuration if replacement fails. Startup uses only the user-scoped `PawHide Auto` Run entry.

## Capture engine and performance

The Windows Graphics Capture service continues across page navigation. A bottom-right status chip returns to Capture; an optional floating thumbnail is available in Settings. Display selection is preserved by device name across topology changes; a disconnected source is not silently replaced by a different display.

Acquisition wakes on WGC frame events, drains stale frames, and counts distinct source timestamps. A reusable three-texture staging ring uses nonblocking readback. Larger sources scale on the GPU to fit 1920 x 1080 with the native D3D11 video processor; native-resolution preview is optional. Opaque pixels move directly into egui's image buffer, avoiding a second full-frame conversion. The visible preview wakes when a completed frame is ready. Cursor changes update the session without restarting it.

The target is smooth 60 FPS on an actively updating supported source. Counters report acquired frames, processed readbacks, and distinct images submitted to a visible preview separately; static sources and hidden previews can report lower rates. Initial development-session samples were approximately 50-56 FPS; sustained 60 FPS remains an acceptance item, not a verified claim. No duplicate frames are manufactured. HDR tone mapping and recording are not implemented. No FFmpeg dependency is used.

## Distribution and installation

- `PawHide-setup.exe`: small per-user NSIS online installer, Start Menu entries, Apps registration, and uninstall support.
- `PawHide-Windows-x64.zip`: portable CLI, GUI, agent, and README, with a SHA-256 companion manifest.

The installer anonymously fetches the latest **public** GitHub release metadata, pins both ZIP and checksum to that release, validates the exact manifest filename and SHA-256, checks ZIP entries, then installs only the three required binaries. It does not prompt ordinary users for GitHub credentials. The repository is currently private: public online installation cannot succeed until public distribution is enabled and matching release assets exist. Developers can use authorized workflow artifacts directly while it remains private.

Close PawHide and protected applications before upgrading or uninstalling if Windows locks their binaries. Maintenance stops the monitor with a deadline and never terminates target applications. Upgrades preserve rules and unrelated files, attempt rollback on a copy failure, and retain backups if rollback fails. Uninstall removes only PawHide files, shortcuts, its Apps key, and an exactly matching startup command. User configuration and logs survive uninstall. Open PawHide after maintenance to resume configured monitoring, including after an aborted upgrade; the installer offers this on its Finish page.

With NSIS installed, create `dist` and run:

```powershell
New-Item -ItemType Directory -Force dist | Out-Null
& "${env:ProgramFiles(x86)}\NSIS\makensis.exe" installer/PawHide.nsi
powershell.exe -NoProfile -ExecutionPolicy Bypass -File installer/test-bootstrap.ps1
```

The installer fixtures do not contact GitHub or alter a real installation. They cover checksums, ZIP traversal/missing components, and rollback. Real public download, upgrade, shortcuts, Apps registration, and uninstall still need an isolated user-account acceptance test.

## Diagnostics and branding

The original logo had a fully opaque charcoal outer matte. `tools/Restore-Logo.ps1` removes only the rounded badge's exterior matte, feathers and unmattes the outer glow, and preserves interior artwork pixels. It generates PNG and 32-bit ICO entries at 16, 20, 24, 32, 40, 48, 64, 96, 128 and 256 pixels. Supply an unmodified original as `-Source`; do not repeatedly process the restored PNG. No AI regeneration was used.

Additional interactive checks:

```powershell
cargo test -p pawhide-agent --locked native_ownership_roundtrip -- --ignored --nocapture
cargo test -p pawhide-ui --release --locked gpu_scaling_and_readback -- --ignored --nocapture
cargo test -p pawhide-core --locked explicit_titleless_taskbar_lookup -- --ignored --nocapture
./tools/Test-Protection.ps1 -Application floorp.exe
```

The protection script requires one initially visible application window, uses a bounded helper, and restores its original capture affinity afterward where possible. A failure is an error, not proof of privacy. Never target anti-tamper software or a capture client.

## License decision required

Cargo metadata already declares `MIT`, but the repository has no standalone license text. No legal terms have been changed or new license selected in this work. The owner must confirm the intended terms and copyright notice before distribution.

| Option | Relevant tradeoff |
| --- | --- |
| [MIT](https://opensource.org/license/mit) | Short permissive terms; retain copyright and permission notices. |
| [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0) | Permissive with an explicit contributor patent grant and additional notice obligations. |
| [GPL-3.0](https://opensource.org/license/GPL-3.0) | Distributed modified versions carry source-sharing and copyleft obligations. |

Dependencies and branding assets also need a distribution-notice review before a public release. Version 1.0 and public publication remain owner decisions.
