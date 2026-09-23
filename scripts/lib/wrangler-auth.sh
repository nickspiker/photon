# Sourced, not executed. Wrangler authentication for unattended runs: the bridge, a deploy over SSH, any non-interactive shell. Wrangler's OAuth login lives in a per-user browser flow that a headless session can never complete, and without it wrangler refuses every command with "set CLOUDFLARE_API_TOKEN" (field 2026-09-22: the Android dev publish built a perfect APK, then failed three uploads in a row from the bridge).
# The R2 account token lives beside the other signing material in the keys dir as ONE bare line, `cloudflare-r2-token` (the pasted Cloudflare page is `cloudflare token.txt`; this is just its "Token value"). Same threat model as TOKEN.p12.pass: anyone holding the keys dir already holds worse.
# An already-exported CLOUDFLARE_API_TOKEN wins; an interactive wrangler login still works when the file is absent — this only fills the gap.
if [ -z "${CLOUDFLARE_API_TOKEN:-}" ]; then
    _wa_keys="${TOKEN_KEYS_DIR:-}"
    if [ -z "$_wa_keys" ]; then
        for _d in /mnt/Harbor/Code/keys /mnt/Chiton/MEGA/Code/keys "$HOME/MEGA/code/keys" "$HOME/Code/keys"; do
            [ -d "$_d" ] && { _wa_keys="$_d"; break; }
        done
    fi
    if [ -n "$_wa_keys" ] && [ -r "$_wa_keys/cloudflare-r2-token" ]; then
        CLOUDFLARE_API_TOKEN="$(tr -d ' \n' < "$_wa_keys/cloudflare-r2-token")"
        export CLOUDFLARE_API_TOKEN
        # The account the token belongs to; a token scoped to one account still needs this named when the login has no session to infer it from.
        export CLOUDFLARE_ACCOUNT_ID="${CLOUDFLARE_ACCOUNT_ID:-6be7ec6dc3bb3f60fabb62ff944cc2c2}"
    fi
    unset _wa_keys _d
fi
