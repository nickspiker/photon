# PowerShell build script for debug with hash verification

Write-Host "Building debug binary..." -ForegroundColor Cyan
cargo build

Write-Host "Appending hash for self-verification..." -ForegroundColor Cyan
cargo run --bin hash-release -- target/debug/photon-messenger.exe

Write-Host ""
Write-Host "✓ Debug build complete with hash verification!" -ForegroundColor Green
Write-Host "Binary: target/debug/photon-messenger.exe"
