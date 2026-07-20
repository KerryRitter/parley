# Parley installer for Windows (PowerShell).
#
#   irm https://raw.githubusercontent.com/KerryRitter/parley/main/install.ps1 | iex
#
# Downloads the prebuilt par.exe from the latest GitHub release and puts it on
# your PATH. No Rust toolchain required.

$ErrorActionPreference = 'Stop'

$repo       = if ($env:PAR_REPO)        { $env:PAR_REPO }        else { 'KerryRitter/parley' }
$installDir = if ($env:PAR_INSTALL_DIR) { $env:PAR_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\par' }

# Only an x64 Windows build is published; it runs fine under x64 emulation on
# Windows on ARM.
$target = 'x86_64-pc-windows-msvc'
$url    = "https://github.com/$repo/releases/latest/download/par-$target.zip"

Write-Host "Downloading $url"
$tmp = Join-Path $env:TEMP ("par-" + [System.Guid]::NewGuid().ToString() + ".zip")
try {
  Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing
} catch {
  throw "Failed to download the Windows release binary from $url. Check that a release with Windows assets exists."
}

New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Expand-Archive -Path $tmp -DestinationPath $installDir -Force
Remove-Item $tmp -Force

$exe = Join-Path $installDir 'par.exe'
if (-not (Test-Path $exe)) {
  throw "Release archive did not contain par.exe"
}

# Add the install dir to the user's PATH if it isn't already there.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $installDir) {
  $newPath = if ([string]::IsNullOrEmpty($userPath)) { $installDir } else { "$userPath;$installDir" }
  [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
  $env:Path = "$env:Path;$installDir"
  Write-Host "Added $installDir to your user PATH. Restart your terminal for it to take effect elsewhere."
}

Write-Host "Installed par to $exe"
Write-Host "Run 'par --version' to verify."
