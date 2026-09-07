#!/bin/bash
# Install + enable the photon-lifeline user unit (docs/headless-lifeline.md): a watcher that becomes the headless bridge host the moment no real photon holds the instance lock. The user manager survives session death (proven across the 2026-09-04 three-day incident), so this unit is the guarantee that there is always SOME photon answering.
set -e
UNIT_DIR="$HOME/.config/systemd/user"
mkdir -p "$UNIT_DIR"
cat > "$UNIT_DIR/photon-lifeline.service" <<'EOF'
[Unit]
Description=Photon headless lifeline (bridge survives X death — docs/headless-lifeline.md)

[Service]
# The shim verifies the binary then execs it with args forwarded; --lifeline watches the instance lock and pumps headless only while nothing else holds it.
ExecStart=%h/.local/bin/photon-launch --lifeline
# A yield handoff exits 0 (full-UI launch took the lock) and a crash exits non-zero — both must come back as the watcher.
Restart=always
RestartSec=10

[Install]
WantedBy=default.target
EOF
systemctl --user daemon-reload
systemctl --user enable --now photon-lifeline.service
systemctl --user --no-pager status photon-lifeline.service | head -5
echo "completed $(date)"
