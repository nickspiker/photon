---
name: feedback-script-timestamps
description: Every build/deploy script ends by printing a completion timestamp
metadata:
  type: feedback
---

Every build/deploy script's last line on success is `echo "completed $(date '+%F %T')"` — so a build left running answers "when did it finish" from the scrollback.

**Why:** Nick leaves long builds unattended; the terminal's only clock is the output itself.
**How to apply:** any NEW script that builds, publishes, or deploys ends with the stamp (shared helpers count once — see manifest_end_dev_publish); scripts with multiple success exits stamp each.
