# Photon Initial Onboarding & Attestation Flow - Full Specification

## Overview

Photon uses a two-attestation system for identity verification. New users must receive vouches from two existing network members to establish their handle. The system is invite-only, privacy-focused, and designed to prevent sybil attacks thru social proof and reputation staking.

---

## Core Principles

1. **Invite-only network** - Cannot join without existing member attestations
2. **Two attestations required** - Must have two independent humans vouch for uniqueness
3. **Unattested = available** - Handles are only reserved when fully attested (2/2)
4. **First to complete wins** - Race conditions accepted as feature, not bug
5. **Privacy by default** - Consenting visibility for handle discovery
6. **Time-bound requests** - 24-hour expiration prevents network clutter
7. **Reputation at stake** - Attesters risk reputation when vouching

---

## Rate Limiting

- **1 invite per hour** per device/install
- **One in-flight invite at a time** - cannot send new invite until current one completes or expires (24h) per install (user has multiple devices? could potentially send a bunch)
- Rate limit only consumed on **successful attestation completion**
- Failed/cancelled attestations do not count against rate limit

---

## New User Flow

### Step 1: Install Photon, Choose Mode

```
Welcome to Photon

Enter desired handle:
[@nick               ]

    [ Generate Attestation Request ]
    [ Add Authorized Device ]
```

Two paths:
- **Generate Attestation Request** - New handle, need attestations
- **Add Authorized Device** - Already have handle, adding new device

---

## Path A: Generate Attestation Request (New Handle)

### Step 2: Specify Attesters

```
Generate attestation request for @nick

First attester:
[@dan                ]

Second attester:
[@alice              ]

    [ Generate Request ]
```

**Rules:**
- Can specify one or both attesters
- Can generate multiple requests with different attester pairs
- Each request gets unique verification words
- All requests expire in 24 hours
- First request to reach 2/2 attestations wins

**After generation:**

```
Attestation request for @nick created

Your verification words:
    turtle piano mountain

Call @dan and @alice and read these words.

---

All requests for @nick:

Request #1: @dan + @alice (just now)
Request #2: @bob + @carol (5 min ago)

All expire in: 23h 58m
```

---

### Step 3: Contact Your Attesters

**You read the verification words to each attester over phone/in-person.**

This proves:
- You possess the invite token/device pin
- You are who the attester thinks you are (voice recognition)
- Timing/context makes sense
- Highly recommended to be in person

---

## Attester Flow (Dan's Perspective)

### Step 1: Dan Checks Requests

```
"Attest a new user"

Enter their requested handle:
[@nick               ]

Network returns with "Nick does need attestation and did assign you to attest"
Attestation requests don't appear, they must be polled. Once polled, request can be reviewed and confirmed/attested

@nick - 2 min ago
Device: Mobile (Android 14), Seattle WA
[ Review Request ]

**Request details visible:**
- Handle being claimed
- Device type and OS
- Approximate location (city level)
- Time since request generated

---

### Step 2: Dan Reviews Specific Request

```
Attestation request for @nick

Request details:
- Device: Mobile (Android 14)
- Location: Seattle, WA
- Requested: 2 minutes ago

[word fields appear]

Enter the words they read to you:

Word 1: [         ]
Word 2: [         ]
Word 3: [         ]

[After all three words correct, button activates]

⚠️ ATTESTATION WARNING ⚠️

You are staking your reputation that:
- This is a unique human being
- They do NOT already have a Photon handle
- You have personally verified their identity

If this person has multiple handles or is not 
a unique human, YOUR reputation will be penalized.

The network relies on attesters to ensure 
one-human-one-handle integrity.

    [ Attest @nick ]    [ Reject ]
```

**Verification process:**
1. Dan types the handle (`@nick`) to confirm who he's attesting
2. Three word fields appear
3. Dan types the three words you read to him
4. After all three words are correct, `[ Attest @nick ]` button activates
5. Dan reads the warning and clicks to complete attestation

**Word verification:**
- 3 attempts maximum per request
- 1 second minimum delay between attempts
- Wrong handle = immediate rejection (doesn't count as attempt)
- 3 failed word attempts = reputation ding for attester

*Cryptographic note: Dan nor dan's device knows what my three words are and must be round tripped to my device for confirmation

---

### Step 3: Network Validation

**When Dan clicks [ Attest @nick ]:**

Network checks:
1. Is the device pin request for `@nick` sent to `@dan` still valid?
2. Do the verification words match the cryptographic binding?
3. Is `@nick` already attested by someone else? (note that the network check for taken handle happens on Dan's side and happens last)

**Case A: Valid request, handle available**
→ Attestation #1 complete, waiting for second attester

**Case B: Handle already fully attested**
```
@nick is already attested

Attested: 2024-03-15
Attesters: [redacted for privacy]

Cannot create new attestation.

If this is the requester recovering their identity:
They should use [ Add Authorized Device ]

    [ Cancel ]
```

**Case C: No valid request found**
```
No attestation request for @nick sent to you

Possible reasons:
- Request expired (24h timeout)
- Request was cancelled
- Handle mismatch

    [ Cancel ]
```

---

## Parallel Attestation Requests

**Users can generate multiple simultaneous requests:**

```
Your pending requests for @nick:

1. @dan + @alice (2 min ago)
   Status: @dan vouched, waiting for @alice
   
2. @bob + @carol (5 min ago)
   Status: Waiting for both
   
3. @eve + @frank (12 min ago)
   Status: @eve vouched, waiting for @frank

All expire in: 23h 55m
```

**First pair to complete 2/2 attestations wins:**
- `@nick` becomes fully attested
- All other pending requests will not complete as they will all fail the taken handle test by attestor
- Failed/cancelled attesters take no reputation hit
- Only successful attesters consume rate limits

---

## Attestation Completion

### When Second Attester Completes:

```
✓ @nick successfully attested!

Attesters: @bob, @carol
Attested: 2025-10-25 14:23:07 ET

Your other pending requests have been cancelled.

    [ Continue to Photon ]
```

**What happens:**
- Handle `@nick` is now cryptographically bound to your keys
- All other pending requests for `@nick` invalidate
- You gain full network access
- Bob and Carol consume one invite from their hourly rate limit (cannot spam attest people, 1 per hour MAX)
- Dan, Alice, Eve, Frank (if any vouched but didn't complete) do NOT consume rate limit

---

## Attestation Failure & Expiration

### After 24 Hours With No Completion:

```
Your attestation request for @nick expired

None of your attesters completed verification
within 24 hours.

Your attesters can try again.

    [ Generate New Request ]
```

**What happens:**
- All pending requests for `@nick` expire
- Handle `@nick` remains available (unattested = claimable)
- Attesters who vouched but didn't complete: no rate limit consumed
- You can generate fresh requests immediately

**Cooldown:**
- None needed - rate limiting handled by "one in-flight invite at a time" rule
- If Dan's invite to you expires, Dan can immediately invite someone else

---

## Path B: Add Authorized Device (Existing Handle)

### Used for:
1. Adding new device when you already have handle
2. Recovering access after losing all devices

```
Add device to @nick

How would you like to authorize?

    [ Clone keys from active device ]
    (You have another device with Photon)

    [ Regenerate keys from trusted parties ]
    (You lost all devices, need key reconstruction)
```

---

### Option 1: Clone Keys From Active Device

```
Add device to @nick

Your verification words:
    jungle crystal hammer

On your other device:
1. Open Photon
2. Go to Settings → Devices → Authorize New Device
3. Enter these three words

Waiting for authorization...
```

**On existing device:**

```
Authorize new device for @nick?

Device: Laptop (Fedora Linux)
Location: Renton, WA
Time: Just now

Enter words from new device:

Word 1: [         ]
Word 2: [         ]
Word 3: [         ]

    [ Authorize ]    [ Reject ]
```

**After authorization:**
- Keys cloned to new device
- New device fully functional
- Both devices can send/receive messages

---

### Option 2: Regenerate Keys From Trusted Parties

```
Regenerate keys for @nick

Contact your key holders to reconstruct 
your identity.

Required: 3 of 5 key holders

○ @alice (pending...)
○ @bob (pending...)
○ @carol (2/5 shards received)
○ @dave (pending...)
○ @eve (pending...)

Waiting for key reconstruction...
```

**Key holder flow:**
- Receives request: "@nick needs key recovery"
- Verifies identity (out-of-band: phone call, video chat, etc.)
- Approves shard release
- After threshold met (e.g., 3 of 5), keys reconstructed

**After reconstruction:**
- Keys rebuilt from shards
- New device has full access
- Identity recovered

---

## Verification Wordlist

### Specifications:
- **Total wordlist size:** 3,177 words
- **Short list (used for verification):** 510 words (frequency-sorted)
- **Phrase length:** 3 words
- **Selection criteria:**
  - Phonetically distinct (no wait/weight/wheat confusion)
  - Visually distinct (high Levenshtein distance)
  - Culturally appropriate (no offensive terms)
  - Easy to pronounce
  - Short (1-2 syllables preferred)

### Security Properties:
- **Total combinations (510³):** 132,651,000
- **Probability of guessing in 3 attempts:** 0.00000226%
- **Time to brute force (1s delay):** ~4.2 years

### Example phrases:
- turtle piano mountain
- jungle crystal hammer
- coffee dragon sunset

---

## Privacy & Handle Discovery

### Handle Visibility:
- **Opt-in discoverability** - handles only searchable if user consents
- **Collision detection only** - network returns "already attested" on exact hash match
- **No enumeration** - cannot probe network to see which handles exist
- **2-hop visibility** - can see consenting handles of friends-of-friends

### Lazy Network Checking:
- Handle availability NOT checked when generating request
- Only checked when attester tries to vouch
- Prevents enumeration attacks
- Reduces network load

---

## Reputation System

### Attesters Stake Reputation:
- Vouching for someone puts attester's reputation at risk
- **Penalties for:**
  - Attesting duplicate handles (same human, multiple accounts)
  - Attesting non-humans or bots
  - Attesting fake/stolen identities
  - 3 failed verification attempts in a row

### No Penalties For:
- Attestation request expiring (attestee didn't follow thru)
- Being part of cancelled parallel request (faster attesters completed first)
- Legitimate recovery/challenge scenarios

### Reputation Recovery:
- Successful attestations increase reputation
- Time-based decay of penalties
- Community review for disputed cases

---

## Cryptographic Bindings

### Device Pin:
- Ephemeral keypair generated per attestation request
- Bound to:
  - Device hardware ID
  - Desired handle (`@nick`)
  - First attester handle (`@dan`)
  - Second attester handle (`@alice`)
- Creates unique request signature
- Prevents request forgery/interception

### Verification Words Derivation:
- Generated from cryptographic material (device pin + handle + attester handles)
- Deterministic but unpredictable
- Cannot be precomputed (session-specific)
- Validates possession of correct device pin

### Attestation Record:
- Permanent cryptographic record of:
  - Handle → Public key binding
  - Attester identities
  - Timestamp (Eagle Time)
  - Device metadata (optional)
- Stored in distributed network
- Immutable once complete

---

## Edge Cases & Error Handling

### Multiple Devices Pin Same Handle Simultaneously:
**No conflict** - each device generates unique request bound to different attesters or device IDs.

Example:
- Phone pins `(@nick, @dan, @alice)`
- Laptop pins `(@nick, @bob, @carol)`
- Both are valid pending requests
- First to reach 2/2 wins

---

### Attester Types Wrong Handle:
```
Handle mismatch

You entered: @nik
Request is for: @nick

    [ Try Again ]    [ Cancel ]
```
Does not count as verification attempt.

---

### Verification Words Wrong (3 Attempts):
```
Incorrect verification words

Attempts remaining: 2

    [ Try Again ]    [ Cancel ]
```

After 3 failures:
```
Verification failed

Too many incorrect attempts.
This attestation request has been rejected.

Your reputation has been penalized.

    [ OK ]
```

---

### Second Attester Never Responds:
After 24 hours, request expires. User can:
- Generate new request with same first attester + different second
- First attester's previous vouch doesn't count (request expired)
- First attester must re-vouch on new request
- No penalty for first attester

---

### Someone Else Completes Attestation First:
```
@nick is no longer available

Someone else completed attestation while 
your request was pending.

Choose a different handle.

    [ Try Again ]
```

Unattested = available. Race conditions accepted.

---

### Attester Revocation:

**Before Attestation:**
- Attester can revoke/cancel request anytime before vouching
- Request disappears from attester's queue
- Requester notified: "Attestation request cancelled by attester"

**After Attestation:**
- Cannot revoke once vouched
- If 1/2 attested, first attester cannot undo their vouch
- Must rely on second attester to reject if needed

---

## First Steps After Attestation

### Successfully Attested:

```
Welcome to Photon, @nick

Your handle is now cryptographically secured.

What would you like to do?

    [ Take First Photo ]
    [ Message Your Attesters ]
    [ Invite Someone ]
    [ Explore Settings ]
```

**Default behavior:**
- Opens to camera (primary use case: encrypted photo messenger)
- Can immediately message attesters (they're auto-added as connections)
- Has limited invites available (rate-limited)

---

## Summary of Key States

### For Handle Requester:
1. **No request generated** - can generate multiple requests
2. **Request pending** - waiting for attestations (0/2, 1/2, or multiple parallel)
3. **Fully attested** - 2/2 complete, handle bound, network access granted
4. **Request expired** - 24h passed, can regenerate
5. **Request cancelled** - attester rejected, can try different attesters

### For Attester:
1. **Request received** - appears in queue, can review
2. **Review in progress** - checking details, entering words
3. **Vouch completed** - attestation recorded, waiting for second attester
4. **Attestation complete** - handle fully bound, rate limit consumed
5. **Request cancelled** - expired/rejected/superseded, no rate limit consumed

---

## Technical Implementation Notes

### Storage:
- Pending requests: Ephemeral (24h TTL)
- Completed attestations: Permanent, immutable
- Verification words: Never stored, derived on-demand
- Device pins: Local only, never transmitted

### Network Queries:
- Attesters query by their own handle (see requests sent to them)
- No global "all pending requests" endpoint
- Hash-based collision detection for privacy
- Eagle Time for timestamp consensus

### Scalability:
- Parallel request generation: unlimited
- Active requests per handle: unlimited (all expire in 24h)
- Rate limiting prevents spam at attestation completion
- Network load proportional to successful attestations, not attempts

---

Who can find my handle in search?

○ Anyone (public directory)
○ Friends-of-friends only (2-hop visibility)
○ Nobody (completely private, must share friend code)
```

**Anyone:**
- @nick appears in public search
- Strangers can find you
- Useful for: public figures, businesses, open networking

**Friends-of-friends:**
- Only visible to people within 2 hops of your social graph
- Alice (friend) → Bob (Alice's friend) can see you
- Random stranger can't
- Useful for: most normal people

**Nobody:**
- Not searchable at all
- Must share friend code manually
- Useful for: privacy-focused users, celebrities avoiding fans

---

### Setting 2: Who can send initial message / friend request
```
Who can contact me?

○ Anyone who can find me
○ Friends-of-friends with introduction
○ Friends-of-friends only (auto-accept)
○ Nobody (must share friend code)
```

**Anyone who can find me:**
- Bob finds @nick in search
- Bob can send friend request with message
- Nick approves/rejects

**Friends-of-friends with introduction:**
- Bob (Alice's friend) wants to message Nick (Alice's friend)
- Bob must ask Alice to introduce them
- Alice sends: "Nick, this is Bob, he's cool"
- Nick sees introduction, can accept

**Friends-of-friends only:**
- If Bob is friend-of-friend, auto-accept
- No request needed
- Trust is transitive

**Nobody:**
- Can't be contacted unless you share friend code
- Maximum privacy

---

### Setting 3: Friend request approval
```
Friend request handling:

○ Manual approval (I review each request)
○ Auto-accept from friends-of-friends
○ Auto-accept with reputation threshold (>500 rep)
○ Disabled (friend code only)
```

**Manual approval:**
```
🔔 Friend request from @bob

⚪ @bob (attested by @dan, @eve)
Reputation: 847
Mutual connections: @alice

Message: "Hey Nick, Alice suggested I reach out!"

    [ Accept ]    [ Reject ]    [ Block ]
```

You decide case-by-case.

**Auto-accept from friends-of-friends:**
- Bob is Alice's friend, you're Alice's friend
- Automatically accepted
- Shows up in your contacts

**Auto-accept with reputation threshold:**
- Only users with >500 reputation can auto-add
- Prevents new/low-quality accounts
- Still requires some vetting

---

### Setting 4: Message-before-friending
```
Can people message me before being friends?

☑ Allow initial message with friend request
☐ Friend request only, no message until accepted
```

**Allow initial message:**
```
Friend request from @bob

"Hey Nick, we met at the conference last week.
Want to grab coffee and talk about Spirix?"

    [ Accept & Reply ]    [ Reject ]
```

You see their message before deciding.

**Friend request only:**
```
Friend request from @bob

⚪ @bob (attested by @dan, @eve)
Reputation: 847
Mutual connections: @alice

    [ Accept ]    [ Reject ]
```

No message until you accept. Cleaner, but less context.

---

## The introduction flow (friends-of-friends with introduction)

### Bob wants to message Nick, both are Alice's friends:

**Bob → Alice:**
```
"Hey Alice, can you introduce me to Nick?
I want to talk to him about his floating point work."
```

**Alice → Nick (via Photon):**
```
Introduction request from @alice

Alice wants to introduce you to @bob

@bob (attested by @dan, @eve)
Reputation: 847

Alice's message:
"Nick, this is Bob. He's working on similar math
stuff and I think you two would hit it off."

    [ Accept Introduction ]    [ Decline ]
```

**If Nick accepts:**

**Alice → Bob:**
```
Nick accepted the introduction!
You can now message him.
```

**Bob can now message Nick directly.**

**Benefits:**
- Warm introduction (not cold contact)
- Alice vouches for both parties
- Social pressure for good behavior
- Reputation on the line (Alice's, Bob's, Nick's)

---

## Friend code sharing (for private users)

### Nick's setting: "Nobody can contact me"

**Nick wants to give access to specific person:**
```
Settings → Friend Code

Share this code to allow someone to add you:

photon://friend/8f4a92c1abc...

Or QR code:
[QR code displayed]

This code:
☑ Never expires
☐ Single use only
☐ Expires in 7 days

    [ Generate New Code ]    [ Revoke All Codes ]
```

**Nick shares code with Bob (via email, text, in-person):**

**Bob scans/clicks code:**
```
@nick wants to connect

⚪ @nick (attested by @dan, @alice)
Reputation: 1,247

    [ Add Friend ]    [ Ignore ]
```

**Bob clicks Add Friend:**

**Nick gets notification:**
```
🔔 @bob used your friend code

⚪ @bob (attested by @dan, @eve)
Reputation: 847

    [ Accept ]    [ Reject ]
```

**Even with friend code, Nick still approves.**

---

## Reputation-based filtering

### Nick's setting: "Auto-accept >500 reputation"

**High-rep user Bob requests:**
```
@bob (reputation: 847) wants to add you

Auto-accepted based on reputation threshold.

    [ View Profile ]    [ Remove ]
```

**Low-rep user Charlie requests:**
```
🔔 Friend request from @charlie

⚪ @charlie (attested by @frank, @grace)
Reputation: 127 (below your threshold)

Message: "Saw your Spirix work, would love to chat!"

    [ Accept ]    [ Reject ]
```

Charlie needs manual approval despite friend request because reputation is low.

---

## Mutual connections display

**When viewing friend request:**
```
Friend request from @bob

⚪ @bob (attested by @dan, @eve)
Reputation: 847

Mutual connections: (3)
    @alice (your friend since 2024-03-15)
    @carol (your friend since 2024-08-22)
    @dave (your friend since 2025-01-10)

Common interests:
    - Rust programming
    - Cryptography
    - Photography

    [ Accept ]    [ Reject ]    [ View Full Profile ]
```

**You can see:**
- Who you both know
- How long you've known them
- Common ground

**Helps decide:** "3 mutual friends? Probably legit."

---

## Block/Report

**If someone is harassing or spamming despite being attested:**
```
@bob is harassing you

    [ Block ]    [ Report to Network ]
```

**Block:**
- Bob can no longer message you
- Bob can't see you're online
- Bob removed from your contacts
- Revoke Bob's message access to your devices

**Report:**
- Network sees: "@bob reported by @nick for harassment"
- Bob's reputation takes hit
- Multiple reports → Bob's attesters lose reputation
- Pattern of reports → Bob's account flagged
- Bob's attesters might revoke their attestation

**Social pressure:** If Bob harasses enough people, his attesters' reputations suffer, they revoke him, he loses access.

---

## Example configurations:

### Public figure (wants reach):
```
Discoverability: Anyone
Contact: Anyone who can find me
Approval: Manual (review all requests)
Message-before-friending: Yes
```

### Normal person (balanced):
```
Discoverability: Friends-of-friends only
Contact: Friends-of-friends with introduction
Approval: Manual
Message-before-friending: Yes
```

### Privacy-focused (locked down):
```
Discoverability: Nobody
Contact: Nobody (friend code only)
Approval: Manual
Message-before-friending: No
```

### Open networker (maximum connection):
```
Discoverability: Anyone
Contact: Anyone who can find me
Approval: Auto-accept >500 reputation
Message-before-friending: Yes

## YES. The arbitrator model.

---

## How it works:

### During initial setup (when you first join Photon):

```
Set up device authorization

Who can approve your new devices?

This person will be notified when you try to
add a device and can approve or reject.

Choose your arbitrator:
[@wife               ]

    [ Set Arbitrator ]
```

**You designate ONE person** (wife, best friend, business partner, whoever) as your device authorization arbitrator.

---

## Adding a device - the flow:

### You: "I want to add my smartwatch"

```
[Your phone]

Add new device via Bluetooth

Scanning...

Found: Smartwatch (WearOS)

Device identifier:
    nebula-transcription-helicopter

Notifying your arbitrator (@wife)...
```

---

### Wife gets notification:

```
🔔 @nick wants to add a device

Device: Smartwatch (WearOS)
    nebula-transcription-helicopter

Location: Home
Time: 2:47 PM

    [ Approve ]    [ Reject ]    [ Ask Nick ]
```

Wife texts you: "hey man, you buy a fucking watch without asking???"

You: "yeah, got the new WearOS one, it's dope"

Wife: *clicks Approve*

---

### Authorization completes:

```
[Your phone]

✓ Device authorized by @wife

Smartwatch added successfully.

    [ Done ]
```

**No passwords. Just "did wife say yes?"**

---

## Why this works:

### 1. **Social verification at human speed**
- Wife knows your voice, your patterns, your behavior
- Can call/text you to verify: "did you actually buy a watch?"
- Much harder to fool than any password

### 2. **Out-of-band confirmation**
- Attacker steals your phone
- Tries to add their laptop
- Wife gets notification
- Wife texts you: "adding a laptop?"
- You: "WTF no, I'm at the airport, phone got stolen"
- Wife: *clicks Reject*

### 3. **Insider attack resistance**
- If WIFE steals your phone and tries to add device
- She can't approve her own request (conflict of interest detected)
- Falls back to: contact YOUR other arbitrator or key holders

### 4. **No password needed**
- You trust your wife
- She trusts you
- That's the security model
- Cryptography ensures she actually said yes (signatures)
- But decision is human: "does this seem like Nick?"

---

## Attack scenarios:

### Attacker steals phone at airport:

**Attacker:** Tries to add laptop

**System:** Notifies wife

**Wife:** Gets notification at home, texts you

**You:** "Phone stolen!"

**Wife:** Rejects authorization

**Attacker:** Locked out

---

### Sophisticated attacker intercepts wife's notification:

**Attacker:** Steals phone, somehow MITMs wife's Photon

**System:** Sends authorization request

**Attacker's fake notification:** Shows "approve" button

**Wife:** Clicks, but...

**Problem:** Wife's approval is cryptographically signed with HER keys. Attacker doesn't have wife's keys. Can't forge signature.

**Result:** Approval rejected as invalid signature.

---

### Wife is compromised:

**Attacker:** Compromises wife's device

**Attacker:** Approves rogue device authorization

**Problem:** Now attacker has access to your account

**Mitigation:** 
- Audit log shows wife approved suspicious device
- You can revoke both the rogue device AND change arbitrator
- Social recovery: contact your key holders, explain wife's account was compromised

**This is STILL better than passwords** because:
- Attacker needs to compromise TWO people (you + wife)
- Audit trail shows who approved what
- You can recover via other social connections

---

## Multiple arbitrators / threshold:

### For paranoid users:

```
Set up device authorization

Require approval from:

○ 1 of 1 arbitrator (fast, convenient)
● 2 of 3 arbitrators (more secure)
○ 3 of 5 arbitrators (maximum security)

Choose your arbitrators:
[@wife               ]
[@bestfriend         ]
[@brother            ]

    [ Set Arbitrators ]
```

**Attacker now needs to:**
- Steal your phone AND
- Compromise 2 of your 3 arbitrators

Much harder.

---

## Arbitrator responsibilities:

```
@wife is your device arbitrator

This means:
- She'll be notified when you add devices
- She can approve or reject
- She should verify it's really you before approving

You are @bob's arbitrator

This means:
- You'll be notified when Bob adds devices
- You can approve or reject
- You should verify it's really Bob before approving
```

**Mutual arbitration is common:**
- You're wife's arbitrator
- Wife is your arbitrator
- You protect each other

---

## The "I'm solo, no spouse/friends" case:

### Option A: Designate a trusted contact anyway
- Parent
- Sibling
- Close friend
- Business partner

**Someone who knows you well enough to verify.**

### Option B: Time-delayed self-approval
```
No arbitrator set

New device authorizations will have a 24-hour delay.

You can cancel from any existing device during
this window.

    [ Set Delay Period ]
```

**Self-arbitration with time for you to notice and react.**

### Option C: Community arbitration
```
No personal arbitrator set

Your device authorizations will be reviewed by
trusted community members based on:
- Your reputation score
- Behavioral patterns  
- Network consensus

Approval time: 1-6 hours

    [ Use Community Arbitration ]
```

**Decentralized social proof.**

---

## Arbitrator can be changed:

```
Settings → Security → Arbitrator

Current arbitrator: @wife

    [ Change Arbitrator ]
```

**But requires:**
- Current arbitrator approval (wife must approve you changing to someone else)
- OR multi-device consensus (all your devices agree)
- OR social recovery (key holders verify)

**Prevents attacker from changing arbitrator to themselves.**

---

## The beauty of this system:

### Zero knowledge required:
- No password to remember
- No PIN to forget
- No biometric to fake

### All trust, no secrets:
- You trust wife
- Wife trusts you
- Network verifies wife actually said yes (crypto signatures)
- But decision is human judgment

### Auditable:
```
Device authorization history:

💻 Work Laptop
   Approved by: @wife
   Date: 2025-10-20

⌚ Smartwatch  
   Approved by: @wife
   Date: 2025-10-23

📱 Backup Phone
   Approved by: @wife
   Date: 2025-10-25
```

You can see who approved what, when.

### Revocable:
- Wife's account compromised? Change arbitrator.
- Device stolen? Revoke it.
- Suspicious approval? Investigate and revoke.

---

## This is it. This is how you do passwordless device authorization.

**Social trust + cryptographic verification.**

Wife doesn't need to know your password. She just needs to know YOU.

And when she clicks "Approve," the network cryptographically verifies that SHE actually clicked it (signature verification).

**No passwords. No PINs. Just "did wife say it's cool?"**

---

## Just remember: Passless (adjective)

**Definition:**

Authentication without memorized secrets or required knowledge.

Identity is verified through cryptographic proof and social attestation rather than passwords, PINs, security questions, or any information the user must remember. While the user may enter codes or perform verification steps, all necessary information is provided in the moment—nothing must be recalled from memory. Trust derives from human relationships and distributed social verification.

**The amnesia test:** A passless system allows authentication even after complete memory loss, as identity verification depends on relationships with trusted contacts rather than recalled credentials.

---

## Usage examples:

"Photon's passless authentication means your wife can verify your identity and restore access to your account—no passwords to remember or forget."

"In a passless system, losing your memory doesn't mean losing your identity; the people who know you can vouch for who you are."

"Unlike passwordless systems that replace passwords with magic links or biometrics, passless systems eliminate secret credentials entirely, relying instead on social trust networks."

---

## Etymology:

pass (credential requiring memory/knowledge) + -less (without)

Distinct from "passwordless" (a marketing term often describing systems that still require secrets, just not traditional passwords).

---

**Oh, and A=1. Always.**

## End of Specification