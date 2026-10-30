param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryPath,
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"

if ($Tag -notmatch '^[A-Za-z0-9._-]+$') {
    throw "Tag must contain only letters, numbers, dots, underscores, or hyphens."
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$binary = (Resolve-Path -LiteralPath $BinaryPath).Path
$output = [System.IO.Path]::GetFullPath($OutputDirectory)
$ffmpegVersion = "n9.0.2-37-gae19868753"
$ffmpegUrl = "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-29-19-17/ffmpeg-n9.0.2-37-gae19868753-win64-gpl-9.0.zip"
$mkvtoolnixVersion = "99.0"
$mkvtoolnixUrl = "https://mkvtoolnix.download/windows/releases/99.0/mkvtoolnix-64-bit-99.0.7z"
$work = Join-Path ([System.IO.Path]::GetTempPath()) "langmux-package-$([guid]::NewGuid().ToString('N'))"
$stage = Join-Path $work "stage"
$ffmpegExtract = Join-Path $work "ffmpeg"
$mkvtoolnixExtract = Join-Path $work "mkvtoolnix"
$ffmpegArchive = Join-Path $work "ffmpeg.zip"
$mkvtoolnixArchive = Join-Path $work "mkvtoolnix.7z"

try {
    New-Item -ItemType Directory -Path $stage, $ffmpegExtract, $mkvtoolnixExtract, $output -Force | Out-Null

    Invoke-WebRequest -Uri $ffmpegUrl -OutFile $ffmpegArchive
    Expand-Archive -LiteralPath $ffmpegArchive -DestinationPath $ffmpegExtract -Force
    $ffmpegExe = Get-ChildItem -LiteralPath $ffmpegExtract -Filter "ffmpeg.exe" -File -Recurse | Select-Object -First 1
    $ffprobeExe = Get-ChildItem -LiteralPath $ffmpegExtract -Filter "ffprobe.exe" -File -Recurse | Select-Object -First 1
    if (-not $ffmpegExe -or -not $ffprobeExe) {
        throw "FFmpeg $ffmpegVersion archive is missing ffmpeg.exe or ffprobe.exe."
    }

    $ffmpegRoot = $ffmpegExe.Directory.Parent.FullName
    $ffmpegLicense = Get-ChildItem -LiteralPath $ffmpegRoot -File -Recurse |
        Where-Object { $_.Name -match '^LICENSE(?:\..*)?$' } |
        Select-Object -First 1
    if (-not $ffmpegLicense) {
        throw "FFmpeg $ffmpegVersion archive does not contain a license file."
    }

    $sevenZip = (Get-Command "7z.exe" -ErrorAction SilentlyContinue).Source
    if (-not $sevenZip) {
        $sevenZipCandidates = @(
            (Join-Path $env:ProgramFiles "7-Zip/7z.exe"),
            (Join-Path ${env:ProgramFiles(x86)} "7-Zip/7z.exe")
        )
        $sevenZip = $sevenZipCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    }
    if (-not $sevenZip) {
        throw "7-Zip is required to extract the MKVToolNix portable archive."
    }

    Invoke-WebRequest -Uri $mkvtoolnixUrl -OutFile $mkvtoolnixArchive
    & $sevenZip x $mkvtoolnixArchive "-o$mkvtoolnixExtract" -y | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "7-Zip failed to extract MKVToolNix $mkvtoolnixVersion."
    }
    $mkvmergeExe = Get-ChildItem -LiteralPath $mkvtoolnixExtract -Filter "mkvmerge.exe" -File -Recurse | Select-Object -First 1
    $mkvtoolnixLicense = Get-ChildItem -LiteralPath $mkvtoolnixExtract -Filter "COPYING" -File -Recurse | Select-Object -First 1
    if (-not $mkvmergeExe -or -not $mkvtoolnixLicense) {
        throw "MKVToolNix $mkvtoolnixVersion archive is missing mkvmerge.exe or COPYING."
    }

    $ffmpegBin = Join-Path $stage "tools/ffmpeg/bin"
    $mkvtoolnixBin = Join-Path $stage "tools/mkvtoolnix"
    New-Item -ItemType Directory -Path $ffmpegBin, $mkvtoolnixBin -Force | Out-Null
    Copy-Item -LiteralPath $ffmpegExe.FullName -Destination $ffmpegBin
    Copy-Item -LiteralPath $ffprobeExe.FullName -Destination $ffmpegBin
    Copy-Item -LiteralPath $ffmpegLicense.FullName -Destination (Join-Path (Split-Path -Parent $ffmpegBin) "LICENSE")
    Copy-Item -LiteralPath $mkvmergeExe.FullName -Destination $mkvtoolnixBin
    Copy-Item -LiteralPath $mkvtoolnixLicense.FullName -Destination $mkvtoolnixBin
    Get-ChildItem -LiteralPath $mkvmergeExe.Directory.FullName -Filter "*.dll" -File |
        Copy-Item -Destination $mkvtoolnixBin

    Copy-Item -LiteralPath $binary -Destination (Join-Path $stage "langmux.exe")
    Copy-Item -LiteralPath (Join-Path $repoRoot "README.md") -Destination $stage
    Copy-Item -LiteralPath (Join-Path $repoRoot "LICENSE") -Destination $stage
    Copy-Item -LiteralPath (Join-Path $repoRoot "THIRD_PARTY_NOTICES.md") -Destination $stage

    $archive = Join-Path $output "LangMux-$Tag-windows-x64.zip"
    if (Test-Path -LiteralPath $archive) {
        Remove-Item -LiteralPath $archive -Force
    }
    Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $archive -CompressionLevel Optimal
    Write-Output "Packaged LangMux $Tag with FFmpeg $ffmpegVersion and MKVToolNix $mkvtoolnixVersion: $archive"
}
finally {
    if (Test-Path -LiteralPath $work) {
        Remove-Item -LiteralPath $work -Recurse -Force
    }
}
