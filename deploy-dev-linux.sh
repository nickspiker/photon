#!/bin/bash
set -e

# Deploy Linux development build with logging to R2
# Fast debug builds (~30 sec) with --features development

R2_BUCKET="holdmyoscilloscope"
R2_PATH="photon"

echo "Building Linux development binary..."
cargo build --features development

echo ""
echo "Signing Linux binary..."
./sign-after-build.sh debug

echo ""
echo "Uploading to R2..."
wrangler r2 object put "$R2_BUCKET/$R2_PATH/photon-messenger-linux-development" \
    --file target/debug/photon-messenger --remote

# Also upload the install script
wrangler r2 object put "$R2_BUCKET/$R2_PATH/install-development.sh" \
    --file install-development.sh --content-type text/plain --remote

echo ""
echo "Linux dev build deployed to R2"
echo "  Binary: https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/photon-messenger-linux-development"
echo ""
echo "Install with:"
echo "  curl -sSfL https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH/install-development.sh | sh"
