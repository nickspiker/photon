# Sourced, not executed. Cloudflare R2 upload helpers (via wrangler) for the dev channel.
# Binaries + their install scripts land in the `holdmyoscilloscope` bucket under `photon/`;
# users install with `curl -sSfL <R2_BASE_URL>/install-development.sh | sh`.
# (The real version-bumping release upload lives in deploy.sh, not here.)

R2_BUCKET="holdmyoscilloscope"
R2_PATH="photon"
R2_BASE_URL="https://brobdingnagian.holdmyoscilloscope.com/$R2_PATH"

# Never let wrangler open its interactive "report this error to Cloudflare?" prompt — on a transient upload failure it blocks the publish (or an unattended deploy) forever instead of exiting nonzero for the retry below.
export WRANGLER_SEND_METRICS=false

# publish_r2 <object-name> <local-file> [content-type]
# Retries the put twice on failure (transient "fetch failed" socket drops mid-upload are the observed mode; the object create is atomic, so a re-put is safe).
publish_r2() {
    local name="$1" file="$2" ctype="$3" attempt
    for attempt in 1 2 3; do
        if [ -n "$ctype" ]; then
            wrangler r2 object put "$R2_BUCKET/$R2_PATH/$name" --file "$file" --content-type "$ctype" --remote && return 0
        else
            wrangler r2 object put "$R2_BUCKET/$R2_PATH/$name" --file "$file" --remote && return 0
        fi
        echo "publish_r2: attempt $attempt failed for $name — retrying"
    done
    echo "publish_r2: giving up on $name after 3 attempts" >&2
    return 1
}
