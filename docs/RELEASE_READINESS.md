# PawHide — CLI-only release readiness

This is a development-only migration. No v1.0 tag or public release is implied by passing CI.

## Automated checks (must pass for this revision)

- [ ] `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] Windows release build and complete Rust workspace tests
- [ ] `pawhide --help`, `pawhide list --help`, `pawhide auto --help`, `pawhide doctor --help`
- [ ] AutoHide restart stop/start semantics and lock/event lifecycle on interactive Windows
- [ ] CLI-only portable ZIP contains `pawhide.exe`, `pawhide_agent.dll`, README and docs, **not** the GUI
- [ ] Installer fixtures: hash checks, ZIP traversal, missing agent, new-install rollback, CLI-only upgrade, legacy GUI retirement and failure rollback
- [ ] NSIS online bootstrap builds without publishing or modifying any release

## Real-world acceptance (required before publication)

- [ ] `hide` / `show` for supported programs such as PicView; capture affinity verified by PID/HWND after applying
- [ ] AutoHide new windows, rule removal, monitor restart, stop-after-bounded-injection and stale window handles
- [ ] Explorer folder ownership/navigations, multiple tabs, and overlap with manual exclusions
- [ ] Monitor remains responsive through target exit/hang/relaunch
- [ ] Full monitor sharing in Discord: excluded windows truly absent, local windows still visible/usable, task switching and previews checked
- [ ] Windows sign-in startup, portable use, installer upgrade from GUI-era release, uninstall and old GUI file removal
- [ ] Interactive `watch`, output accuracy for `--filter`/`--search`, `doctor --json`, and exit codes tested

## Known limitations

- Windows capture-affinity state does **not** certify stream output; protected clients and anti-tamper targets are not injected.
- AutoHide cannot guarantee zero-frame exposure between opening and rule application.
- Some applications (previously Floorp) may reject agent loading; do not assume protection.
- Tabs within one HWND cannot always be distinguished, and some system previews may show sensitive metadata.
- `auto stop` leaves existing exclusions in place until explicitly shown or window closure.
- After an upgrade, previously injected DLLs may remain mapped until their owning process exits; do not force-unload.
- The online installer requires a **matching, owner-approved CLI-only public release**, never the previous GUI ZIP.
- Cargo metadata declares MIT but standalone license terms have not yet been approved.
