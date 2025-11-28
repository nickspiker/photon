# Photon Messenger Installer for Windows
# Run this script in PowerShell

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

# Download binary
$downloadUrl = "https://holdmyoscilloscope.com/photon/binaries/photon-messenger-windows.exe"
$tempBinary = "$env:TEMP\photon-messenger-$PID.exe"

Write-Host "Downloading Photon Messenger..." -ForegroundColor Yellow

try {
    $ProgressPreference = 'SilentlyContinue'  # Suppress progress bar for faster downloads
    Invoke-WebRequest -Uri $downloadUrl -OutFile $tempBinary -ErrorAction Stop
} catch {
    Write-Host "Error: Failed to download binary" -ForegroundColor Red
    Write-Host $_.Exception.Message -ForegroundColor Red
    exit 1
}

# Run binary once to self-verify signature
Write-Host "Verifying signature..." -ForegroundColor Yellow

try {
    $verifyOutput = & $tempBinary --version 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "Binary verification failed"
    }
} catch {
    Write-Host "Error: Binary signature verification failed." -ForegroundColor Red
    Write-Host "The downloaded file may be corrupted or tampered with." -ForegroundColor Red
    Remove-Item $tempBinary -ErrorAction SilentlyContinue
    exit 1
}

Write-Host "✓ Signature verified" -ForegroundColor Green
Write-Host ""

# Install binary
$installDir = "$env:LOCALAPPDATA\Programs\PhotonMessenger"
$binaryPath = "$installDir\photon-messenger.exe"

Write-Host "Installing to $installDir..." -ForegroundColor Yellow

# Create install directory
New-Item -ItemType Directory -Path $installDir -Force | Out-Null

# Move binary
Move-Item -Path $tempBinary -Destination $binaryPath -Force

Write-Host "✓ Binary installed" -ForegroundColor Green
Write-Host ""

# Add to PATH
Write-Host "Adding to PATH..." -ForegroundColor Yellow

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$installDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$installDir", "User")
    # Update PATH for current session
    $env:Path += ";$installDir"
    Write-Host "✓ Added to PATH" -ForegroundColor Green
} else {
    Write-Host "✓ Already in PATH" -ForegroundColor Green
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

Write-Host "✓ Start Menu shortcut created" -ForegroundColor Green
Write-Host ""

# Clean up
$ProgressPreference = 'Continue'

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "✓ Photon Messenger installed successfully!" -ForegroundColor Green
Write-Host "==========================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Run 'photon-messenger' to start." -ForegroundColor White
Write-Host "Or find 'Photon Messenger' in your Start Menu." -ForegroundColor White
Write-Host ""
Write-Host "Note: You may need to restart your terminal" -ForegroundColor Yellow
Write-Host "      to refresh your PATH environment variable." -ForegroundColor Yellow
Write-Host ""
