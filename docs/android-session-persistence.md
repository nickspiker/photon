# Android session persistence — the boot-locked identity capsule

## The problem

On Android the app de-attests on a plain app restart.
The only session persistence today is a sticky broadcast, and sticky broadcasts **die with the app on Samsung** (tested) — and almost certainly on other aggressive-process-manager OEMs (Xiaomi, Oppo, Huawei).
Underneath, `tohu`'s session file falls back to the temp dir on Android (there is no `XDG_RUNTIME_DIR`), which is wiped on restart, so `tohu::session()` returns `None` → the launch auto-resume in [`photon_app.rs`](../src/ui/photon_app.rs) at the `tohu::session()` check drops to the attest screen → de-attest.
The blank avatar/orb are downstream of that: no attestation → no `handle_proof` → nothing keyed on `identity_seed` loads.

## The model: the device is the credential, power is the boundary

There is no password.
The **handle** is a public username (memorable, e.g. `CactusGardener`), not a secret.
The **device** is the credential: only this device, with its `device_secret` (the software stand-in for the *ira*, today `BLAKE3(ANDROID_ID)`), can attest as that handle.
The security boundary is the **power rail**, enforced by the *wairua* — the per-boot secret that lives only for the life of the boot (GLOSSARY: "held in volatile registers until power interruption").

Consequences, all falling out of one design:
- **App restart → seamless resume.** Same boot → same *wairua* → the capsule decrypts. Works on every device, no permission, no broadcast.
- **Reboot → re-type the handle.** The *wairua* dies → the capsule is undecryptable → attest → re-derive the roots from the typed handle (~1 s memory-hard recompute) → the device re-proves itself. This is the "logout on reboot", and it is the feature, not a cost — a user reboots ~weekly, so it is one short handle entry a week versus a password every day.
- **Reinstall → resume without re-typing**, if a survives-reinstall tier held the capsule for this boot (see tiers).

## The capsule

The session is the 96-byte `SessionIdentity` = `identity_seed ‖ vault_seed ‖ handle_proof` (register-shaped, the *proof* — never the plaintext handle string).

Sealing:
1. Wrap the 96 roots in a versioned VSF envelope.
2. AEAD-encrypt with `kete::encrypt_bytes(roots, &wairua)` — ChaCha20-Poly1305, nonce‖ct‖tag.
   The Poly1305 tag is what makes a failed open an unambiguous `Err`, not 96 bytes of plausible garbage — the whole "fail → attest" flow depends on that crispness.
3. For exposed tiers only, append a `device_secret` keyed MAC over the ciphertext (`BLAKE3::keyed_hash(device_secret, ciphertext)`), so a world-writable tier cannot be used to plant a forged identity.
4. Wrap `{ version, mode, sealed_blob, mac? }` as the VSF capsule.

Opening is the reverse: parse → (verify MAC if exposed) → `kete::decrypt_bytes` → parse roots.
Any failure (VSF parse, MAC mismatch, AEAD auth failure = wrong *wairua* = reboot, or tamper, or corruption) returns `None`.
The three failure causes are indistinguishable and the response is identical, so there is nothing to branch on: `None` → attest → overwrite every tier.

## The wairua (per-boot key)

- **Local tiers:** `wairua = ihi::spaghettify(boot_id)`.
- **Exposed tiers:** `wairua = ihi::spaghettify(boot_id ‖ device_secret)` — a leaked capsule then cannot be brute-forced offline without the device's ANDROID_ID root, and it is `device_secret`-MAC'd anyway.

`boot_id` is `/proc/sys/kernel/random/boot_id`: a 128-bit UUIDv4 (~122 bits from `get_random_bytes`), fresh and non-monotonic per boot, gone on reboot.
It is world-readable and that is fine — the boundary is power, not secrecy; on a live device the identity is *meant* to be readable, and a cold/locked device is protected by FBE (the ciphertext is unreadable) regardless of `boot_id`.
Entropy caveat: `boot_id` is materialised at first read and can be weakly seeded if read before the RNG is up; in practice it is seeded by the time an app reads it post-boot, and mixing `device_secret` on the exposed tier covers the residual.

**Source, decided by an up-front SELinux check (implementation step 0):** if the `untrusted_app` domain can read `/proc/sys/kernel/random/boot_id`, `tohu` reads it directly.
If a ROM's SELinux policy blocks it, the fallback is a random 32-byte *wairua* generated once per boot and stashed in the sticky broadcast (dies on reboot, survives restart), handed to `tohu` via `set_wairua_override`.

### Considered and rejected: a separate XOR pad

A random XOR pad layered on top of the AEAD adds nothing if it is derived from or stored with the *wairua*.
The AEAD already gives "right key → right value, wrong key → fail" (128-bit Poly1305, ~2⁻¹²⁸ false-accept), and XORing by a value derivable from the same secret is a no-op — anyone who can attempt the decrypt already holds the pad.
The only version that would add security is an *independent* secret the attacker lacks, held in a more-protected tier — and that is exactly what `device_secret` already is on the exposed-tier key `spaghettify(boot_id ‖ device_secret)`: a shared-media attacker who reads the public `boot_id` still cannot open the capsule without the app-private ANDROID_ID root.
So the "a successful-looking decrypt still won't yield the value without a second secret" property is already present for the exposed tier, done in the KDF where it belongs; a standalone pad would be `device_secret` with extra steps and its own durable-storage problem.

## Sizes (precise)

- *wairua* / key: **256-bit** (ChaCha20 key; our `spaghettify(...) → [u8;32]` drops straight in).
- Auth tag: **128-bit** Poly1305 — the integrity check that fails on a wrong *wairua*; ~2⁻¹²⁸ false-accept, so a reboot's new `boot_id` fails the tag with certainty for any practical purpose.
- Nonce: **96-bit** random per seal (fresh each write; no reuse concern for a capsule written a handful of times).
- No AAD is bound today; if the `version`/`mode` header should be cryptographically bound rather than just wrapped, that is the one-line place to add it.

## Storage tiers

Because the capsule is inert boot-locked ciphertext, we write it to every tier that will hold it and, on launch, try each in order until one opens.
The union of their survival properties is the resilience.

| Tier | Permission | Survives | Key / seal |
|---|---|---|---|
| `filesDir` (internal app-private) | none | app restart | `spaghettify(boot_id)`, authoritative (sandbox write-protected) |
| external shadow (`getExternalFilesDir`) | none | app restart (adb-pullable) | `spaghettify(boot_id)` |
| sticky broadcast | none (install-time only) | reinstall, *same boot*, **only where the OEM keeps it** (Samsung: no) | `spaghettify(boot_id ‖ device_secret)` + MAC |
| shared media (SAF) | one user file-pick | reinstall **and** full uninstall, same boot | `spaghettify(boot_id ‖ device_secret)` + MAC |

Sticky broadcast is demoted to a zero-permission garnish — never load-bearing, since `filesDir` covers restart on every device and SAF covers reinstall reliably.
Cloud backup is deliberately excluded: a cloud-restored capsule is almost always cross-boot, so it fails to decrypt (zero resume gain), and it ships ciphertext off-device for nothing.
Real cloud identity survival is a separate, non-boot-locked, recovery-secret-wrapped feature — that is **custodes**, down the road with the FGTW split.

## Flows

**On attest success** (extends the `QueryResult::Success` handler that already sets `pending_broadcast_signal = 1`):
1. `tohu::set_session(&roots)` → seals `Local` and writes `filesDir` + external shadow.
2. Signal the sticky broadcast (Kotlin `sendSessionBroadcast`) — now carrying the sealed `Shared` capsule, not raw roots.
3. If the SAF backup is enabled, write the sealed `Shared` capsule to the persisted URI.

**On launch** (extends the existing `tohu::session()` auto-resume):
1. Try `filesDir` → external shadow via `tohu::session()` (`Local` open).
2. If `None`, try the sticky broadcast bytes and the SAF URI bytes via `tohu::open_session(bytes, Shared)`.
3. First success → `tohu::set_session` into the local tiers (rehydrate for next time) → resume paints Ready immediately.
4. All `None` → attest (existing path, untouched) → and re-seal every tier on the next success.

## Crate boundaries

- `ihi::spaghettify(&[u8]) -> [u8; 32]` — exists, no change.
- `kete::encrypt_bytes` / `decrypt_bytes` (ChaCha20-Poly1305) — exist, no change.
- **`tohu`** — the change: capsule seal/open, *wairua* derivation, `boot_id` read (+ override), and the local file tiers.
- **`photon`** — the orchestration: the attest-success writes, the launch try-order, and the platform tiers (sticky broadcast via JNI, SAF via Kotlin).
- **Desktop is unchanged** — its `XDG_RUNTIME_DIR` tmpfs already gives "survives restart, dies at logout/reboot" for free; the capsule crypto is platform-agnostic and can be adopted there later if wanted.

## tohu API surface

```
pub enum SealMode { Local, Shared }              // Local = boot_id only; Shared = boot_id ‖ device_secret + MAC

pub fn seal_session(s: &SessionIdentity, mode: SealMode) -> Option<Vec<u8>>   // None if no wairua source
pub fn open_session(bytes: &[u8], mode: SealMode) -> Option<SessionIdentity>  // None on any parse/MAC/AEAD failure

pub fn set_session_dir(dir: &Path)               // Android: point the local file tiers at filesDir (from JNI)
pub fn set_wairua_override(w: &[u8; 32])          // fallback when /proc boot_id is SELinux-blocked

// session() / set_session() / clear_session() stay, repointed on Android to the sealed local-tier capsule
```

`open_session` returning `None` on failure is what keeps photon's existing "`None` → attest" path untouched — no photon control-flow change, only added tier writes and an extended launch try-order.

## Permissions and graceful degradation

Zero permissions, zero user interaction gives the case that matters: **app-restart resume works on every device, forever, no prompt** (`filesDir` + shadow).
Only reinstall/uninstall-without-re-typing is gated, behind a single SAF file-pick (not a scary blanket permission).

The optional backup is offered up front and is fully decline-friendly — declining costs nothing on reboot (you re-type the handle either way, by design) and only means a reinstall asks for your handle once.

User-facing copy:

> **No password. Your device is your key.**
> Stay signed in as long as your phone is on — restart the app all you want, you are still you.
> Turn it off or reboot and you will just enter your handle again to sign back in.
> Nothing to remember but your name, nothing to leak.

> **Back up your identity?** So reinstalling Photon does not ask for your handle again.
> It is an encrypted file only this phone can read — no account, no server, nothing leaves your device.
> **[Save it]  [Not now]** — Not now is fine; everything works, you will just type your handle once if you ever reinstall.

The forgot-your-handle-and-rebooted-with-no-backup lockout is the recovery case, filled by **custodes**.

## Implementation order

0. **SELinux check** — confirm whether the `untrusted_app` domain can read `/proc/sys/kernel/random/boot_id`; decides `boot_id`-in-Rust vs the Kotlin `set_wairua_override` fallback.
1. **tohu capsule crypto** — `SealMode`, `seal_session`, `open_session`, *wairua* derivation, with round-trip + wrong-wairua-fails + MAC-tamper-fails tests.
2. **tohu local tiers** — `set_session_dir`; repoint `session`/`set_session`/`clear_session` to the sealed capsule in `filesDir` + shadow.
3. **photon** — JNI passes the data dir to `tohu`; extend the attest-success handler (write local tiers) and the launch try-order (local → platform).
4. **sticky broadcast** — carry the sealed `Shared` capsule (send + restore paths); it is now inert ciphertext, so its OEM flakiness is only a "might re-attest after reinstall" matter, never a security one.
5. **SAF optional backup** — the up-front decline-friendly prompt, the file-pick, the persisted URI, write-on-attest and read-on-launch.
6. **Decide whether the sticky broadcast earns its JNI complexity** at all, given `filesDir` + SAF cover restart and reinstall more reliably.
