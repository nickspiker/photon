---
name: feedback-voca-camelcase
description: Default voca-encoded values to camelCase concatenation (no spaces); space-separated form is opt-in for read-aloud transcription
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 9ca1f039-527e-467a-81cb-1540c007c7b0
---

When emitting voca-encoded values (device keys, pairing codes, IDs) in logs, JSON, error messages, chat, or any text context where the value sits alongside other text, default to **camelCase concatenation** of the words (e.g. `conditionMediaTradeFuture`), not space-separated.

**Why:** Double-click in terminals/editors/chat selects a "word" — defined by whitespace. Space-separated voca codes can't be copied with one gesture; camelCase concatenation makes the entire code one selectable token. Same reasoning as [[feedback-terminal-clipboard]] (spaces around `=` so double-click isolates the value) — both rules optimize for the gesture that actually gets used.

**How to apply:** When encoding values for any text surface, emit camelCase. Reserve the space-separated form for cases where the value is read aloud and a human is transcribing in real-time — there the word boundaries help the listener segment.
