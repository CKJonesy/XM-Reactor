# Drives reactor-probe.exe through UI Automation and reports every tap whose callback named a
# different chip. Usage: pwsh drive.ps1 <probe.exe> <log path>
param([string]$Exe, [string]$Log)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Remove-Item $Log -ErrorAction SilentlyContinue
$env:PROBE_LOG = $Log
$proc = Start-Process $Exe -PassThru
Start-Sleep 4
$root = [System.Windows.Automation.AutomationElement]::RootElement
$byPid = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $proc.Id)
$win = $null
for ($i = 0; $i -lt 20 -and -not $win; $i++) { Start-Sleep -Milliseconds 500; $win = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $byPid) }
if (-not $win) { throw 'probe window not found' }
"window: '" + $win.Current.Name + "' " + $win.Current.BoundingRectangle

function Mark($text) { Add-Content -Path $Log -Value ([string][DateTimeOffset]::Now.ToUnixTimeMilliseconds() + ' ' + $text) }
function Find($name) {
    $el = $null
    for ($i = 0; $i -lt 10 -and -not $el; $i++) {
        $el = $win.FindFirst([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $name)))
        if (-not $el) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $el) { throw "element '$name' not found" }
    $el
}
function Tap($name) {
    $el = Find $name
    $state = $el.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState
    Mark "TAP $name (was $state)"
    $el.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Start-Sleep -Milliseconds 700
}
function Click($name, $wait = 700) {
    $el = Find $name
    Mark "CLICK $name"
    $el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Start-Sleep -Milliseconds $wait
}

Mark "SCENARIO S1 steady"
Tap 'Gaming'; Tap 'Heavy'
Mark "SCENARIO S2 one flap"
Click 'Flap' 1200; Click 'Flap' 1200
Tap 'Flat'; Tap 'Gaming'
Mark "SCENARIO S3 rapid double flap"
Click 'Flap' 50; Click 'Flap' 50; Click 'Flap' 50; Click 'Flap' 1500
Tap 'Heavy'; Tap 'Clear'
Mark "SCENARIO S4 drop first preset"
Click 'DropFirst' 1200
Tap 'Gaming'; Tap 'Heavy'
Mark "SCENARIO S5 rotate"
Click 'Rotate' 1200
Tap 'Clear'; Tap 'Hard'
Mark "SCENARIO S6 flap while shape changed"
Click 'Flap' 1200; Click 'Flap' 1200
Tap 'Soft'; Tap 'Heavy'
Mark "SCENARIO S7 tiles after flap"
Tap 'Speak'; Tap 'DSEE'; Tap 'Speak'
Mark "SCENARIO S8 storm (Disconnected, Connecting, Disconnected, Ready 1 ms apart)"
Click 'Storm' 2500
Tap 'Heavy'; Tap 'Clear'; Tap 'Hard'; Tap 'Cinema'; Tap 'Standard'
Mark "SCENARIO S9 three storms back to back"
Click 'Storm' 30; Click 'Storm' 30; Click 'Storm' 2500
Tap 'Soft'; Tap 'Gaming'; Tap 'Heavy'; Tap 'Speak'
Mark "SCENARIO S10 tap during a storm"
Click 'Storm' 0
try { Tap 'Clear' } catch { Mark "tap during storm failed: $_" }
Start-Sleep -Milliseconds 2000
Tap 'Hard'; Tap 'Clear'
Mark "SCENARIO S11 storm from Disconnected"
Click 'Flap' 1200
Click 'Storm' 2500
Tap 'Soft'; Tap 'Gaming'; Tap 'Background music'
Mark "END"
Stop-Process -Id $proc.Id -Force -Confirm:$false

"--- log:"
Get-Content $Log | ForEach-Object { $_ -replace '^\d+ ', '' }
"--- analysis:"
$tapped = $null; $bad = 0
foreach ($line in (Get-Content $Log | ForEach-Object { $_ -replace '^\d+ ', '' })) {
    if ($line -match '^TAP (\S+)') { $tapped = $Matches[1]; continue }
    if ($line -match '^(CLICK|SCENARIO|END)') { $tapped = $null; continue }
    if ($line -match '^(chip|tile|mode)-callback (\S+) (.*)$') {
        $name = $Matches[2]
        if ($tapped -and $name -ne $tapped) { "MISMATCH: tapped $tapped, callback $name $($Matches[3])"; $bad++ }
        if (-not $tapped) { "UNPROMPTED: callback $name $($Matches[3])"; $bad++ }
    }
}
"mismatches or unprompted callbacks: $bad"
