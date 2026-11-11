// The language catalog: every user-facing string is a Msg variant, every language is one exhaustive match, and the compiler is the completeness checker.
// Doctrine (docs/languages.md): keys are WHOLE messages with holes (never concatenate translated fragments); variants carry semantic values (counts, names) and each language renders its own word order, plurals, and gender.
// Never translated: handles (byte-precise), voca pairing words (protocol material), log lines (photonlog grep-ability), dozenal digit names (Zil/Ter/Lun/Stel — invented photon vocabulary, universal like the glyphs), VSF field names and storage keys, the brand word "Photon".
// Numbers inside translated strings render at this edge per the number doctrine: EVERY numeral goes thru crate::fmt_num (or fmt_i when signed). The one deliberate exception is Msg::FileBubble, whose size draws in a face that can't resolve the dozenal glyph bytes — see the comment there; it is a font problem, not a licence for raw arabic anywhere else.
// Multi-line passages (About prose, join instructions, the riddle) are ONE variant joined with '\n'; call sites iterate .lines() so translators see whole passages.
// Adding a string = add a Msg variant; every language file then fails to build until its arm exists, so no English can silently leak into a translated UI.

use super::state::SettingsPage;
use std::borrow::Cow;

pub mod de;
pub mod en;
pub mod es;
pub mod fil;
pub mod fr;
pub mod hi;
pub mod id;
pub mod it;
pub mod mi;
pub mod pl;
pub mod pt;
pub mod ru;
pub mod sw;
// Turkish's language code IS `tr`, which collides by sight with `tr()`, the translate function below. Rust keeps modules and functions in separate namespaces so both resolve unambiguously — `tr::text(msg)` is the module, `tr(msg)` is the call — and the file stays named for its code like every other language here.
pub mod tr;
pub mod uk;
pub mod vi;

/// The UI language: fgtw's one list (fgtw::text::Lang), shared with ferros. A device-local typed setting (display.lang, x-string code), seeded once from the OS locale at first launch, the user's after that.
/// Its `index()` is the storage form of the session atomic below and never moves; `ALL` is the picker order.
pub use fgtw::text::Lang;

// Session mirror of the language setting — a static so tr() reads the language without threading &self thru every draw call (same shape as DOZENAL_UI).
static CURRENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn lang() -> Lang {
    Lang::from_index(CURRENT.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn set_lang(l: Lang) {
    CURRENT.store(l.index(), std::sync::atomic::Ordering::Relaxed);
}

/// A message in the current language: Photon's own, or a fleet message from fgtw (`tr(fgtw::text::Msg::...)` converts).
pub fn tr<'a>(msg: impl Into<Msg<'a>>) -> Cow<'static, str> {
    let msg = msg.into();
    match lang() {
        Lang::En => en::text(msg),
        Lang::Es => es::text(msg),
        Lang::Mi => mi::text(msg),
        Lang::Pt => pt::text(msg),
        Lang::Fr => fr::text(msg),
        Lang::Id => id::text(msg),
        Lang::De => de::text(msg),
        Lang::Sw => sw::text(msg),
        Lang::Vi => vi::text(msg),
        Lang::Tr => tr::text(msg),
        Lang::Ru => ru::text(msg),
        Lang::Hi => hi::text(msg),
        Lang::Uk => uk::text(msg),
        Lang::It => it::text(msg),
        Lang::Pl => pl::text(msg),
        Lang::Fil => fil::text(msg),
    }
}

/// A fleet or identity message in fgtw's words (fgtw::text, the one copy Photon and ferros share), in language `l`.
pub fn fleet_text(l: Lang, m: fgtw::text::Msg) -> Cow<'static, str> {
    let mut s = String::new();
    crate::log_err!(fgtw::text::tr(l, m, &mut s), "write a fleet message");
    s.into()
}

impl<'a> From<fgtw::text::Msg<'a>> for Msg<'a> {
    fn from(m: fgtw::text::Msg<'a>) -> Self {
        Msg::Fleet(m)
    }
}

pub enum Msg<'a> {
    /// A fleet or identity message: its words live in fgtw::text, so ferros and Photon never drift.
    Fleet(fgtw::text::Msg<'a>),
    // ---- shared verbs / small buttons ----
    Answer,
    Decline,
    /// The silent dismissal on the ring panel.
    Reject,
    RejectedWaveRow,
    ReplicatePill,
    ReplicatingToFleet,
    Delete,
    Keep,
    Play,
    EndWave,
    EndBeam,
    HangUp,
    Attest,
    Cancel,
    Arm,
    Disarm,
    Add,
    Update,
    // ---- page names (nav rail) ----
    PageName(SettingsPage),
    // ---- wave screen ----
    SpeakerToggleOn,
    SpeakerToggleOff,
    SpeakerPlain,
    /// The automatic device choice in the wave screen's mic / speaker / camera pickers (the system default, else the first device found).
    AutoPick,
    /// A phone camera's facing in the beam's self-view caption ("Back · 24 mm").
    CameraFront,
    CameraBack,
    EarpiecePlain,
    /// The earpiece on a device whose voice path is slow (a longer delay than the loudspeaker).
    EarpieceSlower,
    AddHandle,
    AddHandlePlain,
    BackToContact,
    WaveStart,
    // Beam = video (wave's sibling) — stubbed, unwired; the label ships so the button can exist before the feature.
    BeamStart,
    /// The ring panel's audio answer (and the wave card's option): answering is choosing audio.
    WaveBack,
    /// The video answer / the wave card's option — a stub until video lands.
    BeamBack,
    /// The in-wave video switch — a stub until video lands.
    BeamToggle,
    IncomingWave,
    IncomingWaveNoPath,
    /// The ☎ pill while the friend is online but only over the relay — a wave cannot carry voice there yet, so the pill says why instead of failing silently (Nick 2026-09-17).
    WaveStartNoPath,
    /// A group wave is running and this device is not in it: the pill joins it (Nick 2026-11-01: "leave/join at will").
    WaveJoin,
    /// The same, with our camera.
    BeamJoin,
    /// The in-wave mute pill while our microphone goes out: a tap silences it.
    WaveMute,
    /// The same pill while muted: a tap brings the microphone back.
    WaveUnmute,
    /// The in-wave camera pill while our camera goes out: a tap turns it off, the wave goes on (the label says what a tap does, as Mute does).
    CameraOff,
    /// The in-wave red button (Nick 2026-11-01: "Should be leave beam/wave, not end beam/wave. Nobody can end it, everyone can leave, thus ending it."): we leave; the wave ends for us, and for everyone once the last one leaves.
    LeaveWave,
    /// The same, while video is in the wave.
    LeaveBeam,
    /// The wave card's "wave back" in the same state.
    WaveBackNoPath,
    WaveActiveNoPath(&'a str),
    WaveReconnecting,
    /// Answer tapped but no frame could go out (the friendship is mid-ceremony) — the ring keeps going, the person needs to know why nothing happened.
    AnswerFailedReconnecting(&'a str),
    WaveDroppedRow,
    /// A dropped wave with its live duration — the wave card header.
    WaveDroppedDur(&'a str),
    WaveChipElsewhere(&'a str),
    WaveChipElsewhereUnknown,
    StopPlayback,
    WaveEndedDur(&'a str),
    WavingName(&'a str),
    WaveRow,
    MissedWaveRow,
    WaveDeclinedRow,
    BusyRow,
    // ---- launch / handle / join ----
    YesForever,
    HandleTaken,
    HandleAttestedElsewhere(&'a str),
    IdentityResumeHint,
    IdentityOccupied,
    HandleHint,
    HandleHintJoin,
    ThisDeviceName(&'a str),
    CopyWords,
    WordsCopied,
    LaunchJoinInstructions,
    StartFreshIdle,
    StartFreshArmed,
    OffGridListening,
    AddedHandle(&'a str),
    AlreadyAdded(&'a str),
    NotFound,
    SearchError(&'a str),
    AlreadyInContacts,
    /// The add box was given our own handle: we are no contact of ours.
    ThatsYou,
    Attesting,
    /// What the attest is doing now (handle_query::ATTEST_STAGE): 0 proving the handle, 1 checking this device is ours, 2 asking fgtw.org, 3 opening the vault, 4 loading conversations.
    AttestStage(u8),
    RevokedByFleet,
    IdentityCarriedHint,
    // '\n'-joined passage; the call site iterates .lines() and gives the FIRST line the headline (error) colour.
    PermanenceWarning,
    // '\n'-joined passage; same first-line-headline colour rule as PermanenceWarning.
    KnownHandleWarning,
    PickAnotherName,
    ItsMineShowWords,
    LockedRetry,
    // ---- ready screen ----
    PeersOnline(usize),
    NetworkBack,
    AvatarDropHint,
    SearchPlaceholder,
    StorageDegraded,
    /// The amber banner clearing itself: a persist succeeded after the failure that raised it.
    StorageRecovered,
    StorageDataLost,
    AutoAttestBadge,
    ClockOff { pretty: &'a str, ahead: bool },
    HoursShort(u64),
    MinutesShort(u64),
    SecondsShort(u64),
    // ---- conversation ----
    NewMessages(usize),
    MessagesDelivered(usize),
    ChatDaysSpan(usize),
    ContactFleetPinned(usize),
    /// Person page: how many of their devices answer right now.
    DevicesOnline(usize),
    PublishedNameExplainer(&'a str),
    EditingSnippet(&'a str),
    ReactSnippet(&'a str),
    EnrolAttempts { n: usize, fleet_owner: Option<&'a str> },
    MessageNotSaved,
    AttachmentLimit,
    AgoDays(u32),
    AgoHours(u32),
    AgoMinutes(u32),
    AgoSeconds(u32),
    /// DMS age (dozenal mode): the pre-rendered glyph string.
    AgoDms(&'a str),
    // ---- the Dozenal page ----
    DmsHead,
    DmsIntro,
    /// Plain reading of a time-ago rung, keyed by the DOUBLING COUNT OF OSCILLATIONS (0 one oscillation, 20 a millisecond, 24 a frame, 30 a second, 36 a minute, 42 an hour, 46 a day, 51 a month, 55 a year, 61 a lifetime, 89 the age of the universe); empty for values the legend doesn't list.
    DmsReading(u32),
    /// The size legend on the Dozenal page: sizes count doublings of a BIT, so a byte is four.
    DmsSizeHead,
    DmsSizeIntro,
    /// Plain reading of a DMS size value (bit length of the size in bits); empty for values the legend doesn't list.
    DmsSizeReading(u32),
    // ---- the scaling explained (Nick 2026-09-11: "DMS clearly explained on the base page and why it's used") ----
    /// What one number for how much means, why doublings, and the three forms.
    DmsScaleHead,
    DmsScaleProse,
    /// The unit of account: what "one" is on every scale, and what happens below it.
    DmsUnitsHead,
    DmsUnitsProse,
    /// REPUTATION, the reason the scaling exists (docs/dozenal.md): five sections on the Base page's dozenal arm — what a grade is, how one fills, why the set never totals, and what stands behind it.
    /// LOG FORM (2026-09-30): the grade is a signed DOUBLING COUNT of praise over dings, and beside it the doubling count of everyone who spoke — two axes that deliberately cannot be added into one, because any single scalar is a score once people sort by it.
    RepHead,
    RepProse,
    RepOneHead,
    RepOneProse,
    RepFillHead,
    RepFillProse,
    /// One row of the reputation ladder, keyed by ROW INDEX — the two restaurants and the spotless rungs; the reading and its support are computed at the render edge (rep_reading / rep_support), never typed.
    RepRow(u32),
    RepNoTotalHead,
    RepNoTotalProse,
    RepBehindHead,
    RepBehindProse,
    /// The length legend: doublings of the hydrogen line's wavelength; the reading takes the signed doubling count.
    DmsLengthHead,
    DmsLengthIntro,
    DmsLengthReading(i32),
    /// The hex page's one line on lengths: millimetres, linear.
    HexLengthNote,
    /// Zero has no logarithm: an age of nothing and a size of nothing read as words (pure-log DMS, 2026-09-11).
    DmsNow,
    DmsEmpty,
    // ---- THE BASE PAGE LESSON (Nick 2026-09-30: "a good formal edgumacation … splain how this shiz works with 1+1"), in reading order ----
    /// How the twelve digit names carry their values: four stems by threes, three endings by ones. Sits above the cheat sheet.
    DigitsNamingProse,
    /// Counting and adding in plain dozenal digits — carrying at twelve, Lun plus Lun is Zila Zil, the exact fractions.
    CountHead,
    CountProse,
    /// Arithmetic on DOUBLING COUNTS: multiply by adding, divide by subtracting, and the awkward add (the bigger wins, the smaller nudges).
    MagArithHead,
    MagArithProse,
    /// One row of the add ladder, keyed by row index; the value is computed at the render edge from real bit counts.
    MagAddRow(u32),
    /// The fraction digits of a doubling count: four digits, half a percent.
    FineHead,
    FineProse,
    /// One row of the four-digit mass ladder, keyed by row index (a newborn … a car); values computed in hydrogens.
    FineReading(u32),
    /// "Let's imagine": the scales photon does not show yet, all read off the same atom — speed, temperature, mass. Rows keyed by index; every value computed from the anchors in lib.rs.
    ImagineHead,
    ImagineProse,
    ImagineSpeedHead,
    ImagineSpeedIntro,
    ImagineSpeedReading(u32),
    ImagineTempHead,
    ImagineTempIntro,
    ImagineTempReading(u32),
    ImagineMassHead,
    ImagineMassIntro,
    ImagineMassReading(u32),
    /// THE LIVE BLOCK at the top of the Base page (Nick 2026-10-01): the clock as a share of today and the time since the page opened as a magnitude, each with its reading pre-rendered in the current base.
    LiveHead,
    LiveProse,
    LiveClockLabel,
    LiveSinceLabel,
    /// Day of the week as a WORD, Monday = 0 … Sunday = 6 (seven is coprime to twelve, so weekdays never get glyphs — docs/dozenal.md).
    Weekday(u32),
    // ---- base page (2026-09-10) ----
    /// Early note on the dozenal page: time and size are logarithmic (Dozenal Metric Scaling).
    BaseLogNote,
    /// The digit cheat sheet's title and the hex page's coder blurb.
    DigitsHead,
    WhyHex,
    WhyHexProse,
    HexTimeIntro,
    HexTimeReading(u64),
    HexSizeIntro,
    HexSizeReading(u64),
    /// Diagnostics: the last wave's link, as a frequency in the current base.
    LastWave { link: &'a str, loss: &'a str, buffer: &'a str },
    NoWaveYet,
    ConversationTitle,
    BackToContacts,
    NameReclaimed,
    IdentityEndedFrozen,
    // The param is the CLUTCH ladder status string (already narrated elsewhere), passed thru verbatim.
    ClutchStatus(&'a str),
    // The ceremony ladder's step text, zero-indexed 0..=11 — the prefix (.⟨dozenal digit⟩ or n/12) is the caller's (Contact::clutch_status_detail).
    ClutchStep(u8),
    ClutchSecured,
    // ---- compact wave bar ----
    WaveBarWaving(&'a str),
    WaveBarInWave(&'a str),
    WaveBarKeepRecording,
    NoDirectPathSuffix,
    // ---- message details strip + action pills ----
    SentDetail { age: &'a str, state: &'a str },
    ReceivedDetail(&'a str),
    DeliveryDelivered,
    /// One line per other member under a selected row of ours: they hold it (any of their devices ACKed), or not yet.
    MemberHolds { name: &'a str },
    MemberWaiting { name: &'a str },
    /// Under our own face on a group wave's roll (docs/group-waves.md).
    WaveYou,
    DeliveryReplicated,
    DeliverySending,
    RecoveredSuffix,
    EditedSuffix,
    /// One prior version of an edited row in the selected meta (chat.history on): its text as it stood, stamped with its age.
    EditWasLine { age: &'a str, text: &'a str },
    BlobDeliveredSuffix,
    BlobSendingSuffix,
    BlobNotHereSuffix,
    BlobHereSuffix,
    // The param is a reaction emoji glyph, never translated.
    ReactTheySuffix(&'a str),
    ReactYouSuffix(&'a str),
    ReplyPill,
    EditPill,
    CopyPill,
    /// Link consent dialog title — a tapped message link never opens silently.
    LinkConsentTitle,
    /// The consent dialog's Open action.
    OpenLinkPill,
    /// Consent warning when the destination contains non-ASCII bytes (homograph honesty).
    LinkNonAsciiWarn,
    /// Release armed (holdings.rs): the have-set check is running.
    HoldingsChecking,
    /// Release armed: no fleet key in RAM, so nothing could be checked.
    HoldingsNoKey,
    /// Release armed: every held attachment is also on a sibling.
    HoldingsAllSafe,
    /// Release armed: what exists only on this device (counts and size pre-rendered).
    HoldingsAtRisk { files: &'a str, recordings: &'a str, size: &'a str },
    /// Release armed: the siblings that have not answered, each "name (age)".
    HoldingsWaiting(&'a str),
    /// A sibling with no signed traffic this session, in place of its age.
    HoldingsNotHeard,
    /// The Release card's copy pill (removal safety phase 3): copy what exists only here, then release by itself.
    HoldingsCopyThenRelease,
    /// The same pill while that is pending: cancel it.
    HoldingsStopWaiting,
    /// Hint under the copy pill: which sibling takes the copies.
    HoldingsCopyHint(&'a str),
    /// Hint while copy-then-release is pending: who is copying and what happens next.
    HoldingsCopyingTo(&'a str),
    CopiedPill,
    ResendPill,
    FetchPill,
    SavePill,
    /// A wave's recording to a file (the wave card's word for save) and a wave off the timeline (its word for delete), Nick 2026-09-12.
    ExportPill,
    /// Toast while a wave recording decodes into its export file.
    ExportingWave,
    DiscardPill,
    /// "Keep it, but not here": drop this device's copy of an incoming pigeon's bytes — the row and re-fetch stay.
    LoftPill,
    /// The details strip's stats for an attachment row (Nick 2026-09-12: "stats up top: name, time, size, type"): pre-formatted parts, the age follows on the same line.
    AttachStats { name: &'a str, kind: &'a str, size: &'a str, dims: &'a str },
    /// The kind as a word for the stats line.
    AttachKindName(crate::types::AttachKind),
    PlayPill,
    /// The replayed wave screen's pause (replay.rs).
    PausePill,
    /// The replayed wave screen: a voice its recording does not name, by its place (1-based for people).
    ReplayVoice(usize),
    DeletePill,
    DeletingPill,
    StopPill,
    // ---- contact panel (About / Stats / Manage) ----
    NameNotShared,
    NameShared(&'a str),
    AvatarNotShared,
    AvatarShared,
    ReclaimedStranger,
    IdentityEndedByOwner,
    IdentityNotFolded,
    WhatTheyShare,
    HistoryComplete,
    HistorySyncing,
    HistoryIdle,
    SelfNoChain,
    ChainWoven,
    AlwaysReachableSelf,
    MessagesSentReceived { total: usize, sent: usize, recv: usize },
    RowsShouldMatch,
    /// The ceremony status while OUR add awaits theirs (consent gate): nothing is being made yet, and "making keys 0/12" said otherwise (Nick 2026-10-01).
    ClutchWaitingTheirAdd,
    SiblingSignsItselfOut,
    BootPill { armed: bool },
    /// Person page: open (or found) the conversation of exactly the two of you.
    MessagePill,
    /// A group's Manage page: the hint under its big avatar while the pointer is over it (desktop).
    GroupAvatarDropHint,
    /// A group's Manage page: take the group's picture away (the members' pie again).
    RemoveGroupAvatarPill,
    // ---- groups (docs/molecules.md §10.5) ----
    /// The Ready list's filter strip (FilterAll is the stream filter's, shared).
    /// The Ready strip pill for group invitations waiting on us (a count follows it).
    FilterInvitations,
    /// The Ready view of the people you have added (docs/conversations.md D8).
    FilterPeople,
    FilterAtoms,
    FilterMolecules,
    /// The sponsor's line under an atom or molecule row: the friends invited who have not joined yet.
    InvitedWaiting { names: &'a str },
    /// The notification body when a friend invites us into a molecule.
    InvitedYouTo { title: &'a str },
    /// Bonds-page row tags: a standing member (locked), an invite out and not answered, and the two edits staged until the page is left.
    BondMember,
    BondInvited,
    BondWillInvite,
    BondWillWithdraw,
    /// The picker's "start a new group" row.
    NewAtom,
    /// The Ready screen's create pill while naming a new atom: alone it founds an atom, with friends picked a molecule (they are invited as it is founded).
    CreateAtom,
    CreateMolecule,
    /// History policy pills, fixed at birth (D5).
    HistoryFromGenesis,
    HistoryFromJoin,
    /// Header suffix while our Join awaits the sponsor's wrap.
    JoiningStatus,
    /// Header suffix while an era's wrap has not reached this device.
    CatchingUpStatus,
    /// Header suffix after we left.
    LeftStatus,
    /// A member whose fold has not yet succeeded — drawn as a contact without a name is.
    PendingMember,
    /// The conversation panel's pages (docs/conversations.md Phase 1).
    ConversationPageName(crate::ui::state::ConversationPage),
    /// How many of a conversation's other members are online (the In numbers page, more than one other).
    MembersOnline { online: usize, total: usize },
    MemberPendingName,
    /// Our own row in a member list.
    YouLabel,
    EraIndex { n: &'a str },
    MutePill { muted: bool },
    LeaveMoleculePill { armed: bool },
    LeaveMoleculeNote,
    BindNote,
    NobodyToAdd,
    YouLeftNote,
    /// Settings → Conversations (docs/molecules.md §10 in plain words).
    MoleculesExplainHead,
    MoleculesExplainProse,
    DefaultHistoryHead,
    DefaultHistoryNote,
    YourMolecules,
    NoAtomsYet,
    /// The offer card (docs/molecules.md §10.1), invitee side: "<sponsor> brought you into <title> · <n>".
    BondOfferLine { sponsor: &'a str, title: &'a str, n: &'a str },
    /// The Join pill.
    BindPill,
    BootRemovesEverywhere,
    BootOstracism,
    /// Manage page (2026-09-27): two-tap pills that discard this conversation's waves, or its whole history, on every device of ours — the friend's copy is theirs.
    ClearWavesPill { armed: bool },
    ClearHistoryPill { armed: bool },
    ClearHistoryNote,
    // ---- add device / pairing ----
    TapOrbCancel,
    AddDeviceError(&'a str),
    JoinFailed(&'a str),
    RequestFailed(&'a str),
    // ---- fleet page ----
    FleetTitle,
    FleetTapToCopy,
    /// The fleet's signature floor line: which scheme set every member has proved, and how many members have declared the full basket.
    FleetSigFloor(&'a str, usize, usize),
    /// A device card's signing line: the schemes the chain knows this device signs with.
    DeviceSigns(&'a str),
    /// Fleet page: what the device says its hardware is (the plate kind's name) and the security level that states, 0 to 4.
    DeviceLevel(&'a str, u8),
    /// Fleet page: the device never said what its hardware is (an older build).
    DeviceLevelUnstated,
    /// Chain lock-out badge on a device card.
    ChainLocked,
    RetiredStillYours,
    RevokedBadge,
    Online,
    /// Path-tier words — the same resolution that picks the dot's colour, spoken (see ShownTier).
    TierLan,
    TierWan,
    TierDirect,
    TierRelay,
    /// One-line legend under a presence dot, so the colour scheme explains itself.
    TierLegend,
    Offline,
    OnlineVia(&'a str),
    NotEggedYet(&'a str),
    FoundNearby(&'a str),
    SignOutPublishFailed,
    LastDeviceCantSignOut,
    NoIdentityToRemove,
    SignOutBuildFailed,
    ReleaseFailed,
    DeviceReinstatedToast(&'a str),
    RevokedNotice(&'a str),
    ConfirmReinstate(&'a str),
    ConfirmRevoke { name: &'a str, last_unlocker: bool },
    CopiedName(&'a str),
    ReleasePill { armed: bool },
    BridgePill,
    ReinstatePill { armed: bool },
    RevokePill { armed: bool },
    SingleCopyWarning,
    DeviceSignsItselfOut,
    RenamePill,
    // ---- security page ----
    SecurityLock,
    /// KILL (Nick 2026-09-10): one tap drops the identity and ends the process this instant; the vault stays.
    SecurityKill,
    SecurityKillHint,
    /// The bridge shell's host-side switch (Nick 2026-11-03, security audit UPW-027): default OFF, set on this device only, never synced — a lost phone must not be a shell on every computer you own.
    SecurityShell { on: bool },
    SecurityShellHint,
    /// What the operator sees when a host whose switch is off receives a command.
    BridgeShellOff,
    SecurityRevoke { armed: bool },
    SecurityRevokeHint,
    /// Why Revoke is dead on a fleet of one — shown IN PLACE of the normal hint, so the greying always carries its reason.
    SecurityRevokeAloneHint,
    /// Why Release is dead on a fleet of one.
    SecurityReleaseAloneHint,
    RevokeNeedsAnotherDevice,
    SecurityShred { armed: bool },
    SecurityRemoveShred { armed: bool },
    SecurityStatusLine,
    LoadOnStartup,
    LoadOnStartupExplainer,
    LifelineCheckbox,
    LifelineExplainer,
    LifelineChangeFailed(&'a str),
    UnattendedTitle,
    UnattendedCheckbox,
    UnattendedArmExplainer,
    UnattendedDisarmExplainer,
    UnattendedMismatch,
    UnattendedWarning,
    UnattendedArmedToast,
    UnattendedDisarmedToast,
    CouldntChangeLoginItem(&'a str),
    Custodians,
    CustodianCheckbox,
    CustodianExplainer,
    SecurityIntro,
    // Pre-attest Security page (Nick 2026-09-16): one verb, Wipe, for a device someone else held or a handle nobody will type again.
    SecurityIntroUnattested,
    SecurityWipeUnattestedHint,
    SecurityLockHint,
    SecurityShredHint,
    SecurityRemoveShredHint,
    // ---- appearance ----
    Theme,
    DarkChrome,
    LightChrome,
    PartyColours,
    ZoomTextSize,
    ColourCalibration,
    // ---- notifications ----
    NotificationsTitle,
    ChimeNewMessage,
    VibrateNewMessage,
    RingIncomingWave,
    /// Notifications page: hold every wave recording on this device (replication by default).
    HoldWavesOnDevice,
    /// Wave page: open the mic thru the phone's own voice processing (echo cancel, noise, gain) instead of the calibrated raw feed — an experiment, device-local.
    VoiceDsp,
    /// Wave page (Android): treat a wave as a phone call — the in-communication audio mode, so other apps' sounds go quiet during it.
    InCallMode,
    /// Beam page: the Save beam video heading and its checkbox (beam.save_video, off by default).
    BeamSaveHead,
    BeamSaveVideo,
    KeepEditHistory,
    VibrateIncomingWave,
    PresenceCheckbox,
    PerContactOverride,
    // ---- updates ----
    UpdatesTitle,
    /// "What's new in {version}" — the release-notes heading on the Updates page.
    WhatsNew(&'a str),
    /// Under the dev pill on the Updates page.
    DevChannelHint,
    PhotonVersion(&'a str),
    AutoUpdateInstall,
    UpdateChecking(&'a str),
    UpdateUnavailable(&'a str),
    UpdateNoBuild(&'a str),
    UpdateAlreadyOn { kind: &'a str, ver: &'a str },
    UpdateGet { kind: &'a str, ver: &'a str },
    Updating,
    Downloading,
    /// The pre-formatted size (unit_size: `n MiB`, or the bare hex bit count).
    DownloadingSize(&'a str),
    UpdateAvailableToast(&'a str),
    Installing { channel: &'a str, ver: &'a str },
    UpdatedRestarting,
    DownloadedConfirm,
    UpdateFailed(&'a str),
    // ---- diagnostics ----
    HardLogs,
    LogCleared,
    /// Diagnostics: the log file's size, already formatted in the current base (crate::dms_size).
    LogEmpty,
    LogSent,
    SendFailed(&'a str),
    NoLogToSend,
    /// The pre-formatted size (unit_size: `n KiB`, or the bare hex bit count).
    SendingLog(&'a str),
    CantSendNotSignedIn,
    DiagRecordInspect { ts: &'a str, lines: usize },
    DiagDecoding,
    DiagMeta { count: usize, size: &'a str },
    DiagTrimmed,
    DiagInfo { used: &'a str, cap: &'a str, pct: u64 },
    // ---- the Vault page (read-only stats; every size pre-formatted thru dms_size, every count thru fmt_num64) ----
    VaultIntro,
    VaultCapacity(&'a str),
    VaultOdometer(&'a str),
    VaultOccupied { now: &'a str, live: &'a str, dead: &'a str },
    VaultLiveEntries(&'a str),
    VaultCommits(&'a str),
    VaultHealthOk,
    VaultHealed(&'a str),
    VaultDegraded,
    VaultRefresh,
    VaultReading,
    /// "What holds the space" — the per-conversation breakdown section head on the Vault page.
    VaultSpaceHead,
    VaultFilterAll,
    VaultFilterWaves,
    VaultFilterPictures,
    VaultFilterSongs,
    VaultFilterFiles,
    VaultFilterKept,
    /// One breakdown row: a conversation's total for the active filter, its name, and its row count.
    VaultConvLine { size: &'a str, name: &'a str, count: &'a str },
    VaultBinEmpty,
    // ---- the Wave page ----
    // ---- the Beam settings page (docs/beams.md "Settings") ----
    BeamIntro,
    BeamFollowHead,
    BeamThisDevice,
    BeamFollowFleet,
    BeamHdrHead,
    BeamHdrProse,
    BeamHdrUnavailable,
    BeamOff,
    BeamOn,
    BeamSizeHead,
    BeamAuto,
    BeamFpsHead,
    BeamQualityHead,
    BeamQualityProse,
    BeamQualityLean,
    BeamQualityStandard,
    BeamQualityRich,
    BeamBitrateHead,
    BeamKbit(&'a str),
    BeamShedHead,
    BeamShedSmooth,
    BeamShedSharp,
    /// The Beam page's camera-rotation heading, naming the camera it applies to.
    BeamRotateHead(&'a str),
    /// The grants gate's refusals: a wave or beam not sent because the microphone or camera is switched off for Photon.
    GrantMicOff,
    GrantCameraOff,
    /// The settings pages' device tests: the Beam page's camera preview and the Wave page's microphone meter.
    BeamPreviewHead,
    BeamPreviewStart,
    BeamPreviewStop,
    BeamNextCamera,
    WaveTestHead,
    WaveTestStart,
    WaveTestStop,
    WaveTestLevel(&'a str),
    BeamTestHead,
    BeamScreenHdr { allowed: &'a str, asked: &'a str },
    BeamScreenHdrNone,
    BeamScreenSdr,
    BeamRampBlackWhite,
    BeamRampWhiteBrightest,
    BeamRampStops,
    BeamPatches,
    BeamPatchWhite,
    BeamPatchAllowed,
    BeamPatchTop,
    BeamShadows,
    BeamColourRamps,
    WaveIntro,
    /// "This device's hearing" — the trim section head.
    WaveHearingHead,
    /// The trim's current value in stops, the route, and the rocker as read.
    WaveHearingLine { stops: &'a str, route: &'a str, rocker: &'a str },
    WaveTrimWhy,
    WaveQuieter,
    /// The trim bar's end labels.
    WaveTrimQuietMin,
    WaveTrimLoudMax,
    WaveLouder,
    /// "This device's voice" — the calibration section head.
    WaveVoiceHead,
    WaveVoiceWhy,
    /// One mic's profile: voiced level, quiet floor, waves of evidence.
    WaveMicLine { mic: &'a str, voiced: &'a str, floor: &'a str, n: &'a str },
    WaveNoProfiles,
    WaveForget,
    WaveMeasureNow,
    WaveListening,
    /// The measure-now verdict.
    WaveMeasured { voiced: &'a str, floor: &'a str },
    WaveMeasuredQuiet,
    /// "The last wave" — the stats section head.
    WaveLastHead,
    WaveNoWaveYet,
    WaveLastLink { dur: &'a str, path: &'a str, floor: &'a str, ema: &'a str, max: &'a str },
    WavePathLan,
    WavePathWan,
    WaveLastLoss { lost: &'a str, of: &'a str, filled: &'a str },
    WaveLastLadder { rate: &'a str, ups: &'a str, downs: &'a str, peer: &'a str },
    WaveLastLevel { start: &'a str, end: &'a str, voiced: &'a str },
    WaveLastReaim { voiced: &'a str, from: &'a str, to: &'a str },
    WaveLastJitter { underruns: &'a str, trims: &'a str },
    /// "Preferences" — the toggles section head.
    WavePrefsHead,
    LogTitle,
    DiagBack,
    DiagClear,
    DiagSubmit,
    DiagView,
    IdeasIntro,
    IdeasSendIdea,
    IdeasSendFix,
    IdeasYours,
    IdeasNone,
    IdeasNothingTyped,
    IdeasSending,
    IdeasSent,
    IdeasSendFailed(&'a str),
    /// Ideas page: the ticker switch (off by default) and the notification titles it raises.
    IdeasTicker,
    IdeasTickerNewIdea,
    IdeasTickerNewFix,
    /// Ideas page: the public list — the matches while typing, and the whole list with the box empty.
    IdeasLikeYours,
    IdeasNothingLikeIt,
    IdeasEveryone,
    IdeasEveryoneNone,
    /// Ideas page: a gripe's state in plain words (the worker's state names: received, seen, planned, working, fixed, declined, duplicate; the client's own: sending, not sent).
    IdeaState(&'a str),
    OptionalNote,
    // ---- you / profile ----
    YouAddCustomField,
    YouIdentity,
    ChangeAvatar,
    DragDropAvatar,
    ProfileSaved,
    NoChanges,
    FieldExists,
    FieldAdded(&'a str),
    ProfileTierName,
    ProfileTierReach,
    ProfileTierPlace,
    ProfileTierPersonal,
    ProfileTierWork,
    ProfileTierSensitive,
    /// Over the share-box column on the You page: what a tick means.
    YouShareHint,
    ProfileTierCustom,
    ProfileNamePreferred,
    ProfileNameFirst,
    ProfileNameMiddle,
    ProfileNameLast,
    ProfileNameNick,
    ProfileNamePrefix,
    ProfileNameSuffix,
    ProfileNameMaiden,
    ProfileNamePronunciation,
    ProfileReachEmail,
    ProfileReachPhone,
    ProfileReachWeb,
    ProfileReachAltMsg,
    ProfilePlaceAddr,
    ProfilePlaceGeo,
    ProfilePlaceTz,
    ProfilePersonalDob,
    ProfilePersonalPronouns,
    ProfilePersonalGender,
    ProfilePersonalLang,
    ProfilePersonalBio,
    ProfileWorkOrg,
    ProfileWorkTitle,
    ProfileSensitiveSsn,
    ProfileSensitivePassport,
    ProfileSensitiveLicense,
    ProfileSensitiveTaxId,
    ProfileSensitiveEmergency,
    // ---- attachment bubbles ----
    RecordingBubble { size: &'a str, fetching: bool },
    RecordingPlaying { pct: u32 },
    FileBubble { glyph: &'a str, name: &'a str, size: &'a str, held: bool },
    // ---- attachment viewer / reader ----
    ViewerBack,
    ViewerDecoding,
    /// The wave panel's live line: rung name (a proper noun, untranslated), round trip as a frequency, loss of 256, buffer frames.
    WaveLiveStats { rung: &'a str, freq: &'a str, loss: &'a str, buf: &'a str },
    ReaderTooLarge,
    AttachDropHint,
    InspectFailed(&'a str),
    // ---- message persistence / attachments toasts ----
    RewritingVault,
    ResentOnChain,
    RepushedFleet,
    FetchingFromDevices,
    CantPlayNow,
    // ---- wave card + stream filter ----
    /// The waveform band while the keep transcode runs (the card exists from hangup; the audio lands after).
    WaveKeeping,
    /// Elapsed / total while a recording plays or is scrubbed — both already base-formatted `M:SS`.
    WavePos { pos: &'a str, total: &'a str },
    FilterAll,
    FilterWaves,
    FilterText,
    SavedTo(&'a str),
    SaveFailed,
    // ---- secured-elsewhere status ----
    DifferentIdentity,
    AnotherDevice,
    SecuredOn(&'a str),
    SecuringOn(&'a str),
    SecuredElsewhere,
    // ---- bridge ----
    BridgeElided { bytes: usize, output: &'a str },
    BridgeShellStartFailed(&'a str),
    BridgeNoOutput(i32),
    BridgeOutputExit { output: &'a str, code: i32 },
    BridgeShellDied(&'a str),
    /// A bridge pigeon (dropped file) could not land in the host's shell directory — the host's transcript reply.
    PigeonLandFailed(&'a str),
    PigeonStalled(&'a str),
    /// The wave's status line: the capture is delivering zeros (wave/mod.rs MIC_SILENT).
    MicSilent,
    /// macOS refused the microphone without asking; Photon fixed itself — quit and reopen.
    MicRelaunch,
    /// Placing a beam (the Outgoing status), and a friend beaming us on the compact bar.
    BeamingName(&'a str),
    BeamBarBeaming(&'a str),
    /// The beam's Calibrate button (beam_calibrate.rs) and what it reports on the status line.
    Calibrate,
    CalScanning,
    Calibrated(&'a str),
    CalFailed(&'a str),
    /// The ring screen's status when the offer carries a beam.
    IncomingBeam,
    /// macOS: the Microphone grant is off for Photon — and where the switch is.
    MicDenied,
    EarlierOutputDropped(&'a str),
    DeviceNotSibling,
    ShellExited,
    StopReceivedIdle,
    NoResponseToStop,
    StreamLost,
    CommandUndeliverable,
    // ---- about ----
    AboutKillswitchReady,
    AboutPasslessHead,
    AboutPasslessLead,
    AboutPasslessProse,
    AboutConsentHead,
    AboutConsentProse,
    AboutTokenHead,
    AboutTokenProse,
    AboutWaveBeamHead,
    AboutWaveBeamProse,
    AboutVersion(&'a str),
    /// The standing clock correction on the About page: how far this device's system clock sits from network-consensus true time, and the confidence half-width. Both already rendered as base-aware numeric strings by the caller.
    AboutClockOffset { ms: &'a str, conf: &'a str },
    /// Shown in the same slot before the first consensus lands.
    AboutClockUnknown,
    AboutVersionSpelled { main: &'a str, patch: Option<&'a str> },
    AboutDozenalHead,
    AboutRiddle,
    WhyDozenal,
    // The decimal-mode swap: shown INSTEAD of WhyDozenal/WhyDozenalProse while the toggle is off — a plain lesson on what base ten is (place value, the Indian origin, zero's date, other bases, the slide rule), no scold line; the red box carries the disapproval.
    BaseTenHead,
    // '\n'-joined paragraphs like WhyDozenalProse. Dozenal glyph control codes (0x10..0x1B) ride inline; the font chain draws them from any family.
    BaseTenProse,
    // '\n'-joined paragraphs; the About card iterates .lines() and wraps each as its own stanza.
    WhyDozenalProse,
    // ---- settings misc ----
    LanguageLabel,
    SettingsTitle,
    SettingsBack,
    Dozenal,
    Hexadecimal,
    Arabic,
}
