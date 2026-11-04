# Pre-release readiness

This development branch remains v0.3.3. No v1.0, tag, push, GitHub release, or public distribution has been approved or performed.

## Automated/native checks

- [x] Native Windows workspace build and 21 passing regular tests; baseline was 11 tests. Three opt-in native checks also passed; the stall fixture is intentionally excluded from normal runs.
- [x] Windows formatting and Clippy checks, including all targets.
- [x] Helper timeout/cancellation regression reaps disposable workers.
- [x] Ownership policy covers navigation, overlapping rules, and uncertainty.
- [x] Native agent roundtrip: automatic restoration, manual priority, prior WDA_MONITOR restoration.
- [x] Existing configuration reads with defaulted preferences; default preferences preserve the legacy schema on save. Locked-file save keeps the previous valid configuration.
- [x] Timestamp pacing preserves 60 Hz under jitter and limits high-refresh processing without duplicating frames; empty WinRT frame results are idle while real device/acquisition errors propagate.
- [x] Native D3D11 2560 x 1440 to 1920 x 1080 scaling/readback preserves test colors.
- [x] Windows PowerShell 5.1 installer fixtures: fresh install, corrupt checksum, traversal, missing component, new-install rollback/unrelated-file preservation, byte-for-byte restoration of previous binaries after failed upgrade, and exact startup-command ownership including legacy extended paths.
- [x] NSIS compiles the small `PawHide-setup.exe` bootstrap.
- [x] Windows CI checks/builds/packages both artifacts with release publishing disabled.
- [x] Explicit titleless-taskbar lookup regression (read-only); no taskbar injection performed during validation.

## Runtime acceptance

- [x] Open the native 900 x 580 GUI; inspect real icons, table alignment, sidebar, and original branding.
- [x] Preview both connected 1080p displays, switch sources, and pause/resume.
- [x] Continue capture on Rules and Activity with the persistent status chip.
- [x] Reproduce Floorp failure in a child and convert the null-module panic into an explicit failed-operation result; affinity remains visible.
- [x] Live Explorer private-folder -> parent navigation restores 0x0; a later manual pin survives the same navigation. The unrelated Explorer window remains visible. Temporary rule removed and test window closed afterward.
- [ ] Complete multi-tab ambiguity, overlapping live rules, closure/recreation stress, and ownership across a monitor-process restart on supported Windows versions.
- [ ] Sustain 60 source/processed/preview FPS on a controlled, actively updating source. Initial samples were about 50-56 FPS; this is not a verified 60 FPS claim.
- [x] Cursor preference toggles on/off without restarting capture; original preference restored.
- [x] Actual frameless resize from 900 x 580 to the 800 x 520 minimum; coalesced-input resize regression and opposite-edge anchoring test.
- [ ] Verify DPI at 100/125/150/200%, enlargement/edge cases, focus order, and screen-reader names on supported Windows builds.
- [ ] Repeat the real Discord **entire-monitor** share with fullscreen/borderless Roblox underneath protected PicView and individual Explorer windows. Check that all local windows remain visible, usable, and in normal task switching.
- [ ] Inspect blacked-versus-excluded results in the intended backend. WDA_MONITOR (0x1) and WDA_EXCLUDEFROMCAPTURE (0x11) are distinct; a 0x11 readback alone does not certify stream output.
- [ ] Test monitor unplug/reconnect and display-resolution changes with physical hardware. Preserve source identity; never silently substitute another screen.
- [ ] Test the optional thumbnail and taskbar experiment separately; taskbar injection is never standard startup behavior.

## Installer/publication gates

- [ ] Enable an owner-approved public distribution location and publish matching portable ZIP/checksum assets only when approved. Anonymous downloads of this private repository cannot work.
- [ ] Test real online download, network failure, upgrade, reinstall, shortcuts, Apps registration, locked binaries, rollback failure, startup cleanup, and uninstall in an isolated Windows user account.
- [ ] Verify all three binaries ship together and portable configuration/startup behavior is understood.
- [ ] Confirm the intended license. Cargo already declares MIT but no standalone license exists; see README's MIT/Apache-2.0/GPL-3.0 comparison. Legal terms were not changed.
- [ ] Review dependency notices, original-logo ownership, signing, and release assets.
- [ ] Owner approves final design and runtime acceptance, then separately authorizes any tag/push/public release.

## Known limitations

- Floorp agent loading failed in the tested session. The crash is contained; Floorp exclusion is **not** supported by the current evidence. No security mitigation bypass is attempted.
- Affinity is best effort across capture backends; switcher previews and other system-rendered surfaces may leak independently.
- Automatic monitoring cannot eliminate exposure before discovery/application.
- Tabs share an HWND. Uncertain Explorer locations retain owned exclusion; older unowned exclusions require an explicit manual decision.
- Installer maintenance stops the monitor. Open PawHide afterward to resume saved rules, including after an aborted upgrade.
- An injected old DLL may remain mapped until the target closes. Restart affected apps after an incompatible agent upgrade; never force-unload an in-flight remote call.
- A killed helper can leave an already-started remote operation pending. Verify affinity and capture output after a timeout.
- Document applications need file-open integration; automatic document-close cleanup and HDR tone mapping are absent.
- Public online install and end-to-end Discord behavior are acceptance gates, not inferred from compilation or fixture tests.
