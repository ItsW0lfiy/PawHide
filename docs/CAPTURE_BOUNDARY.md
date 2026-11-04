# PawHide: capture boundary and acceptance tests

## Purpose (authoritative)

PawHide changes **only what compatible capture consumers see**. It must not
minimize, reposition, occlude, conceal, resize, reparent or remove a window
locally, change local Alt+Tab/Win+Tab membership, alter local taskbar settings,
or inject into Discord to rewrite its private video stream.

Normal local display stays untouched. The user can have Roblox filling a
monitor, **Explorer and PicView both protected and still locally visible above
Roblox**, with unprotected PawHide visible above them. Compatible monitor
capture should show Roblox and the unprotected PawHide, not the two protected
windows. Capture affinity applies to all compatible capture programs, not
just Discord.

## Capture mechanisms and limitations

- `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` is a per-window DWM
  capture-affinity property. It works only in capture paths that honor it;
  successful `GetWindowDisplayAffinity` readback is *not* proof of Discord
  capture exclusion.
- Alt+Tab and Win+Tab Task View are separate Windows UI. They can draw
  protected app titles, icons and thumbnails into the capture independently
  of the original top-level window. Do **not** use `WS_EX_TOOLWINDOW`,
  `ShowWindow`, `SetWindowPos`, taskbar auto-hide or Explorer restart to
  change the user's desktop as a workaround.
- Discord can switch between multiple screen/game capture backends.
  Validate the specific **share entire display** path, not only ShareX or
  a gdigrab/BitBlt test.
- Viewport-only manipulation of the Task View output is **not implemented**.
  A separate app-owned sanitized stream mirror would require a different
  explicitly selected share source, separate design and testing; it is not
  transparent editing of Discord's existing monitor-share output.
- A capture-only taskbar experiment exists via `pawhide capture taskbar`
  `status|hide|show`. Only verified Explorer taskbar HWNDs are targeted;
  no local visibility or desktop-style change is made. Because this injects
  into Explorer it is opt-in, never automatic until tested, and could fail
  or destabilize the shell.
- Roblox anti-tamper targets and Discord capture clients are blocked before injection. Unknown process identity is rejected. Floorp loader failure is reported as failed affinity, not protection.

## Ownership and preview interpretation

Manual and automatic affinity use separate live-HWND properties. Automatic cleanup restores only a recorded prior affinity and preserves a later manual exclusion. Incomplete Explorer locations or unaccounted tabs do not authorize restoration. HWND destruction removes those properties; the monitor can recover ownership after restarting without guessing from window titles.

`Excluded` means a verified 0x11 affinity. `Blacked / WDA_MONITOR` means 0x1; Microsoft documents black/empty output for that older mode and that pre-2004 Windows treats 0x11 as 0x1. Even 0x11 can behave differently across capture backends. Inspect a real preview and the intended stream; no software readback alone certifies what a viewer sees.

The native preview is a separate WGC monitor capture. Its source, processed, and visible-preview frame rates are measured separately and do not certify Discord's backend. It does not record, transmit, modify another application's capture, or inject into a capture client.

## Regression acceptance scenarios

1. **Local preservation**: Protect PicView and an Explorer folder window;
   locally both stay visible and usable on top of fullscreen/borderless Roblox.
   Unprotected PawHide stays visible locally and on the capture.
2. **Screen capture**: Verify with a second viewer of Discord's *full monitor*
   share. The protected windows themselves are absent, and content underneath
   them remains visible. If not, report unprotected rather than success.
3. **Switchers**: Press Alt+Tab and Win+Tab. Local UI must remain fully normal.
   If the viewer still sees sensitive names/previews in switchers, record as
   **known capture leak**, not a successful implementation.
4. **Taskbar experiment**: With no sensitive windows active, run
   `pawhide capture taskbar status`, then `hide`, inspect the actual monitor
   stream, and use `show` to restore capture. Test both success and failure.
   On multi-monitor setups inspect each taskbar separately.
5. **Stability**: The GUI remains responsive on timeout. Window handles are
   revalidated before injection. No blanket explorer.exe process action is
   permitted.
6. **Auto rules**: Newly opened protected documents must not be reported
   hidden until verified in the chosen capture path. Explorer tabs may share
   a single HWND; PawHide cannot hide one tab while leaving other tabs in
   the same HWND exposed.

## Sources

- Microsoft SetWindowDisplayAffinity: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity
- Microsoft GetWindowDisplayAffinity: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowdisplayaffinity
- Microsoft Task switching: https://support.microsoft.com/en-us/windows/experience/how-to-multitask-in-windows
- Discord capture backends: https://support.discord.com/hc/en-us/articles/9410427556375--Windows-Capturing-Application-Window-for-Screen-Share-and-Go-Live
