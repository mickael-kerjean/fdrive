# Build the Windows client.
#
# Debug (default): stop the running client, build, relaunch it.
# -Release: build, stage a signed artifact in target\windows-release.
#
# Signing is opt-in through the environment, the artifact is left unsigned when
# nothing is configured:
#   FDRIVE_SIGN_THUMBPRINT  sha1 of a certificate in the current user store, or
#   FDRIVE_SIGN_PFX         path to a .pfx, with FDRIVE_SIGN_PASSWORD

param(
    [switch]$Release,
    [string]$Target
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Root = Split-Path -Parent $PSScriptRoot
$Config = if ($Release) { "release" } else { "debug" }
$Timestamp = "http://timestamp.digicert.com"

function Host-Triple {
    $line = @(& rustc -vV) -match '^host: '
    if (-not $line) { throw "cannot read host triple from rustc" }
    $line[0].Split(' ')[1]
}

function Find-SignTool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $kits = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    if (-not (Test-Path $kits)) { return $null }
    $found = Get-ChildItem -Path $kits -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Sort-Object FullName -Descending | Select-Object -First 1
    if ($found) { return $found.FullName }
    return $null
}

# The exe is locked while it runs and it holds instance.lock, so nothing builds
# or launches until the old one is gone. Kills are safe, the journal replays
# unpushed edits on the next start.
function Stop-Client {
    $procs = @(Get-Process fdrive-windows -ErrorAction SilentlyContinue)
    if ($procs.Count -eq 0) { return }
    Write-Host "stopping running client"
    $procs | Stop-Process -Force
    $procs | Wait-Process -Timeout 15 -ErrorAction SilentlyContinue
}

if (-not $Target) {
    $Target = Host-Triple
} elseif (Get-Command rustup -ErrorAction SilentlyContinue) {
    rustup target add $Target
    if ($LASTEXITCODE -ne 0) { throw "rustup target add $Target failed" }
}
$Exe = Join-Path $Root "target\$Target\$Config\fdrive-windows.exe"

Set-Location $Root
Stop-Client

$flags = @("build", "-p", "fdrive-windows", "--target", $Target)
if ($Release) { $flags += "--release" }
cargo @flags
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

if (-not $Release) {
    Write-Host "launching $Exe"
    Start-Process -FilePath $Exe
    exit 0
}

$Out = Join-Path $Root "target\windows-release"
$Artifact = Join-Path $Out ("fdrive-windows-{0}.exe" -f $Target.Split('-')[0])

Remove-Item -Recurse -Force $Out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item $Exe $Artifact

$signtool = Find-SignTool
$sign = @("sign", "/fd", "SHA256", "/tr", $Timestamp, "/td", "SHA256")
if ($env:FDRIVE_SIGN_THUMBPRINT) {
    $sign += @("/sha1", $env:FDRIVE_SIGN_THUMBPRINT)
} elseif ($env:FDRIVE_SIGN_PFX) {
    $sign += @("/f", $env:FDRIVE_SIGN_PFX)
    if ($env:FDRIVE_SIGN_PASSWORD) { $sign += @("/p", $env:FDRIVE_SIGN_PASSWORD) }
} else {
    $sign = $null
    Write-Host "no signing certificate configured, artifact stays unsigned" -ForegroundColor Yellow
}
if ($sign -and -not $signtool) {
    throw "signtool.exe not found, install the Windows SDK signing tools"
}
if ($sign) {
    & $signtool @sign $Artifact
    if ($LASTEXITCODE -ne 0) { throw "signtool sign failed" }
    & $signtool verify /pa $Artifact
    if ($LASTEXITCODE -ne 0) { throw "signtool verify failed" }
}

Write-Host "release artifact: $Artifact"
Write-Host "sha256: $((Get-FileHash $Artifact -Algorithm SHA256).Hash.ToLower())"
