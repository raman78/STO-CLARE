# Bootstrap installer for STO-CLARE (Windows).
#
# Downloads the latest installer (.exe) from GitHub Releases and runs it. The
# installer lays the app down under %LOCALAPPDATA%\Programs and registers a
# Start Menu shortcut; re-running upgrades in place.
$ErrorActionPreference = 'Stop'

$Repo = 'raman78/STO-CLARE'

Write-Host '==========================================='
Write-Host '  STO-CLARE installer (Windows)'
Write-Host '==========================================='

Write-Host 'Looking up the latest release...'
# Use /releases (not /releases/latest): the latter excludes pre-releases. The
# list is newest-first, so the first entry is the newest release.
$releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases" `
    -Headers @{ 'User-Agent' = 'sto-clare-installer' }
$release = $releases | Select-Object -First 1

$asset = $release.assets | Where-Object { $_.name -like '*-setup.exe' } | Select-Object -First 1
if (-not $asset) {
    throw "No -setup.exe asset found in the latest release. See https://github.com/$Repo/releases"
}

$tmp = Join-Path $env:TEMP $asset.name
Write-Host "Downloading $($asset.browser_download_url)"
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $tmp -Headers @{ 'User-Agent' = 'sto-clare-installer' }

Write-Host 'Launching the installer...'
Start-Process -FilePath $tmp -Wait

Write-Host '==========================================='
Write-Host 'Done.'
Write-Host '==========================================='
