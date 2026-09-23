#!/bin/bash
# Deploy the website alone, with whatever credential can reach Pages (photon's wrangler-auth rules).
set -e
cd /mnt/Harbor/Code/photon
source scripts/lib/wrangler-auth.sh
if ! wrangler_pages_available; then
    echo "No credential can reach Cloudflare Pages."
    echo "Mint one at https://dash.cloudflare.com/profile/api-tokens with 'Cloudflare Pages: Edit'"
    echo "and save its value as ONE line in /mnt/Harbor/Code/keys/cloudflare-pages-token"
    exit 1
fi
(wrangler_pages_auth && cd /mnt/Chiton/MEGA/holdmyoscilloscope && ./deploy.sh)
