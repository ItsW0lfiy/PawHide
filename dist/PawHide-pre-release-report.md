# PawHide pre-release development report

The v0.3.3 development rebuild is available locally on `codex/pre-release-rebuild`, ending at `4d7226f364a3f58dfb182a892c22438245b28d37`. It is a review build, not a release-ready privacy guarantee. No push, tag, GitHub release, version 1.0, or license change was performed.

## Review artifacts

- [Portable Windows x64 ZIP](D:/Projects/PawHide/dist/PawHide-Windows-x64.zip) — 3,974,746 bytes; CLI, GUI, agent, README, capture boundaries, readiness checklist and original logo.
- [Portable SHA-256 manifest](D:/Projects/PawHide/dist/PawHide-Windows-x64.zip.sha256).
- [Online NSIS bootstrap](D:/Projects/PawHide/dist/PawHide-setup.exe) — 333,484 bytes. It cannot download this private repository's release assets anonymously; public distribution must be enabled separately.
- [Logo before/after on dark and light backgrounds](C:/Users/loene/.codex/visualizations/2026/11/03/01a1a37d-7cb5-7f81-9a1b-3d8d3a8c2df7/logo-restoration.png).
- [Actual native GUI at its tested 800 x 520 minimum](C:/Users/loene/.codex/visualizations/2026/11/03/01a1a37d-7cb5-7f81-9a1b-3d8d3a8c2df7/PawHide-minimum-size.png).

SHA-256:

```text
DA51443C461F71B1581D59C50722388CD8A9E74D37F00B211AC989A742BA3F10  PawHide-Windows-x64.zip
07461AF17B98C068D3DBAD5F014779C60E3445F5830D22B8125148188CA9D4F9  PawHide-setup.exe
```

## Implemented behavior

Protection remains capture-affinity only. External windows are never locally hidden, minimized, restyled out of task switching, covered, moved or resized. Roblox anti-tamper targets and Discord capture clients are rejected for injection. Experimental taskbar handling remains explicitly opt-in; validation did not inject into the taskbar.

Injection and Explorer COM discovery use disposable, bounded CLI workers. HWND, PID and executable identity are revalidated, output is drained with bounded retention, and timeout/cancellation kills and reaps the worker. A failure cannot panic the GUI. A killed worker cannot retract a remote operation already started, so timeouts are reported as unverified and require readback/capture checks. Failure cooldowns prevent rapid retries.

The affinity agent retains the original ABI and adds serialized ownership-aware operations. Window properties distinguish automatic ownership, the previous affinity mode, and later manual protection. Automatic cleanup restores only its own state when all rules are certainly outside; manual protection and overlapping/uncertain rules retain exclusion. HWND destruction clears ownership. Explorer matching reads actual ShellWindows paths, normalizes top-level HWNDs and conservatively handles incomplete locations/tab information. Bulk Explorer protection remains blocked; individual folder windows remain selectable.

Configured rules schedule singleton monitoring automatically. Startup, rule saves and operations remain asynchronous to the GUI. CLI diagnostics and manual monitor controls remain available. Existing configuration loads safely, default preferences serialize in the old rules schema, and a failed atomic replacement preserves the previous valid file.

The native egui GUI now has a compact sidebar, aligned application table with real icons, search/filter/sort, expandable windows, accurate affinity statuses, cancellation/progress, bounded activity history, organized settings and a persistent status bar. It defaults to 900 x 580 logical pixels, has no maximize button, and was actually resized to 800 x 520. Coalesced pointer events are handled so the frameless resize does not lose its initial movement. AccessKit is enabled; screen-reader acceptance remains pending.

Capture persists across page navigation. WGC events drive acquisition, stale frames are drained, and distinct source timestamps are counted. A reusable three-texture staging ring uses nonblocking readback, GPU video processing scales larger sources to fit 1920 x 1080, and native-resolution preview is optional. Pixel buffers move directly into egui without a second complete conversion. Cursor preferences update without restarting the session. Source identity is retained through topology changes, with a persistent capture chip and an optional thumbnail. The worker avoids blocking GUI shutdown on a stalled driver. No FFmpeg or web runtime was added.

The original Hidden Paw logo was restored, not redesigned. Its outer matte was embedded in fully opaque source pixels. Deterministic alpha cleanup preserves the interior artwork, feathers/unmattes the outer edge, and generates ten ICO sizes from 16 through 256 pixels. GUI, executable and installer use the same assets. No generative image tool was used.

The NSIS installer keeps its existing online design, exact `PawHide-setup.exe` name, per-user install, Start Menu shortcuts, Apps registration and uninstall support. Downloads pin ZIP and checksum to one public release, validate SHA-256 and archive entries, and require all three binaries. Maintenance stops the monitor with a deadline, checks file locks, preserves configuration/unrelated files and removes only an exactly owned startup command, including legacy extended paths. Copy failures roll back previous binaries; failed rollback retains backups. Finish offers opening PawHide to resume configured rules. CI builds and uploads artifacts without publishing releases.

## Validation performed on native Windows

The final source passed:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --release --locked
cargo test --workspace --locked
```

There are **21 passing regular tests**: CLI 5, core 13, UI 3. Four tests are intentionally ignored by the normal suite: three native checks and one child-stall fixture. The stall fixture is launched only by the timeout/cancellation regression.

These explicit native checks also passed:

```powershell
cargo test -p pawhide-agent --locked native_ownership_roundtrip -- --ignored --nocapture
cargo test -p pawhide-ui --release --locked gpu_scaling_and_readback -- --ignored --nocapture
cargo test -p pawhide-core --locked explicit_titleless_taskbar_lookup -- --ignored --nocapture
```

They verify automatic affinity restoration/manual priority/prior WDA_MONITOR restoration on test-owned windows, D3D11 2560 x 1440 to 1920 x 1080 color preservation, and read-only resolution of a real titleless taskbar. A one-frame GPU timing is not a steady-state FPS benchmark.

`powershell.exe -NoProfile -ExecutionPolicy Bypass -File installer/test-bootstrap.ps1` passed fresh install, corrupt checksum, traversal, missing DLL, partial-copy rollback, unrelated-file preservation, byte-for-byte upgrade rollback, and mock-registry startup ownership tests. NSIS 3.13 compiled the final bootstrap successfully. The locally generated portable ZIP was passed through the real bootstrap using mocked local downloads into a new target directory: all three installed x64 PE files matched the release build byte-for-byte, and the packaged CLI reported PawHide 0.3.3. This does not validate real public networking or Windows Apps registration.

Interactive checks covered both connected displays, switching, pause/resume, cursor toggles without session restart, capture continuity on Rules/Activity, table/icons/navigation, actual minimum-size resize, and live Explorer private-folder-to-parent restoration. A later manual pin survived navigation; another Explorer window remained visible. The temporary folder rule/window were removed afterward. The user's saved rules were retained and AutoHide was resumed, reporting Running.

Floorp was reproduced through a child: `dll-syringe` asserted on a null injected-module handle. The same failure now exits with a readable error and no GUI crash; Floorp remains alive with visible affinity. This proves failure containment, not successful Floorp exclusion or the underlying loader cause.

## Outstanding acceptance gates

- **60 FPS remains unverified.** Observed native GUI samples were approximately 50–56 source/processed FPS. The pacing regression admits jittered 60 Hz and bounds high-refresh work without duplicates, but it cannot prove sustained runtime throughput. Continue profiling on a controlled animated source and representative hardware.
- **Floorp agent loading still fails in the tested session.** Investigate loader/process compatibility without bypassing security mechanisms. Do not advertise Floorp support on this evidence.
- Repeat the actual Discord entire-monitor stream with fullscreen/borderless Roblox under protected PicView and individual Explorer windows. Verify local visibility/usability/task switching and viewer output independently of affinity readback. Distinguish WDA_MONITOR blacking from WDA_EXCLUDEFROMCAPTURE and backend behavior.
- Live multi-tab ambiguity, overlapping rules, HWND recreation/restart stress, physical display unplug/reconnect, resolution changes and monitor-process ownership continuity need wider acceptance testing.
- Verify 100/125/150/200% DPI, resize enlargement/edge cases, keyboard focus and screen-reader labels. The optional thumbnail and taskbar experiment need separate acceptance. The native GPU test does not establish accessibility or capture privacy.
- Test real public download/network failure, upgrade/reinstall, Apps registration, shortcuts, locked binaries, rollback failure and uninstall in an isolated Windows account. The online installer cannot work publicly until the owner enables public assets. No installer was run against the user's real installation.
- Confirm licensing. Existing Cargo metadata says MIT but there is no standalone license. MIT is short/permissive; Apache-2.0 adds an explicit patent grant and notice requirements; GPL-3.0 adds distributed-source/copyleft obligations. No option was selected or legal terms changed. Review dependency notices, branding ownership and signing before public distribution.

Other boundaries: AutoHide cannot guarantee zero-frame exposure before discovery. Uncertain folder data intentionally retains automatic exclusion. Older unowned exclusions need an explicit manual decision. An injected DLL can remain mapped until its target exits. Installer maintenance stops monitoring; open PawHide afterward, including after an aborted upgrade. HDR tone mapping, recording and document-close integration are absent. No telemetry was added.

## Files changed and local history

39 files changed across focused local commits:

| Area | Files |
| --- | --- |
| Affinity, ownership, discovery, configuration | `agent/src/lib.rs`; `core/src/config.rs`, `explorer.rs`, `folders.rs`, `helper.rs`, `lib.rs`, `monitor.rs`, `ownership.rs`, `platform.rs` |
| CLI and monitoring | `cli/src/auto.rs`, `commands.rs`, `main.rs`, `presentation.rs`, `startup.rs`; `cli/build.rs` |
| Native GUI and capture | `ui/src/app.rs`, `capture_native.rs`, `capture_test.rs`, `icons.rs`, `main.rs`, `operations.rs`, `single_instance.rs`; `ui/Cargo.toml`, `ui/build.rs`; `Cargo.lock` |
| Original branding and diagnostics | `ui/assets/pawhide.png`, `PawHide.ico`; `tools/Restore-Logo.ps1`, `Test-Protection.ps1` |
| Installer/build/docs | `installer/PawHide.nsi`, `bootstrap.ps1`, `maintenance.ps1`, `test-bootstrap.ps1`; `.github/workflows/build.yml`, `.gitignore`, `rust-toolchain.toml`, `README.md`, `docs/CAPTURE_BOUNDARY.md`, `docs/RELEASE_READINESS.md` |

AccessKit accounts for the additional locked accessibility dependencies. Required rustfmt-only changes to existing Windows support files were isolated in the first commit.

```text
9f84e45 style: apply required rustfmt to Windows support code
c5b0b35 fix: isolate affinity failures and track automatic window ownership
9bd8408 feat: rebuild native manager and persistent GPU-scaled capture preview
9ba6e04 fix: restore original Hidden Paw transparency and Windows icon sizes
16796fd fix: restrict shell helpers and resolve explicit titleless taskbars
b754dd5 build: harden online installer and validate Windows distribution
4d7226f docs: document capture guarantees and remaining release gates
```

The branch started at `ec593620d1863d98f4a408a2b6d353effd4bcdcd`. A final `git fetch origin main` confirmed local and remote main still at that same commit; there were no new main commits to integrate. The local checkout is returned to main, while the development branch and packaged artifacts are preserved. The pre-existing untracked `vendor-config.toml` was not edited or staged. `dist/` remains an untracked deliverable directory on main because only the development branch adds its ignore rule. Nothing was pushed or released.
