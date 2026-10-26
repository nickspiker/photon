---
name: project_file_drag_out
description: 2026-10-26 BUILT (unverified on devices) — file drag-out on mac (promise)/Windows (virtual file)/X11 (staged XDND move) + macOS Save panel; no share sheet/AirDrop
metadata:
  type: project
---

Trigger: Nick's MacBook "unable to save pigeons on mac?" — the blob was whole (viewer decoded it); the write into the TCC-guarded Downloads folder failed silently. Fix layers: NSDownloadsFolderUsageDescription in all three plists + `save_refused` logging (0f8e6140), then Nick asked for the permission-free routes: "all of 1 and 2" = macOS save panel + drag-out, and "not a fan of airdrop or resharing tho" (no share sheet, ever, unless he asks). "We don't want random cache hanging around" → no staging where avoidable.

Built: fluor `host/file_drag.rs` — `request(DragFile{name, bytes closure})` + `EventResponse::StartFileDrag` (unit variant; EventResponse stays Copy); host cancels the arbiter + move_drag_armed and calls `file_drag::start`. macOS: NSFilePromiseProvider + NSDraggingSource class `FluorFileDrag` (leaked per drag: provider holds delegate weakly), write on own NSOperationQueue; `save_dialog` = NSSavePanel runModal. Windows: `#[implement(IDataObject, IDropSource)]`, CFSTR_FILEDESCRIPTORW + FILECONTENTS HGLOBAL, DoDragDrop (windows-core 0.58 direct dep for the macro). X11: own x11rb connection + thread, releases winit's implicit grab via dlopen'd XUngrabPointer/xcb_ungrab_pointer (RTLD_NOLOAD), grab + XdndAware walk (skips own window), offers XdndActionMove, answers uri-list/TARGETS/DELETE, staged in $XDG_RUNTIME_DIR/fluor-drag, swept at each drag start.
Photon: press on a held AttachVisual records `file_drag_from`; CursorMoved past FILE_DRAG_SLOP (8 px) → `start_file_drag` (closure = `blob_load_named(hash)`, no seed captured); attach_save → Result<String, SaveMiss{Cancelled,Failed}>, mac goes thru save_dialog.

**How to apply:** compile-checked on all four targets, never run. Field checks: Mac drag to Finder + Save panel appears ABOVE the fullscreen compositor window; Windows Explorer drop; Linux Nautilus/Dolphin drop (watch the "pointer grab refused" log line — the implicit-grab release is the risky bit). Env trap found on the way: a stale audiopus_sys CMakeCache (prefix /usr/local, no toolchain) broke an Android check — delete that build's out/build. Related: [[project_attachments]].
