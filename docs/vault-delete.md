# What "delete" means, per substrate

Settled 2026-09-08 (Nick's verdict, after the SSD/CoW walk-thru). One sentence: **on every target we currently ship, delete is space reclamation — the security boundary is the boot-gated vault key, not erasure.**

## The physics

A zero-overwrite is a TRUE secure erase only where the storage engine commands raw NAND and erased-state == zero — the ferros-on-owned-flash design in manifestus VAULT.md. Nowhere we ship qualifies:

- **Host files (BTRFS/ext4)**: CoW filesystems write the "overwrite" as a NEW extent; every rolling snapshot keeps the old ciphertext for its retention window.
- **Any vendor FTL** (every phone, every SSD, **including Pixel-ferros** — we do not command that NAND stack and won't for years): the controller remaps the LBA; the old page survives until its garbage collection feels like it.

So the zero-write erased nothing, freed nothing (the COW-unlink + plow-reap is what reclaims slots), and cost a full-value write of wear per delete. It is now the `ZeroIsErase` arm of `manifestus::DeletePolicy`, chosen by no current embedder.

## The enforcement (the can't-forget mechanism)

`manifestus::DeletePolicy` is a **required open parameter with no default**. Every embedder — kete's host profile today, a kete-on-owned-NAND profile someday — must write `UnlinkOnly` or `ZeroIsErase` at its open site, next to an enum doc that carries this whole argument. Porting to glyph-class hardware where we own the flash? The compiler makes you read this and choose. That is the reminder: not a comment, a parameter.

## The security story, stated honestly

- At rest, everything is ciphertext under per-address keys derived from the vault master, which exists only after the handle is typed at boot. Snapshots, FTL remnants, stolen disks: ciphertext.
- **Deleting a value does not shred it** while snapshots hold its generations — claiming otherwise would be a lie (the BTRFS window is days). Deletion reclaims space and removes the value from the live index.
- StrongBox/TPM instant-shred was considered and rejected (irritation > value here).
- **True BOOP** — instant, hardware-guaranteed shred — arrives with ferros on glyph, where `ZeroIsErase` becomes both correct and free.

## Record-by-default durability (the same decision's other half)

A wave records like an email saves: the per-call spool key persists in the vault at call START (`call.spool.<id8>` register), so a battery death mid-wave recovers at next launch (`spool::recover_orphans` → the normal keep-transcode). Delete-later is the vault's UnlinkOnly reclaim. Ordering law: blob stored → register dropped → file removed; every crash window resolves at recovery (file+register = re-finish, idempotent by content hash; file-without-register = stray, deleted).
