# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
#
# One-shot installer for ProjectMind on Windows. Downloads the pre-built
# bundle for the latest release matching the host architecture and drops
# the desktop app + MCP server in standard locations.
#
# Usage (PowerShell, any privilege level):
#   iwr -useb https://raw.githubusercontent.com/Plaintext-Gmbh/projectmind/master/scripts/install.ps1 | iex
#
# Environment overrides (set before piping into iex):
#   $env:PM_VERSION = "v1.2.3"  # pin a specific tag (default: latest)
#   $env:PM_NO_APP  = "1"       # skip the desktop app
#   $env:PM_NO_MCP  = "1"       # skip the MCP server

$ErrorActionPreference = 'Stop'

$Repo    = 'Plaintext-Gmbh/projectmind'
$Version = if ($env:PM_VERSION) { $env:PM_VERSION } else { 'latest' }

function Info($msg) { Write-Host "::" -ForegroundColor Cyan -NoNewline; Write-Host " $msg" }
function Warn($msg) { Write-Host "!!" -ForegroundColor Yellow -NoNewline; Write-Host " $msg" }
function Fail($msg) { Write-Host "xx" -ForegroundColor Red -NoNewline; Write-Host " $msg"; exit 1 }

# ---- detect arch -----------------------------------------------------------
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne 'AMD64' -and $arch -ne 'x86_64') {
    Fail "unsupported Windows arch: $arch (only x86_64 builds are published)"
}
$AppSuffix = 'windows-x86_64'
$McpSuffix = 'windows-x86_64'

# ---- pick install destinations --------------------------------------------
$LocalAppData = $env:LOCALAPPDATA
if (-not $LocalAppData) { $LocalAppData = Join-Path $env:USERPROFILE 'AppData\Local' }
$AppDest = Join-Path $LocalAppData 'Programs\ProjectMind'
$BinDest = Join-Path $LocalAppData 'Programs\ProjectMind\bin'

New-Item -ItemType Directory -Force -Path $AppDest | Out-Null
New-Item -ItemType Directory -Force -Path $BinDest | Out-Null

# ---- resolve version ------------------------------------------------------
$ReleaseApi = "https://api.github.com/repos/$Repo/releases"
if ($Version -eq 'latest') {
    Info "resolving latest release tag"
    $latest = Invoke-RestMethod -UseBasicParsing -Uri "$ReleaseApi/latest"
    $Tag = $latest.tag_name
    if (-not $Tag) { Fail "could not parse latest release tag from GitHub API" }
} else {
    $Tag = $Version
}
Info "version: $Tag"

$DownloadBase = "https://github.com/$Repo/releases/download/$Tag"
$Tmp = Join-Path $env:TEMP "projectmind-install-$([guid]::NewGuid())"
New-Item -ItemType Directory -Force -Path $Tmp | Out-Null

try {
    function Download($asset) {
        Info "downloading $asset"
        Invoke-WebRequest -UseBasicParsing -Uri "$DownloadBase/$asset" -OutFile (Join-Path $Tmp $asset)
    }

    # Soft variant: returns $true on success, $false on 404/transport error
    # without raising. Used for the desktop app archive, which is allowed to
    # be missing on releases that ship MCP-only.
    function Download-Optional($asset) {
        Info "downloading $asset"
        try {
            Invoke-WebRequest -UseBasicParsing -Uri "$DownloadBase/$asset" -OutFile (Join-Path $Tmp $asset) -ErrorAction Stop
            return $true
        } catch {
            return $false
        }
    }

    # ---- MCP server ------------------------------------------------------
    if ($env:PM_NO_MCP -ne '1') {
        $McpArchive = "projectmind-mcp-$McpSuffix.tar.gz"
        Download $McpArchive
        $McpExtract = Join-Path $Tmp 'mcp'
        New-Item -ItemType Directory -Force -Path $McpExtract | Out-Null
        tar -xzf (Join-Path $Tmp $McpArchive) -C $McpExtract
        Copy-Item -Path (Join-Path $McpExtract 'projectmind-mcp.exe') -Destination (Join-Path $BinDest 'projectmind-mcp.exe') -Force
        Info "installed: $BinDest\projectmind-mcp.exe"

        # Best-effort: add bin dir to user PATH so `projectmind-mcp` is on PATH
        # in new terminals. Skipping if it's already there to avoid drift.
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if ($userPath -notlike "*$BinDest*") {
            [Environment]::SetEnvironmentVariable('Path', "$userPath;$BinDest", 'User')
            Info "added $BinDest to user PATH (open a new terminal to pick it up)"
        }
    } else {
        Warn "PM_NO_MCP=1 — skipping MCP server"
    }

    # ---- Desktop app -----------------------------------------------------
    if ($env:PM_NO_APP -ne '1') {
        $AppArchive = "projectmind-app-$AppSuffix.tar.gz"
        if (-not (Download-Optional $AppArchive)) {
            Warn "no desktop app bundle in this release ($Tag) — skipping."
            Warn "  the MCP server above is fully functional on its own; re-run"
            Warn "  this script once a release that ships $AppArchive is out, or"
            Warn "  set `$env:PM_NO_APP='1' to silence this warning."
            $env:PM_NO_APP = '1'
        }
    }

    if ($env:PM_NO_APP -ne '1') {
        $AppExtract = Join-Path $Tmp 'app'
        New-Item -ItemType Directory -Force -Path $AppExtract | Out-Null
        tar -xzf (Join-Path $Tmp $AppArchive) -C $AppExtract

        $msi = Get-ChildItem -Path $AppExtract -Filter '*.msi' -Recurse | Select-Object -First 1
        $exe = Get-ChildItem -Path $AppExtract -Filter '*setup*.exe' -Recurse | Select-Object -First 1

        if ($msi) {
            Info "running MSI installer: $($msi.Name)"
            Start-Process -FilePath 'msiexec.exe' -ArgumentList "/i `"$($msi.FullName)`" /qb /norestart" -Wait -NoNewWindow
            Info "installed via msiexec: ProjectMind"
        } elseif ($exe) {
            Info "running NSIS-style installer: $($exe.Name)"
            Start-Process -FilePath $exe.FullName -ArgumentList '/S' -Wait -NoNewWindow
            Info "installed via setup.exe: ProjectMind"
        } else {
            Warn "no .msi or setup .exe found in $AppArchive — desktop app skipped"
        }
    } else {
        Warn "PM_NO_APP=1 — skipping desktop app"
    }

    Write-Host ""
    Write-Host "ProjectMind $Tag installed." -ForegroundColor Green
    Info "MCP server: $BinDest\projectmind-mcp.exe"
    Info "Add to your LLM CLI's mcp config — see https://github.com/$Repo/#readme"
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
