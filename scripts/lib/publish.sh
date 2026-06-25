# Sourced, not executed. Cloudflare R2 upload helpers (via wrangler) for the dev channel.
# Binaries + their install scripts land in the `holdmyoscilloscope` bucket under `photon/`;
# users install with `curl -sSfL <R2_BASE_URL>/install-development.sh | sh`.
# (The real version-bumping release upload lives in deploy.sh, not here.)

R2_BUCKET="holdmyoscilloscope"
R2_PATH="photon"
R2_BASE_URL="https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH"

# publish_r2 <object-name> <local-file> [content-type]
publish_r2() {
    local name="$1" file="$2" ctype="$3"
    if [ -n "$ctype" ]; then
        wrangler r2 object put "$R2_BUCKET/$R2_PATH/$name" --file "$file" --content-type "$ctype" --remote
    else
        wrangler r2 object put "$R2_BUCKET/$R2_PATH/$name" --file "$file" --remote
    fi
}
