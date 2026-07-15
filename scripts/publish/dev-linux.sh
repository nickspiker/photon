#!/bin/bash
# Publish a Linux dev build (logging on) to the R2 dev channel: build -> sign -> upload binary + installer.
# Not a local install — this pushes to the CDN so the team installs with the printed curl|sh.
set -e
cd "$(dirname "$0")/../.."
source scripts/lib/sign.sh
source scripts/lib/publish.sh
source scripts/lib/github.sh
source scripts/lib/manifest.sh

case "$(uname -m)" in
    aarch64 | arm64) arch=arm64 ;;
    *) arch=x86_64 ;;
esac

# Refuse-dirty + patch-bump + commit BEFORE the build, so the binary embeds a clean HEAD whose commit is exactly what the signed manifest claims (docs/updates.md).
manifest_begin_dev_publish "linux-$arch"

echo "Building Linux $arch development binary..."
cargo build --features development
sign_binary debug

echo "Uploading to R2 (primary)..."
publish_r2 "photon-messenger-linux-$arch-development" target/debug/photon-messenger
publish_r2 "install-development.sh" installers/install-development.sh text/plain

echo "Publishing dev manifest row..."
manifest_publish_dev_row "Linux" "$arch" "photon-messenger-linux-$arch-development" target/debug/photon-messenger

echo "Mirroring to GitHub Releases (dev)..."
publish_github_dev "photon-messenger-linux-$arch-development" target/debug/photon-messenger

echo ""
echo "Linux $arch dev published:"
echo "  $R2_BASE_URL/photon-messenger-linux-$arch-development"
echo "  Install: curl -sSfL $R2_BASE_URL/install-development.sh | sh"
