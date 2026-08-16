---
name: project_bridge
description: "BRIDGE = passless remote shell between fleet siblings over PT (SSH replacement for rustdesk); HOST HALF SHIPPED @ c6de89b + 801d902, CLIENT + Security toggle + notification are the next session"
metadata: 
  node_type: memory
  type: project
  originSessionId: f7df2de2-8ae4-45a0-a572-2865a6e4ac5f
---

BRIDGE = passless remote shell between fleet siblings, carried over PT instead of SSH (user's rustdesk replacement; rustdesk crashes constantly — 15+ coredumps on this box). Client and host are ALWAYS fleet siblings until invites land later, so auth = possession of a fold-verified sibling device (the fleet key already encodes that trust); no password.

HOST HALF SHIPPED 2026-07-27 @ c6de89b (+ CLI opt-in 801d902), compiling + 142 tests green, desktop-unix only (real PTY via libc::forkpty):
- protocol.rs: build/parse_term_vsf — ONE 'term' frame family discriminated by `kind` (term_kind::OPEN/DATA/RESIZE/CLOSE/EXIT/NUKE), device-signed, 16-byte session_id, payload fleet-sealed.
- network/bridge.rs: BridgeHost owns live PTY sessions (forkpty + login shell, per-session reader thread streams output via mpsc, write_input/resize/close/nuke, generation-tagged so a nuked shell's stale output drops). seal_term/open_term fold fleet key with blake3 ctx "photon.bridge.term.v0". winsize pack/unpack.
- status.rs: TermReceived StatusUpdate + datagram dispatch + packet-ack.
- photon_app.rs: on_term_frame drives the host behind TWO gates (remote_terminal_enabled() marker + fold-verified SIBLING, never a friend); drive_bridge_output drains shell output→sealed DATA/EXIT frames each tick; send_term_frame = reply path (reuses send_history vehicle); OPEN toasts (ears-and-eyes). Term frames deferred past the drain's checker borrow (term_frames vec, like fleet_tx_rows). Host opt-in = <config>/remote_terminal marker, OFF by default, set via `photon-messenger --enable/--disable-remote-terminal`.

CLIENT DESIGN DECIDED 2026-07-27 (NOT built yet — planned before user's flight): NOT a separate binary/photonsh — an IN-APP terminal SCREEN that makes THIS device the client speaking the existing term_* frames. Decisions: (a) LINE-BASED PLAIN-TEXT v1 (strip ANSI escapes, show stdout/stderr as text — runs ls/cat/grep/scripts; full vt100 cell-grid emulator = phase 2); (b) BRIDGE PILL PER ONLINE-SIBLING ROW on Settings→Your devices (SettingsPage::Fleet render ~6167; rows already carry per-device hit-ids btn_base+16+i tap-copy, +24+i retired-Release — slot the Bridge pill at btn_base+8+i on online non-self non-retired rows).

BUILD PLAN (one focused session): (1) new AppState::Bridge + BridgeClient struct {session_id: [u8;16] random, target sibling pubkey, scrollback Vec<u8>, input line, status}; (2) tap Bridge → gen session_id, send term_open (cols×rows) via send_history+build_term_vsf, switch to AppState::Bridge; (3) CLIENT side of TermReceived dispatch (host side exists): DATA for our active session_id → append scrollback + repaint, EXIT → "[shell exited]"; (4) input: keystrokes → term_data frames (line-based: Enter sends the line); (5) render mono-text scrollback grid + input line + Nuke pill (term_nuke) + back button (term_close on exit). WATCH: the on_activate scope-gate bug — dispatch handlers go BEFORE state gates, never nested inside an AppState::Conversation/other block. Host opt-in already CLI-settable (--enable-remote-terminal).

ALSO still deferred: Security toggle UI for host opt-in; session-open notification beyond the toast. Related: [[project_unattended_reboot]] (the failsafe box this shells into), [[project_peers_are_fgtw]], [[reference_ihi_primitives]]. ferros has a ferros-bridge tool (USB/PT terminal) but it's line-based, NOT a real PTY — different thing.
