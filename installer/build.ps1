# Builds XM Reactor for release and packages it: a portable zip and, unless -SkipMsi, the per-user MSI.
# Needs the wix dotnet tool for the MSI (dotnet tool install --global wix); the extensions match whatever
# version is installed, so the version is set in one place: the workflow's install step, or the desk.
# Usage: pwsh installer/build.ps1 [-SkipMsi]  ->  installer/out/XM-Reactor-<version>-portable.zip and
#        installer/out/XM-Reactor-Setup-<version>.msi
param([switch]$SkipMsi)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$out = Join-Path $PSScriptRoot 'out'
$stage = Join-Path $out 'stage'

Push-Location $repo
try {
    # One source of truth for the version and for where cargo puts the exe (CARGO_TARGET_DIR on CI).
    $meta = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
    if ($LASTEXITCODE) { throw 'cargo metadata failed' }
    $version = ($meta.packages | Where-Object name -eq 'xm-reactor').version
    if (-not $version) { throw 'xm-reactor package not found in cargo metadata' }
    $exe = Join-Path $meta.target_directory 'release\XMReactor.exe'

    cargo build --release -p xm-reactor
    if ($LASTEXITCODE) { throw 'cargo build failed' }
    if (-not (Test-Path $exe)) { throw "built exe not found at $exe" }

    # Only the stage is rebuilt; logs or earlier outputs in installer/out stay.
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    Copy-Item $exe $stage
    Copy-Item (Join-Path $repo 'crates\xm-reactor\assets\LICENSES.md') $stage
    # The MSI's Apps & features icon; not installed.
    Copy-Item (Join-Path $repo 'crates\xm-reactor\assets\app.ico') $stage

    $zip = Join-Path $out "XM-Reactor-$version-portable.zip"
    Compress-Archive -Path (Join-Path $stage 'XMReactor.exe'), (Join-Path $stage 'LICENSES.md') -DestinationPath $zip -Force
    '{0} ({1:N1} MB)' -f $zip, ((Get-Item $zip).Length / 1MB)

    if (-not $SkipMsi) {
        $wixVersion = (wix --version) -replace '\+.*$', ''
        if ($LASTEXITCODE -or -not $wixVersion) { throw 'wix is not installed: dotnet tool install --global wix' }
        wix extension add -g "WixToolset.UI.wixext/$wixVersion" "WixToolset.Util.wixext/$wixVersion"
        if ($LASTEXITCODE) { throw 'wix extension add failed' }
        $msi = Join-Path $out "XM-Reactor-Setup-$version.msi"
        wix build (Join-Path $PSScriptRoot 'XMReactor.wxs') -arch x64 `
            -ext "WixToolset.UI.wixext/$wixVersion" -ext "WixToolset.Util.wixext/$wixVersion" `
            -d "Version=$version" -d "StageDir=$stage" -o $msi
        if ($LASTEXITCODE) { throw 'wix build failed' }
        '{0} ({1:N1} MB)' -f $msi, ((Get-Item $msi).Length / 1MB)
    }
}
finally {
    Pop-Location
}
