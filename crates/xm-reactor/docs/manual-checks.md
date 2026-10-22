# Manual checks

Results only. Owner's WH-1000XM6, firmware 3.1.5, Windows 11 Pro 26340, 2304 x 1536 at 150 %.

## 2026-10-21 · end to end on the XM6 (builds 73053cf and 496935e)

Driven by UI Automation patterns and the app's own log; no mouse or keyboard input.

| Check | Result |
|---|---|
| Connect | About 5 s to "Connected · On your head"; Platinum Silver render |
| Six tiles | One frame each, echo in 20–150 ms |
| Ambient level 10 / 20, Focus on voice | One frame each, echoed |
| Nine presets | Replies logged: 00 Flat, 20 Gaming, 30 Heavy, 31 Clear, 32 Hard, 33 Soft, A0 Manual, A1 Custom 1, A2 Custom 2; bands differ per preset |
| Custom bands | Three keyboard steps sent one debounced frame; Reset to flat one frame |
| Rename | Persisted across a restart |
| Auto power off (Never, When taken off) | Echoed |
| Capture voice during calls off / on | Echoed |
| Listening mode: Background music + room (Café to Living room), Cinema, Standard | Echoed; equaliser reasons shown and chips disabled in both modes; footer "Background music · Living room"; restored Standard + Café |
| Read-only rows | Touch On, NC/AMB "Noise cancelling ↔ Ambient", voice guidance 0, multipoint On |
| Connected devices | Six listed, one connected; Play here not exercised |
| Quit from the ⋯ menu | Exited in 479 ms |
| Band write while a built-in preset is active | The headset switches itself to Manual (echo 59 04 A0 …) and the built-in keeps its bands, so built-ins stay read-only in the app |
| Take-off / put-on pause and resume | Not verified: the headset was in a Teams call (F5 0B 01, 55 04 01 01 00), so Windows had no media session to pause. Retest with music playing and no call |
| Teams call equaliser reason | Shown: "The headphones pause the equaliser during calls" while the call endpoint was open |
| Low-battery toast (demo) | Not run |

**Open:** intermittent spurious frames. One Speak-to-chat tap was followed 0.5 s later by an unprompted opposite frame; one Flat tap by Gaming, Flat, Gaming within 1 s. Instrumented runs (18 taps in demo, three clean launches on the headset) could not reproduce it; one launch whose connection flapped during connect delivered every chip's callback to its neighbour for the life of the window, which points at the UI framework's event routing. Every observation was through UI Automation; not yet seen with the mouse.

## 2026-10-21 · flyout size and placement (814b0d7)

| Check | Result |
|---|---|
| Placement, bottom taskbar | Right edge 18 px (12 DIP) from the work-area edge, bottom 18 px above the taskbar, whatever the icon's position |
| Height, noise cancelling with the curve | 721 DIP; curve 8 DIP above the footer |
| Height, Ambient with the curve | 832 DIP (level slider and Focus on voice add 111) |
| Height back in noise cancelling | 721 DIP (shrinks) |
| Height before the headset reports bands | 534 DIP, grows when the curve arrives |
| Height disconnected (demo Turn off) | 461 DIP |
| Settings page | Fits its content, capped to the work area (952 DIP here) and scrolls |

## 2026-10-21 · CPU (release build, headset connected, 10 s samples of TotalProcessorTime)

| State | CPU | Working set |
|---|---|---|
| Flyout hidden | 0.31 % of one core | 129 MB |
| Flyout open, idle | 0.00 % | 132 MB |

## 2026-10-22 · listening mode on the main view (95644cd, 09aea6e)

Driven by UI Automation on the XM6 (firmware 3.1.5), app connected while a call was already open.

| Check | Result |
|---|---|
| Background music chip | E8 09 00 02 sent, echoed E9 09 00 02; room row appeared with Café selected |
| Living room chip | E8 09 00 01 sent and echoed |
| Cinema chip | Background music off (E8 09 01 01) then Cinema on (E8 04 00), both echoed |
| Standard chip | Cinema off (E8 04 01), echoed |
| Equaliser while the headphones refuse it with no call notice | Chips disabled with "The headphones have paused the equaliser, as they do during calls." (before 09aea6e they were grey with no caption) |
| Settings page | Headphones, On the headphones, App, About cards; Bluetooth address under About; footer shows only the menu |
| Flyout height | Standard 536, Background music 587, Cinema 549 DIP on the 100 % monitor; right edge 12 DIP from the work area |

Not retested: the listening chips with the mouse; a preset tap while the equaliser was paused (refused by the headphones, as expected).

## 2026-10-22 · installer (installer/XMReactor.wxs, build.ps1 on 3c58f6e)

Built with `pwsh installer/build.ps1` (WiX 5.0.2): zip 1.8 MB, MSI 1.9 MB, no warnings. All installs and uninstalls were run with `msiexec` on this PC with the XM6 paired; dialogs were driven through UI Automation.

- Silent install (`/qn`): exit 0; `%LOCALAPPDATA%\Programs\XM Reactor` holds `XMReactor.exe` and `LICENSES.md` only; Start menu `XM Reactor.lnk` present; Run value `XM Reactor` = the installed exe, quoted. Apps & features entry (under `HKLM\...\Uninstall\{ProductCode}`, which is where Windows Installer registers per-user products) shows XM Reactor 0.1.0 with the product icon registered in the user's Installer cache. Pass.
- Launch from the shortcut: the installed exe started, connected to the XM6 within 4 s, and the tray menu read "Open at sign-in · On" (the same Run value the Settings page reads). The flyout itself was not opened in this check. Pass.
- Install over a running copy (`/qn`, maintenance reinstall and a major upgrade): exit 0 both times, the old process gone (Restart Manager shut it down cleanly, WiX CloseApplication as the fallback), one Apps & features entry. The interactive upgrade with the app running was not exercised; Windows would show its standard "close these applications" prompt first, and the silent run shows the app honours that shutdown. Pass (interactive variant unverified).
- Upgrade keeps data: settings.json hash unchanged across a major upgrade. As ported from XM Connect the Run value came back on every upgrade; fixed by searching the existing value (`EXISTINGRUN`) and leaving the sign-in option off on an upgrade unless it was already there. Verified both ways: value absent before the upgrade stays absent, value present stays present. Pass after fix.
- Interactive uninstall while running (full UI, as from Apps & features): as ported the Restart Manager prompt to close `XMReactor.exe` appeared before the cleanup could run; fixed by sequencing `CleanupAsk` before `InstallValidate`. Now the app quits, "Also delete XM Reactor's settings and logs?" appears from the cleanup process, No keeps `%LOCALAPPDATA%\XM Reactor`, and afterwards the folder, shortcut, Run value, AUMID key and `HKCU\Software\XM Reactor` are gone with no Apps & features entry left. Pass after fix.
- Silent uninstall (`/x ... /qn`) while running: exit 0, no dialog, the quiet cleanup stopped the app, data kept, Run value and files gone. The six-second-old app had not yet written its AUMID key, so that removal is covered by the interactive check only. Pass.
- Runtime missing: no clean VM available. The MSI does not check for the Windows App Runtime; the app's start-up check (`platform::runtime`, `runtime_missing_message` test) is the safeguard. Unverified end to end.
- Review fixes (same day): `msiexec /i ... /qn STARTWITHWINDOWS=0` wrote the Run value on a fresh install (any non-empty value is true in an MSI condition); the component now requires the value "1". Verified: fresh install with 0 leaves no Run value, a default install writes it, an upgrade keeps it present or absent as found. `ARPNOREPAIR` added so Apps & features offers no Repair, which would have rewritten a Run value the user had switched off; the entry now shows NoRepair = 1. Pass.

## 2026-10-22 · settings in three views (no scrolling)

Before: the Settings page at 100 % scale on a 1032 px work area hit the 1008 DIP cap and showed 85 % of itself (about 1180 DIP), disconnected. After, with the stock SelectorBar (Headphones, App, About) and one view at a time, measured by UI Automation on the release build: Headphones disconnected 221 DIP, App 560, About 650, Headphones in demo mode (connected, one source) 767; the scroll pattern reports not scrollable on every view. Residual: at 150 % on a 1920 × 1080 screen the cap is about 664 DIP, so the connected Headphones view would still scroll there. Unverified: real XM6 connected (it was on the phone during the check); demo mode stood in.

## 2026-10-22 · review minors (installer, workflow, version resource)

- `XMReactor.exe` now carries a version resource from Cargo.toml: FileVersion and ProductVersion 0.1.0, product name, description and original file name read back from the built exe. Pass.
- `msiexec /x ... /qb` with the app running: no dialog (the data question now needs reduced or full UI), the quiet cleanup stopped the app, data kept, exit 0. Pass.
- `build.ps1` keeps other files in installer/out (a marker file survived a build) and takes the WiX version from the installed tool (5.0.2). Pass.
- Programs folder removal cannot be observed on this PC (other apps live there); the RemoveFolder row is in the MSI and RemoveFolders ran clean. Unverified.
- Workflow: tag check and the separate release job are exercised by the next CI run; the release job itself only runs on a tag. Unverified until the first tag.

## 2026-10-22 · Open Bluetooth settings

Owner report: the link on the disconnected card did nothing (installed build, HyperlinkButton NavigateUri). Could not reproduce on demand: the XM6 reconnects at once and the demo headset's power off also ends demo mode, so the NotConnected card never stayed up. Eliminated: Launcher.LaunchUriAsync does open ms-settings:bluetooth from an unpackaged process on this PC. Fix: the link now has its own click handler that hands the URI to explorer.exe (as the log folder button does) and logs "opening ms-settings:bluetooth". Verified only that explorer.exe opens the page in-session; the owner confirmed the same evening that the link on a real disconnected card now opens Windows Settings. Pass.

## 2026-10-22 · toggle-frame bug: reactor-level repro (tools/reactor-probe)

Hypothesis under test: windows-reactor (rev 0a5ced4) routes a chip's checked-changed event to a neighbour's callback after the tree changes shape. Reading the crate: callbacks with captures compare by pointer, so they are re-sent on every render; its IsChecked listener is a dependency-property callback filtered by a per-object feedback expectation; nested wakes are guarded. The probe mirrors the main view and drives it by UI Automation through 11 scenarios: steady taps; one flap; four flaps 50 ms apart; first preset removed; presets rotated; flap with the list changed; tiles after a flap; a storm (Disconnected, Connecting, Disconnected, Ready about 1 ms apart from a background thread through the same completion handle the engine uses); three storms back to back; a tap during a storm; a storm from Disconnected. Result: 0 mismatches, 0 unprompted callbacks across 40 taps, and reactor's own deselect writes never surfaced as callbacks. The crossing seen once on 2026-10-21 is not reproduced at the framework level. Side note: Reconnect is sent only by the Try again button, and that session was launched from a terminal with --open, so the "reconnect requested with nobody clicking" was most likely the Enter key-up landing on the freshly focused button, not a misrouted event. Status: parked again, with the debug lines from d2407f9 in place; a future occurrence's log pairs the tapped name with the callback's preset.
- Owner report the same evening: with Bluetooth off the card still showed the last track under the Open Bluetooth settings link. Cause: the view model passed now_playing through regardless of session, while the battery row already had the stale-after-disconnect guard. Fixed in the view model (now_playing only while connected), unit test covers BluetoothOff, NotConnected and Disconnected. Verified by test; the owner's build was updated. Pass.
- Owner report after the relaunch: no track shown until the next track started. Cause: the app only decoded the A9 01 track notification and never asked for the current track at connect. Probe on the XM6 (`xm-probe raw A6 01`): the headset answers A7 01 with title, album and artist in the notification's layout ("Undercover Martyn", "Tourist History", "Two Door Cinema Club" captured). Fixed: get_now_playing (A6 01) joins the connect and 15 s refresh reads, A7 01 decodes like A9 01; decoder test uses the captured frame, session tests updated (14 refresh reads). Verified by the probe reply and 221 workspace tests; the owner confirmed the line appears on a track change.
