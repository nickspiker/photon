# Photon Messenger Installer for Windows
# Run this script in PowerShell

Write-Host "Photon Messenger Installer" -ForegroundColor Cyan
Write-Host "============================" -ForegroundColor Cyan
Write-Host ""

# Check if cargo is installed
if (!(Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Rust not found. Installing Rust toolchain..." -ForegroundColor Yellow

    # Download rustup-init
    $rustupInit = "$env:TEMP\rustup-init.exe"
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit

    # Run installer with -y flag for automatic installation
    & $rustupInit -y

    # Clean up
    Remove-Item $rustupInit

    # Refresh environment variables for current session
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path","User") + ";" + [System.Environment]::GetEnvironmentVariable("Path","Machine")

    Write-Host ""
    Write-Host "✓ Rust installed successfully!" -ForegroundColor Green
    Write-Host ""
} else {
    Write-Host "✓ Rust already installed" -ForegroundColor Green
    Write-Host ""
}

Write-Host "Installing Photon Messenger..." -ForegroundColor Yellow
cargo install --locked photon-messenger

Write-Host ""
Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "✓ Photon Messenger installed successfully!" -ForegroundColor Green
Write-Host "==========================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Run 'photon-messenger' to start." -ForegroundColor White
Write-Host ""
Write-Host "Note: You may need to restart your terminal" -ForegroundColor Yellow
Write-Host "      to refresh your PATH environment variable." -ForegroundColor Yellow
