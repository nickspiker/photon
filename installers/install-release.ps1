# Photon Messenger Installer for Windows
# Run this script in PowerShell

# Make errors visible instead of silent BOOP
$ErrorActionPreference = "Stop"
trap {
    Write-Host ""
    Write-Host "ERROR: $_" -ForegroundColor Red
    Write-Host "At: $($_.InvocationInfo.ScriptLineNumber): $($_.InvocationInfo.Line.Trim())" -ForegroundColor Red
    Write-Host ""
    Write-Host "Press Enter to exit..." -ForegroundColor Yellow
    Read-Host
    exit 1
}

Write-Host "Photon Messenger Installer" -ForegroundColor Cyan
Write-Host "============================" -ForegroundColor Cyan
Write-Host ""

# Detect architecture
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne "AMD64" -and $arch -ne "ARM64") {
    Write-Host "Error: Unsupported architecture: $arch" -ForegroundColor Red
    Write-Host "Photon currently supports x64 and ARM64 only." -ForegroundColor Red
    exit 1
}

Write-Host "Detected: Windows ($arch)" -ForegroundColor White
Write-Host ""

# Download binary directly to install location (TEMP often blocked by Defender).
# Pick the native asset per arch: ARM64 (Snapdragon X / Copilot+ PCs) gets the native aarch64 build — no x86
# emulation — while x64 keeps the existing asset. If the arm64 asset is ever absent for a release, an ARM64 box
# can still run the x64 build under Windows' Prism emulation, so a missing native build degrades, never breaks.
if ($arch -eq "ARM64") {
    $downloadUrl = "https://brobdingnagian.holdmyoscilloscope.com/photon/photon-messenger-windows-arm64-release.exe"
} else {
    $downloadUrl = "https://brobdingnagian.holdmyoscilloscope.com/photon/photon-messenger-windows-release.exe"
}
$installDir = "$env:LOCALAPPDATA\Programs\PhotonMessenger"
New-Item -ItemType Directory -Path $installDir -Force | Out-Null
$binaryPath = "$installDir\photon-messenger.exe"

Write-Host "Downloading Photon Messenger..." -ForegroundColor Yellow

try {
    Invoke-WebRequest -Uri $downloadUrl -OutFile $binaryPath -ErrorAction Stop
} catch {
    Write-Host "Error: Failed to download binary" -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
    exit 1
}

# Verify SHA256 hash (Defender blocks execution, so we verify hash instead)
Write-Host "Verifying integrity..." -ForegroundColor Yellow

# Per-arch expected hashes — deploy.sh patches BOTH placeholders (x64 from the x86_64 build, arm64 from the
# aarch64 build). The arm64 line stays a zero placeholder for any release built without the Windows-ARM
# toolchain; an ARM64 box only reaches it when it actually downloaded the arm64 asset, so a placeholder there
# means "no native arm64 this release" and the mismatch correctly refuses a wrong download.
$expectedHashX64 = "867F644EEB52B2979339681025DB70650400FAE1011F815AAF72064E59935719"
$expectedHashArm64 = "0000000000000000000000000000000000000000000000000000000000000000"
if ($arch -eq "ARM64") { $expectedHash = $expectedHashArm64 } else { $expectedHash = $expectedHashX64 }
$actualHash = (Get-FileHash $binaryPath -Algorithm SHA256).Hash

if ($actualHash -ne $expectedHash) {
    Write-Host "Error: Hash verification failed." -ForegroundColor Red
    Write-Host "  Expected: $expectedHash" -ForegroundColor Red
    Write-Host "  Got:      $actualHash" -ForegroundColor Red
    Write-Host "The downloaded file may be corrupted or tampered with." -ForegroundColor Red
    Remove-Item $binaryPath -ErrorAction SilentlyContinue
    exit 1
}

Write-Host "[OK] Integrity verified" -ForegroundColor Green
Write-Host ""

Write-Host "[OK] Binary installed to $installDir" -ForegroundColor Green
Write-Host ""

# Resilient launch (docs/resilient-launch.md): a SECOND copy under a different LOCALAPPDATA root, so a nuked or corrupt primary still leaves a good binary. The updater keeps both fresh; the verify-and-fallback launch shim is Linux-first (Windows keeps launching the primary directly for now). Best-effort — a failed second copy never fails the install.
$copyBDir = "$env:LOCALAPPDATA\PhotonMessenger"
try {
    New-Item -ItemType Directory -Path $copyBDir -Force | Out-Null
    Copy-Item -Path $binaryPath -Destination "$copyBDir\photon-messenger.exe" -Force
    Write-Host "[OK] Second copy installed to $copyBDir" -ForegroundColor Green
    Write-Host ""
} catch {
    Write-Host "  (second copy not installed: $_)" -ForegroundColor Yellow
}

# Add to PATH
Write-Host "Adding to PATH..." -ForegroundColor Yellow

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$installDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$installDir", "User")
    # Update PATH for current session
    $env:Path += ";$installDir"
    Write-Host "[OK] Added to PATH" -ForegroundColor Green
} else {
    Write-Host "[OK] Already in PATH" -ForegroundColor Green
}

Write-Host ""

# Create Start Menu shortcut
Write-Host "Creating Start Menu shortcut..." -ForegroundColor Yellow

$startMenu = [System.IO.Path]::Combine($env:APPDATA, "Microsoft\Windows\Start Menu\Programs")
$shortcutPath = [System.IO.Path]::Combine($startMenu, "Photon Messenger.lnk")

$WshShell = New-Object -ComObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut($shortcutPath)
$Shortcut.TargetPath = $binaryPath
$Shortcut.Description = "Photon Messenger - Decentralized secure messaging"
$Shortcut.WorkingDirectory = $installDir
$Shortcut.Save()

Write-Host "[OK] Start Menu shortcut created" -ForegroundColor Green
Write-Host ""

# Clean up
$ProgressPreference = 'Continue'

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "Photon Messenger installed successfully!" -ForegroundColor Green
Write-Host "==========================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Run 'photon-messenger' to start." -ForegroundColor White
Write-Host "Or find 'Photon Messenger' in your Start Menu." -ForegroundColor White
Write-Host ""
Write-Host "Note: You may need to restart your terminal" -ForegroundColor Yellow
Write-Host "      to refresh your PATH environment variable." -ForegroundColor Yellow
Write-Host ""
