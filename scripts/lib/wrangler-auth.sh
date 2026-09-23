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

# PAGES IS A DIFFERENT PERMISSION FROM R2 (field 2026-09-23, the v102 deploy: "✘ Authentication error [code: 10000]" on `pages deploy`, every other stage green).
# The R2 account token above is scoped to buckets by construction — Cloudflare mints it from the R2 page and it can never touch Pages — so the website step needs its own credential, and until 2026-09-22 it silently had one: nothing exported CLOUDFLARE_API_TOKEN, so wrangler fell through to the browser login, whose scopes include pages:write.
# Exporting the R2 token for the bridge took that fallback away, because an env token outranks the login.
# So: name the Pages credential separately and let the website step pick it, in this order.
#   1. CLOUDFLARE_PAGES_TOKEN already in the environment.
#   2. keys/cloudflare-pages-token — a token minted with "Cloudflare Pages: Edit" (the file is ONE bare line, the Token value, like its R2 neighbour).
#   3. keys/cloudflare-api-token — ONE combined token carrying both Pages:Edit and Workers R2 Storage:Edit, which is the tidiest arrangement: it serves this and the R2 puts alike.
#   4. the browser login, iff wrangler holds one that has not expired — reached by UNSETTING the env token for that one call, which is what `wrangler_pages_env` prints.
# `wrangler_pages_env` echoes the env assignment the caller should run the Pages command under, and nothing at all when no credential can do the job (the caller then says what to create).
wrangler_pages_token() {
    if [ -n "${CLOUDFLARE_PAGES_TOKEN:-}" ]; then
        printf '%s' "$CLOUDFLARE_PAGES_TOKEN"
        return 0
    fi
    _wp_keys="${TOKEN_KEYS_DIR:-}"
    if [ -z "$_wp_keys" ]; then
        for _d in /mnt/Harbor/Code/keys /mnt/Chiton/MEGA/Code/keys "$HOME/MEGA/code/keys" "$HOME/Code/keys"; do
            [ -d "$_d" ] && { _wp_keys="$_d"; break; }
        done
    fi
    for _f in cloudflare-pages-token cloudflare-api-token; do
        if [ -n "$_wp_keys" ] && [ -r "$_wp_keys/$_f" ]; then
            tr -d ' \n' < "$_wp_keys/$_f"
            unset _wp_keys _d _f
            return 0
        fi
    done
    unset _wp_keys _d _f
    return 1
}

# Does wrangler hold a browser login that is still good? Its config carries an ISO expiry beside the scopes; an expired one makes wrangler demand a token in any non-interactive shell, which a deploy always is.
wrangler_login_live() {
    _wl_cfg="${WRANGLER_CONFIG:-$HOME/.config/.wrangler/config/default.toml}"
    [ -r "$_wl_cfg" ] || { unset _wl_cfg; return 1; }
    grep -q 'pages:write' "$_wl_cfg" || { unset _wl_cfg; return 1; }
    _wl_exp="$(sed -n 's/^expiration_time[[:space:]]*=[[:space:]]*"\(.*\)".*/\1/p' "$_wl_cfg" | head -1)"
    unset _wl_cfg
    [ -n "$_wl_exp" ] || return 1
    # String compare is enough: both sides are ISO-8601 in UTC, and lexical order IS chronological order there.
    [ "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \< "$_wl_exp" ]
}

# Arm this shell for a Pages command: a token when we have one, else the login (env token CLEARED so wrangler reaches for it).
# Call it inside the subshell that runs the command — it exports, deliberately, rather than printing an `env VAR=…` prefix: a token on a command line is readable in the process list by anyone on the box, and a deploy under `set -x` would echo it.
wrangler_pages_auth() {
    if _wp_tok="$(wrangler_pages_token)"; then
        CLOUDFLARE_API_TOKEN="$_wp_tok"
        CLOUDFLARE_ACCOUNT_ID="${CLOUDFLARE_ACCOUNT_ID:-6be7ec6dc3bb3f60fabb62ff944cc2c2}"
        export CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID
        unset _wp_tok
        return 0
    fi
    wrangler_login_live || return 1
    unset CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID
    return 0
}

# Quiet predicate for the caller's branch: is there any credential that can reach Pages?
wrangler_pages_available() {
    wrangler_pages_token > /dev/null 2>&1 || wrangler_login_live
}
