# Device Attestation Parity Act

Draft statutory language and legislator brief. Washington State.

---

## Part 1: The one page a legislator reads

### The problem in one sentence

Companies are refusing to deliver goods and services people already paid for, based not on whether the customer's device is secure, but on whose software is running on it.

### What happened

A Washington father bought concert tickets for his daughter from Ticketmaster. Paid in US dollars. Real purchase, real seat, real person.

The ticket would not render on her phone.

Her phone runs GrapheneOS, a security-hardened Android built on Google's own Pixel hardware. Verified boot intact. Hardware security chip functioning. Smaller attack surface than the factory software. By every measurable property, more secure than the phone next to her in line.

The check that blocked her does not measure any of that. It measures whether Google signed the operating system. It returns the same answer for a hardened phone as it does for one riddled with malware: not ours, denied.

Her father is a telecommunications engineer. He had to write barcode recovery software to extract the ticket. Nobody else in that line could have done that. Everyone else in her position simply does not go.

### The pattern

This is not one company or one bad afternoon.

Two systems, same architecture:

1. **Device attestation.** Access to payments, tickets, banking, and transit is conditioned on running vendor-approved software. A device that can cryptographically prove its boot chain is intact is rejected anyway, because the proof does not trace to the right company.

2. **Developer registration.** Software must be tied to an identity on file with a private company before it can install normally. The company states plainly that it does not review the software itself.

Neither measures security. Both measure provenance and call it security.

The second one fails on its own terms. Identity checks stop people who will not lie. They do not stop people who will. A fraud operation absorbs a document check and a small fee as a cost of business. An honest developer without a government ID, without an accepted payment method, or without a jurisdiction the company operates in, is simply excluded. The filter selects on honesty, not on safety.

### What this bill does

It does not ban security checks. Businesses keep every verification capability they have today.

It requires one thing: **if you condition access on a security property, you must accept a valid cryptographic proof of that property, no matter who issued it.**

Verify all you want. You may not reject a proof that validates because of whose name is on it.

### Why it passes

There is no public argument against it.

A business that objects has to say out loud: we reject this proof because it is not ours. That sentence concedes the check was never about security.

The bill also cannot be attacked as a fraud giveaway, because a compromised device cannot produce a valid hardware-rooted proof. It fails on the merits. That is the entire point. The bill restores merit-based evaluation and removes provenance-based exclusion.

### Who it protects

Every Washington resident who bought something and could not receive it. Every developer excluded from distribution for lacking documents rather than lacking skill. Every person running secure software who is treated as a threat for doing so.

---

## Part 2: Draft statutory language

**AN ACT relating to consumer access to purchased goods and services conditioned on device software provenance; adding a new chapter to Title 19 RCW.**

### Section 1. Findings and intent

The legislature finds that:

(1) Consumers increasingly receive goods and services they have purchased thru software running on personal computing devices, including event tickets, transit passes, payment credentials, boarding documents, and access credentials.

(2) Certain businesses condition delivery of these purchased goods and services on verification systems that evaluate the origin of software running on a consumer's device rather than the security properties of that device.

(3) These systems reject devices that can demonstrate, thru hardware-backed cryptographic attestation, security properties equal to or exceeding those of accepted devices.

(4) The result is that consumers who have paid for goods and services are denied delivery of them for reasons unrelated to security, fraud, or any legitimate business purpose.

(5) The legislature intends to preserve the ability of businesses to verify device security while prohibiting the use of such verification as a mechanism to exclude devices based on software vendor identity.

### Section 2. Definitions

For purposes of this chapter:

(1) **"Attestation"** means a cryptographically signed statement about the configuration, integrity, or security properties of a computing device.

(2) **"Covered business"** means a person or entity that, in the course of business, conditions a consumer's access to a purchased good, a purchased service, or an essential digital service on the results of a device attestation.

(3) **"Essential digital service"** means a payment service, ticketing service, transportation credential, financial account access service, government service interface, or communications service.

(4) **"Hardware root of trust"** means a cryptographic key or certificate provisioned into device hardware by the device manufacturer at the time of manufacture, and not modifiable by the device owner or by software running on the device.

(5) **"Security property"** means a verifiable technical characteristic of a device relevant to its integrity, including but not limited to: the integrity of the boot chain, the presence of an unmodified bootloader lock state, the confinement of application processes, the integrity of cryptographic key storage, and the absence of unauthorized privilege escalation.

(6) **"Valid attestation"** means an attestation that:
   (a) chains to a hardware root of trust;
   (b) has not expired and has not been revoked; and
   (c) asserts the security property that the covered business requires.

### Section 3. Prohibited conduct

(1) A covered business that conditions access on the presence of a security property shall not deny access to a consumer whose device presents a valid attestation of that security property.

(2) A denial is prohibited under subsection (1) regardless of:
   (a) the identity of the vendor, developer, or distributor of the operating system or software on the device;
   (b) whether the operating system was signed by, licensed by, or certified by the covered business or any affiliate of the covered business;
   (c) whether the operating system appears on a list maintained by any private entity; or
   (d) the identity of the entity that issued the attestation, provided the attestation chains to a hardware root of trust.

(3) A covered business shall not condition delivery of a good or service already purchased by a consumer on the consumer installing, running, or maintaining software from a particular vendor, where the consumer's device presents a valid attestation of the security properties the business requires.

(4) A covered business shall not condition the installation or operation of software on a consumer's device on the software's developer having registered an identity with the covered business or any affiliate, where the consumer has affirmatively chosen to install that software.

### Section 4. Permitted conduct

Nothing in this chapter prohibits a covered business from:

(1) requiring attestation of any security property, provided that valid attestations of that property are accepted regardless of source;

(2) denying access to a device that fails to present a valid attestation;

(3) denying access to a device presenting an attestation that has been revoked, has expired, or does not chain to a hardware root of trust;

(4) publishing the specific security properties it requires;

(5) taking action against fraud, unauthorized access, or violation of terms of service on grounds independent of software vendor identity; or

(6) declining to provide technical support for configurations it does not test.

### Section 5. Disclosure

(1) A covered business that conditions access on device attestation shall publish, in a form accessible to the public and free of charge:
   (a) the specific security properties required;
   (b) the technical means by which a device may attest to those properties; and
   (c) a point of contact for reporting a rejected valid attestation.

(2) A covered business shall respond to a report under subsection (1)(c) within thirty days.

### Section 6. Enforcement

(1) A violation of this chapter is an unfair or deceptive act in trade or commerce and an unfair method of competition for purposes of applying chapter 19.86 RCW.

(2) A consumer injured by a violation of this chapter may bring an action for actual damages, injunctive relief, and reasonable attorneys' fees and costs.

(3) The attorney general may bring an action to enforce this chapter.

### Section 7. Construction

(1) This chapter shall be liberally construed to effectuate its remedial purpose.

(2) The rights and remedies in this chapter are in addition to any other rights and remedies available under law.

(3) Any waiver of the provisions of this chapter is void and unenforceable.

### Section 8. Severability

If any provision of this act or its application to any person or circumstance is held invalid, the remainder of the act or the application of the provision to other persons or circumstances is not affected.

---

## Part 3: Drafting notes

Points a committee will raise, and where the draft already answers them.

**"This legalizes rooted phones and lets fraud thru."**

It does not. Section 2(6) requires the attestation to chain to a hardware root of trust and to actually assert the property. A compromised device cannot produce that. It fails the check on the merits. This is the load-bearing definition in the entire bill and it is where opposing testimony will concentrate. Do not let it get weakened.

**"Businesses need to control their own security posture."**

They keep it. Section 4 is deliberately generous. Every capability survives except one: refusing a proof that validates because of who issued it.

**"Federal preemption / dormant commerce clause."**

Expected. My Health My Data faced the same and has held so far. Section 8 severability matters here. The consumer-delivery provisions in Section 3(3) are the most defensible and should be drafted to stand alone if Section 3(4) is struck.

**"Who decides what counts as a security property?"**

Section 2(5) enumerates rather than delegates, on purpose. Delegating to an agency invites capture by the parties being regulated. Enumeration can be amended by the legislature as technology moves.

**Section 3(4) is the most aggressive provision.**

It targets developer registration mandates directly. It will draw the heaviest opposition and it is the most likely to be cut in committee. Consider whether to introduce it in the same bill or hold it for a second session. The consumer-delivery provisions can pass without it. It cannot pass without them.

**The story goes first.**

The rule is broad. The testimony is not. Open with the ticket, the daughter, the money already paid. Get to the statute after the room already agrees something is wrong.

Ends: two.