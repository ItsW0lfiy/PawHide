# PawHide — CLI-first Windows capture privacy

PawHide is a native Rust command-line utility that excludes selected application windows from compatible screen captures while keeping them visible and usable on your physical desktop. It has **no graphical frontend**: the CLI is the single user interface, and AutoHide runs as an independent background monitor.

**Important:** A Windows capture-affinity readback of 0x11 is not proof of what Discord, other capture clients, app switchers or system previews actually show. Always verify the *real receiving stream* before exposing private content. See [capture boundaries](docs/CAPTURE_BOUNDARY.md).

## Install or build

Requires Windows 10 version 2004+ or Windows 11 (x64). Rust builds require the repository's nightly toolchain and Windows SDK. From this checkout:

~~~powershell
cargo build --release --locked
cargo all   # run checks, tests, packaging, and NSIS installer fixtures
~~~

**The two binaries that must ship together** are `pawhide.exe` and `pawhide_agent.dll` in the same folder. There is no `PawHideUI.exe` and no `cargo app` alias. The CLI executable works directly from PowerShell or Command Prompt. The portable ZIP and online-installer bootstrap are produced in `dist/` by `cargo all` after CI/installer tests succeed.

The online installer downloads the latest GitHub release ZIP and verifies its SHA-256 checksum. A GUI-era release is deliberately rejected by the new CLI installer; publish a matching CLI-only ZIP and checksum **only after testing and separately approving the release**. The old published release remains unchanged.

The installer preserves `%LOCALAPPDATA%\PawHide\config.json`, logs, and saved rules. When upgrading a GUI-era installation it backs up and removes the obsolete GUI executable only after replacing the matching CLI and agent; a failed upgrade restores the older binaries.

## Discover commands and their optional arguments

~~~powershell
pawhide --help
pawhide list --help
pawhide hide --help
pawhide auto --help
pawhide auto add --help
pawhide logs show --help
~~~

**Help syntax:** square brackets denote optional values, angle brackets denote required arguments. Flags like `--verbose` are optional switches. A value following a flag, such as `--search TEXT`, is required **only when that flag is used**. Options can usually appear before or after positional arguments.

### Find and protect windows

~~~powershell
pawhide list                                  # Group applications
pawhide list --windows                        # Individual window titles
pawhide list --search PicView                 # Search exe names or titles
pawhide list --filter excluded                # Only capture-excluded windows
pawhide list --filter attention --json        # Unknown/unsupported windows for scripts
pawhide list PicView.exe --windows            # Narrow to one executable
pawhide hide PicView.exe                      # Protect all eligible matching windows
pawhide hide PicView.exe --title "Photo"      # Match a title substring
pawhide hide --pick                           # Numbered interactive window picker
pawhide show PicView.exe                      # Restore normal capture visibility
pawhide status PicView.exe --verbose          # Inspect HWND/PID/affinity
pawhide inspect PicView.exe --json            # Raw window details for scripting
~~~

| Optional argument | Commands | Meaning |
|---|---|---|
| `APPLICATION` | `list`, `status`, `hide`, `show` | Target executable; `hide`/`show` need a target unless using `--pick` |
| `--windows` | `list` | Print each window instead of grouping by application |
| `--all` | `list` | Include tool/background windows; **does not** bypass protection safety rules |
| `--search TEXT` | `list` | Case-insensitive substring match against executable and window title |
| `--filter STATE` | `list` | `all` (default), `excluded`, `visible`, or `attention` |
| `--json` | `list`, `status`, `inspect`, `doctor` | Structured output for tools/scripts |
| `--title TEXT` | `hide`, `show` | Match window title substring; requires a target |
| `--pick` | `hide`, `show` | Numbered terminal picker for one window |
| `--verbose` / `-v` | `hide`, `show`, `status` | Show window IDs, owners, and technical details |
| `--allow-shell` | `hide`, `show` | Advanced permission for an explicitly targeted Explorer/shell window; risky targets remain restricted |

`--filter attention` matches unsupported, unknown, or non-standard affinity readings; it does not mean a failed protection attempt was detected. Discord and protected/anti-tamper game processes are not injected.

### AutoHide rules and Windows sign-in

~~~powershell
pawhide auto add PicView.exe                  # Save rule; start monitor if needed
pawhide auto list                             # Saved executable rules
pawhide auto status                           # Monitor state and coverage
pawhide auto sync                             # Apply current rules once
pawhide auto restart                          # Controlled stop/start, no force-kill
pawhide auto stop                             # Stop monitoring, keep existing exclusions
pawhide auto remove PicView.exe               # Remove an automatic rule
pawhide startup enable                        # AutoHide at user login (per-user registry)
pawhide startup status
pawhide startup disable
~~~

Restart requires saved rules. If the monitor fails to stop, the restart stops with an error rather than starting a second instance. `auto stop` does **not** automatically unprotect windows that are already excluded.

### Protected folders, logs and diagnostics

~~~powershell
pawhide folders add "D:\Private"              # Directory rule
pawhide folders list
pawhide folders match "D:\Private\notes.md"   # Check rule coverage
pawhide folders remove "D:\Private"
pawhide config path
pawhide config show
pawhide config validate
pawhide config edit                           # Open config in Notepad
pawhide doctor                                # Read-only installation diagnostics
pawhide doctor --json                         # Diagnostics for scripts
pawhide logs show --lines 60                  # Default 30; maximum 5000
pawhide logs follow                           # Watch new monitor log entries
pawhide watch --interval 2                    # Refresh terminal summary (Ctrl+C)
pawhide capture taskbar status                # Experimental taskbar affinity
~~~

Folder automation uses verified Explorer folder paths; integration with a cooperating editor such as Wolfmark can use `pawhide folders notify 0xHWND FILE`. These rules do not magically identify every file opened in an arbitrary editor.

`doctor` checks installation companions, configuration, monitor status, and window enumeration without injecting into other processes. `watch` displays a live terminal summary, not a screen recording or a real Discord capture preview. The old 60 FPS GUI preview, icons, thumbnails and visual-only preferences are retired.

## Troubleshooting and release safety

If a window fails to hide, first try `pawhide doctor`, then `pawhide hide PicView.exe --verbose` and `pawhide logs show --lines 60`. Agent injection into certain protected applications can fail for legitimate platform/security reasons; PawHide reports failures rather than attempting to bypass protection.

Stop AutoHide before upgrading in-use binaries with `pawhide auto stop`. The installer checks file locks and preserves existing configuration; it does not terminate user applications or forcibly unload injected code. If an older agent DLL remains mapped inside a protected application, close and reopen that application after an upgrade.

For a controlled live-Windows affinity regression, see `tools/Test-Protection.ps1`. A verified Windows API return status alone is **not** a privacy guarantee.

The repository remains a **development build**. A successful CI build or ZIP generation does not authorize a public release. Confirm distribution rights, the intended license and actual Discord capture behavior separately. Cargo metadata says MIT, but the repository's standalone license text still needs owner approval.
