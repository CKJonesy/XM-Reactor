# reactor-probe

A stand-alone windows-reactor app shaped like XM Reactor's main view (recovery buttons, tiles,
listening-mode row, preset chips with capturing callbacks), driven by UI Automation to look for the
spurious toggle frames seen on 2026-10-21. Build with the app's target directory so the reactor
dependency is reused, then run the driver:

    cd tools/reactor-probe
    $env:CARGO_TARGET_DIR = "$env:LOCALAPPDATA\xm-reactor\target"; cargo build
    pwsh drive.ps1 "$env:LOCALAPPDATA\xm-reactor\target\debug\reactor-probe.exe" probe.log

Each tap logs the chip tapped and the chip the callback was built for; the driver reports any
mismatch or unprompted callback. Not part of the workspace.
