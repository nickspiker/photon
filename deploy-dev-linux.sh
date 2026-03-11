#!/bin/bash
set -e

# Deploy Linux development build with logging to R2
# Fast debug builds (~30 sec) with --features development

R2_BUCKET="holdmyoscilloscope"
R2_PATH="photon"

# Detect host architecture for naming
ARCH=$(uname -m)
if [ "$ARCH" = "aarch64" ] || [ "$ARCH" = "arm64" ]; then
    ARCH_NAME="arm64"
else
    ARCH_NAME="x86_64"
fi

echo "Building Linux $ARCH_NAME development binary..."
cargo build --features development

echo ""
echo "Signing Linux binary..."
./sign-after-build.sh debug

echo ""
echo "Uploading to R2..."
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-linux-$ARCH_NAME-development" \
    --file target/debug/photon-messenger --remote

# Also upload the install script
wrangler r2 object put "$R2_BUCKET/$R2_PATH/install-development.sh" \
    --file install-development.sh --content-type text/plain --remote

echo ""
echo "Linux $ARCH_NAME dev build deployed to R2"
echo "  Binary: https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/photon-messenger-linux-$ARCH_NAME-development"
echo ""
echo "Install with:"
echo "  curl -sSfL https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-development.sh | sh"
