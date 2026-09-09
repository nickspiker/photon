---
name: project_links
description: message links = typed MARKS (kind 1 = link, byte range + dest) beside the plaintext; send-side bare-URL detection, receiver validation, link-coloured bubble spans with a consent dialog; 2026-09-09 LIVE detection in the compose box on the edit edge + paste-onto-selection tagging (desktop chord) — previews and non-http schemes deliberately absent
metadata:
  type: project
---

Links are typed content marks (`types::MessageMark`: kind, byte start, byte len, dest) riding the message package as fields beside the body, validated on receive (`valid_marks`: known kind, char-boundary ranges, no overlap, http/https only), persisted in the row, painted in the link colour with a hit rect, and opened only thru the consent dialog that shows the full destination.

2026-09-09: fluor `MultiTextbox` gained styled `Span`s (char range + colour + optional dest) that ride every edit (`spans_on_insert`/`spans_on_delete`, `edit_seq` as the app's change edge). Photon runs `detect_url_marks` over the compose text on that edge (`sync_compose_link_spans`, called at the top of render_frame) so a URL is link-coloured the moment it is one. A URL pasted onto a selection (desktop Ctrl/Cmd+V) tags the selection as a link (`tag_link`); tagged marks leave with the text (`take_compose_tagged_marks` → `marks_for_send`), and a re-serve reads the ROW's marks, never re-detecting, so the tag survives. Android tagging gesture: not built (the IME owns paste there).

Later 2026-09-09 (the link button, Nick's spec): links are PURPLE everywhere (`theme::LINK_PURPLE`, bold weight 700 in bubbles). A purple chain-link `Button` (`compose_link_btn`, glyph U+1F517 FE0F) sits one slot left of send while the box holds a bare URL span (`compose_link_available`); the click (`compose_link_click`) relabels that span to the literal trim (scheme off, trailing slash off: "https://passless.org/photon/" → "passless.org/photon") via fluor `relabel_span`, keeping the full URL as dest. fluor rules after a relabel: the first valid URL char typed at the caret REPLACES the label (so it can read "Photon"), further URL chars extend the link, any other char (space included) ends it; the caret tints to the span under it (`draw_blinkey_tinted`) so the writer sees which side of the edge the next key lands. The consent dialog receives the mark's dest, never the label.

**Deliberately absent:** link previews (a fetch on someone's behalf), bare domains / mailto / tel, markdown syntax.
