# Photon Messenger DEVELOPMENT Installer for Windows
# Run this script in PowerShell
# This installs the development build with logging enabled

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

Write-Host "Photon Messenger DEVELOPMENT Installer" -ForegroundColor Magenta
Write-Host "=======================================" -ForegroundColor Magenta
Write-Host ""
Write-Host "This is a DEVELOPMENT build with logging enabled." -ForegroundColor Yellow
Write-Host "Logs will be written to: %APPDATA%\photon\photon.log" -ForegroundColor Yellow
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

# Download binary directly to install location (TEMP often blocked by Defender)
# Add cache-busting query param to bypass Cloudflare CDN cache
$cacheBust = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$downloadUrl = "https://brobdingnagian.holdmyoscilloscope.com/photon/photon-messenger-windows-development.exe?v=$cacheBust"
$installDir = "$env:LOCALAPPDATA\Programs\PhotonMessenger"
New-Item -ItemType Directory -Path $installDir -Force | Out-Null
$binaryPath = "$installDir\photon-messenger.exe"

Write-Host "Downloading Photon Messenger (dev)..." -ForegroundColor Yellow

try {
    Invoke-WebRequest -Uri $downloadUrl -OutFile $binaryPath -ErrorAction Stop
} catch {
    Write-Host "Error: Failed to download binary" -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
    exit 1
}

# Verify SHA256 hash (Defender blocks execution, so we verify hash instead)
Write-Host "Verifying integrity..." -ForegroundColor Yellow

$expectedHash = "7A20ADEB0FAB3B07A64601BD49ED20A4ECE187376DDBA041EDDBC959CB50B450"
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
$Shortcut.Description = "Photon Messenger (DEV) - Decentralized secure messaging"
$Shortcut.WorkingDirectory = $installDir
$Shortcut.Save()

Write-Host "[OK] Start Menu shortcut created" -ForegroundColor Green
Write-Host ""

# Clean up
$ProgressPreference = 'Continue'

Write-Host "==========================================" -ForegroundColor Magenta
Write-Host "Photon Messenger (DEV) installed!" -ForegroundColor Green
Write-Host "==========================================" -ForegroundColor Magenta
Write-Host ""
Write-Host "Run 'photon-messenger' to start." -ForegroundColor White
Write-Host "Or find 'Photon Messenger' in your Start Menu." -ForegroundColor White
Write-Host ""
Write-Host "DEVELOPMENT BUILD - Logs at:" -ForegroundColor Yellow
Write-Host "  $env:APPDATA\photon\photon.log" -ForegroundColor Yellow
Write-Host ""
Write-Host "Note: You may need to restart your terminal" -ForegroundColor Yellow
Write-Host "      to refresh your PATH environment variable." -ForegroundColor Yellow
Write-Host ""
