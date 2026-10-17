---
name: project_links
description: "Message links = typed MARKS (kind 1 = link, byte range + dest) beside the plaintext; a purple chain-link button relabels a bare URL on consent; nothing auto-links while typing; no previews"
metadata:
  type: project
---

Links are typed content marks (`types::MessageMark`: kind, byte start, len, dest) riding the message package beside the body.
They are validated on receive (`valid_marks`: known kind, char-boundary ranges, no overlap, http/https only), persisted in the row, painted purple (`theme::LINK_PURPLE`, bold in bubbles), and opened only thru the consent dialog showing the full destination (the dialog gets the mark's dest, never the label).
Compose (Nick's spec 2026-09-09, CONSENT-ORIENTED): NOTHING is auto-linked or coloured while typing.
A purple chain-link button (`compose_link_btn`, U+1F517 FE0F) shows left of send while `compose_bare_urls()` is non-empty; its click (`compose_link_click` → fluor `relabel_span`) turns the address under the caret into a tagged link labelled with the trimmed address, the label SELECTED: typing replaces it, whitespace/arrows/a click accept, Backspace/Escape revert to plain text.
Desktop Ctrl/Cmd+V of a URL over a selection also tags it.
Tagged marks leave with the text (`take_compose_tagged_marks` → `marks_for_send`); a re-serve reads the ROW's marks and never re-detects.
Deliberately absent: previews (a fetch on someone's behalf), bare domains / mailto / tel, markdown.

**Why:** a link is a consent decision, not a text pattern.
**How to apply:** any new link surface goes thru marks + the consent dialog; the button re-stamps its hit silhouette after the textbox, like send does.
