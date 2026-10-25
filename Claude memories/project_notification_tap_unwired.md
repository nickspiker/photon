---
name: project_notification_tap_unwired
description: "BUILT 2026-10-25 (d4a2ffa8): a notification tap opens its conversation on Android + Linux; macOS/Windows notifiers have no click path yet"
metadata:
  type: project
---

Nick asked 2026-10-01 ("clicking the notification does not bring up that conversation"); wired 2026-10-25.
**Android:** `jni_android::notify_new_message(.., conversation)` → Kotlin `postMessageNotification(.., conversation)` puts EXTRA_OPEN_CONVERSATION on the tap intent (FLAG_UPDATE_CURRENT) → PhotonActivity.applyWaveIntent → `nativeOpenConversation` latch → the tick's `take_open_conversation` → `enter_conversation_id` (pairs and groups, one path; the People tap and group tap use the same enter_* helpers).
**Linux:** desktop_notify posts `notify-send --action=default=Open --wait` on a thread; "default" on stdout = clicked → PENDING_OPEN + ShowWindow via the wake set in set_event_proxy; old libnotify falls back to the plain post.
**Still open:** macOS (osascript) and Windows (PowerShell toast) report no click — needs a UNUserNotificationCenter bundle / a registered app id. The push-wake banner (PhotonMessagingService.postWakeNotification) carries no conversation.

**Why:** a notification that opens nothing makes the user hunt for the conversation.
**How to apply:** any new notifier passes the conversation id; any new "open X" path goes thru enter_conversation_id / enter_molecule / enter_pair_conversation.
