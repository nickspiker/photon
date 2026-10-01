---
name: project_notification_tap_unwired
description: OPEN 2026-10-01 — tapping a new-message notification opens nothing on any platform; Android plan = contact-id extra on the content PendingIntent + one JNI latch drained in tick (the wave-answer pattern); desktop notifiers have no click path at all
metadata:
  type: project
---

Traced 2026-10-01 (Nick: "clicking the notification does not bring up that conversation").

**Android:** message notifications are posted from conversation.rs (~1978 per message, ~2773 batch) thru jni_android::notify_new_message → Kotlin PhotonConnectionService.postMessageNotification (android/.../PhotonConnectionService.kt ~596); the app-not-running push wake posts a generic banner from PhotonMessagingService.postWakeNotification (~85, knows no sender). Both content intents are `PendingIntent.getActivity(this, 0, Intent(PhotonActivity), FLAG_IMMUTABLE)` with NO extras; PhotonActivity (singleTask) only reads the two wave extras in applyWaveIntent (~913). A tap just foregrounds the app. The fixed MESSAGE_NOTIFICATION_ID collapses a multi-sender burst into one banner.
**Desktop:** src/platform/desktop_notify.rs spawns notify-send / osascript / a PowerShell toast and never waits; no click callback exists (macOS shows as Script Editor, Windows as PowerShell). Opening a conversation from a click needs a real notifier (notify-send --action --wait on a thread, UNUserNotificationCenter bundle, registered Windows app id) sending a PhotonEvent like control.rs's ShowWindow.
**The open sequence** already exists: open_conversation_with(ci) (conversation.rs ~335) then the 7-line tail at driver.rs ~1512 (state = Conversation, reset_contact_ping_backoff, conv_topbar_off, conv_blind_*, clear_unread, change_focus(None)) — repeated in bridge.rs/wave_ui.rs; pull into one helper and key it by ContactId, not index.

**How to apply:** Android = add a contact-id extra (request code or FLAG_UPDATE_CURRENT so extras refresh), read it in onCreate/onNewIntent, a new JNI `nativeOpenConversation` latch like PENDING_WAVE_ACTION (jni_android.rs ~125), drained in tick like take_wave_action (driver.rs ~3582). Needs an Android build (scripts/android/dev-adb.sh) — not done in the desktop-only 2026-10-01 batch; Nick's go pending.
