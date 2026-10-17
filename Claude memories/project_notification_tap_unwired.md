---
name: project_notification_tap_unwired
description: "OPEN since 2026-10-01: tapping a new-message notification opens nothing on any platform; Android plan = contact-id extra + JNI latch drained in tick; desktop notifiers have no click path"
metadata:
  type: project
---

Nick 2026-10-01: "clicking the notification does not bring up that conversation". Still unwired as of 2026-10-17 (no `nativeOpenConversation` in the tree).
**Android:** message notifications go conversation.rs → `jni_android::notify_new_message` → PhotonConnectionService.postMessageNotification; the push wake posts a generic banner (PhotonMessagingService.postWakeNotification). Both content PendingIntents carry NO extras; PhotonActivity (singleTask) reads only the wave extras (applyWaveIntent). The fixed MESSAGE_NOTIFICATION_ID collapses a multi-sender burst into one banner.
**Desktop:** src/platform/desktop_notify.rs fires notify-send / osascript / a PowerShell toast and never waits — no click callback. A real notifier is needed (notify-send --action --wait on a thread, a UNUserNotificationCenter bundle, a registered Windows app id) sending a PhotonEvent like control.rs's ShowWindow.
**The open sequence exists:** `open_conversation_with(ci)` plus the tail in driver.rs (state = Conversation, ping backoff reset, topbar/blind reset, clear_unread, change_focus(None)), repeated in bridge.rs / wave_ui.rs — pull it into one helper keyed by ContactId, not index.

**Why:** a notification that opens nothing makes the user hunt for the conversation.
**How to apply:** Android = contact-id extra (unique request code or FLAG_UPDATE_CURRENT), read in onCreate/onNewIntent, a `nativeOpenConversation` latch like PENDING_WAVE_ACTION drained in tick like take_wave_action; build with scripts/android/dev-adb.sh. Nick's go pending.
