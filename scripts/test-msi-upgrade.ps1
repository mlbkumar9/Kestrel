<#
Upgrade test for the Windows installer. Installs an old Kestrel MSI (for example the published
4.0.0 one), writes user settings and a user preset in the 4.0 format, installs the new MSI over
it and checks that exactly one Kestrel remains, at the expected version, with the user data intact.

It really installs and upgrades Kestrel: run it on a disposable machine (the CI windows-latest
runner), never on a machine that has Kestrel installed. It needs an elevated shell.

  ./scripts/test-msi-upgrade.ps1 -OldMsi old.msi -NewMsi new.msi -ExpectedVersion 4.0.2
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$OldMsi,
    [Parameter(Mandatory = $true)][string]$NewMsi,
    [Parameter(Mandatory = $true)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$ExpectedVersion,
    # msiexec verbose logs are written here; CI uploads the folder when the test fails.
    [string]$LogDirectory = 'msi-upgrade-logs'
)

$ErrorActionPreference = 'Stop'

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script from an elevated shell.'
}
foreach ($msi in $OldMsi, $NewMsi) {
    if (-not (Test-Path -LiteralPath $msi -PathType Leaf)) { throw "MSI not found: $msi" }
}
New-Item -ItemType Directory -Force -Path $LogDirectory | Out-Null
$LogDirectory = (Resolve-Path -LiteralPath $LogDirectory).Path

# Marker values the upgrade must leave alone.
$markerPresetName = 'UPGRADE_MARKER_PRESET_4_0'
$markerDestination = 'C:/UpgradeMarkerDestination'

$uninstallRoots = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKCU:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)

function Get-KestrelEntry {
    foreach ($root in $uninstallRoots) {
        if (-not (Test-Path -LiteralPath $root)) { continue }
        foreach ($key in Get-ChildItem -LiteralPath $root) {
            if ($key.GetValue('DisplayName') -eq 'Kestrel') {
                [pscustomobject]@{ Key = $key.Name; Version = $key.GetValue('DisplayVersion') }
            }
        }
    }
}

function Invoke-Msiexec {
    param([string]$Msi, [string]$LogName)
    $log = Join-Path $LogDirectory $LogName
    $arguments = @('/i', "`"$((Resolve-Path -LiteralPath $Msi).Path)`"", '/qn', '/norestart', '/l*v', "`"$log`"")
    Write-Host "msiexec $($arguments -join ' ')"
    $process = Start-Process -FilePath 'msiexec.exe' -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "msiexec exited with $($process.ExitCode) for $Msi (log: $log)"
    }
}

function Assert-That {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw "FAILED: $Message" }
    Write-Host "ok: $Message"
}

# Settings live in <config dir>/kestrel/gpui.ron (crates/kestrel-gpui/src/state/storage.rs);
# on Windows the config dir is %APPDATA%. User presets are stored in the same file.
$settingsPath = Join-Path $env:APPDATA 'kestrel\gpui.ron'
$installDirectory = Join-Path $env:ProgramFiles 'Kestrel'
$shortcutPath = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Kestrel\Kestrel.lnk'

Assert-That (@(Get-KestrelEntry).Count -eq 0) 'no Kestrel is installed before the test'
Assert-That (-not (Test-Path -LiteralPath $settingsPath)) 'no settings file exists before the test'

# 1. Install the old MSI.
Invoke-Msiexec -Msi $OldMsi -LogName 'install-old.log'
Assert-That (Test-Path -LiteralPath (Join-Path $installDirectory 'Kestrel.exe')) 'old MSI installed Kestrel.exe'
$oldEntries = @(Get-KestrelEntry)
Assert-That ($oldEntries.Count -eq 1) "old MSI registered one Kestrel entry (version $($oldEntries[0].Version))"

# 2. Write 4.0-format settings and a user preset: a RON map of key to RON text.
$preferences = '(light_theme:true,output_directory:"' + $markerDestination + '",audio_languages:"fra, jpn",' +
    'subtitle_languages:"spa",remux_engine:Ffmpeg,selected_preset_id:"user-upgrade-marker",' +
    'current_settings:(container:mkv),' +
    'user_presets:[(id:"user-upgrade-marker",name:"' + $markerPresetName + '",description:"Upgrade test",' +
    'builtin:false,settings:(container:mkv,video:encode((encoder:x264,rate:quality(20)))))],' +
    'existing:overwrite,output_target:Some(folder("' + $markerDestination + '")),' +
    'legacy_banner_dismissed:true,output_target_explicit:true)'
$escaped = $preferences.Replace('\', '\\').Replace('"', '\"')
$record = '{"kestrel.preferences.v3":"' + $escaped + '"}'
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $settingsPath) | Out-Null
[System.IO.File]::WriteAllText($settingsPath, $record, (New-Object System.Text.UTF8Encoding($false)))
$settingsBefore = (Get-FileHash -LiteralPath $settingsPath -Algorithm SHA256).Hash
Write-Host "wrote $settingsPath"

# 3. Upgrade with the new MSI.
Invoke-Msiexec -Msi $NewMsi -LogName 'install-new.log'

# 4. Assertions.
$entries = @(Get-KestrelEntry)
$entries | ForEach-Object { Write-Host "uninstall entry: $($_.Key) $($_.Version)" }
Assert-That ($entries.Count -eq 1) 'exactly one Kestrel entry in Programs and Features'
Assert-That ($entries[0].Version -eq $ExpectedVersion) "Programs and Features shows version $ExpectedVersion"

$exe = Join-Path $installDirectory 'Kestrel.exe'
Assert-That (Test-Path -LiteralPath $exe) 'Kestrel.exe is installed'
$info = (Get-Item -LiteralPath $exe).VersionInfo
$fileVersion = ($info.FileVersion -replace '(\.0)+$', '')
$productVersion = ($info.ProductVersion -replace '(\.0)+$', '')
Assert-That ($fileVersion -eq $ExpectedVersion) "Kestrel.exe FileVersion is $ExpectedVersion (found $($info.FileVersion))"
Assert-That ($productVersion -eq $ExpectedVersion) "Kestrel.exe ProductVersion is $ExpectedVersion (found $($info.ProductVersion))"

Assert-That (Test-Path -LiteralPath $shortcutPath) 'Start menu shortcut exists'
$shell = New-Object -ComObject WScript.Shell
$target = $shell.CreateShortcut($shortcutPath).TargetPath
Assert-That (-not [string]::IsNullOrEmpty($target)) 'Start menu shortcut has a target'
Assert-That ($target -ieq $exe) "Start menu shortcut targets the installed exe (found $target)"

Assert-That (Test-Path -LiteralPath $settingsPath) 'settings file still exists'
$text = Get-Content -LiteralPath $settingsPath -Raw
Assert-That ($text.Contains($markerDestination)) 'settings marker value is intact'
Assert-That ($text.Contains($markerPresetName)) 'user preset marker is intact'
Assert-That ((Get-FileHash -LiteralPath $settingsPath -Algorithm SHA256).Hash -eq $settingsBefore) 'settings file is byte-identical'

foreach ($relative in 'tools\ffmpeg\bin\ffmpeg.exe', 'tools\mkvtoolnix\mkvmerge.exe') {
    Assert-That (Test-Path -LiteralPath (Join-Path $installDirectory $relative)) "bundled $relative exists"
}

Write-Host "MSI upgrade test passed (old to $ExpectedVersion)."
