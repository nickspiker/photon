# PowerShell build script for release with hash verification

Write-Host "Building release binary..." -ForegroundColor Cyan
cargo build --release

Write-Host "Appending hash for self-verification..." -ForegroundColor Cyan
cargo run --release --bin hash-release -- target/release/photon-messenger.exe

Write-Host ""
Write-Host "✓ Release build complete with hash verification!" -ForegroundColor Green
Write-Host "Binary: target/release/photon-messenger.exe"
