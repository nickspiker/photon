// The language catalog: every user-facing string is a Msg variant, every language is one exhaustive match, and the compiler is the completeness checker.
// Doctrine (docs/languages.md): keys are WHOLE messages with holes (never concatenate translated fragments); variants carry semantic values (counts, names) and each language renders its own word order, plurals, and gender.
// Never translated: handles (byte-precise), voca pairing words (protocol material), log lines (photonlog grep-ability), dozenal digit names (Zil/Ter/Lun/Stel — invented photon vocabulary, universal like the glyphs), VSF field names and storage keys, the brand word "Photon".
// Numbers inside translated strings render at this edge per the number doctrine: EVERY numeral goes thru crate::fmt_num (or fmt_i when signed). The one deliberate exception is Msg::FileBubble, whose size draws in a face that can't resolve the dozenal glyph bytes — see the comment there; it is a font problem, not a licence for raw arabic anywhere else.
// Multi-line passages (About prose, join instructions, the riddle) are ONE variant joined with '\n'; call sites iterate .lines() so translators see whole passages.
// Adding a string = add a Msg variant; every language file then fails to build until its arm exists, so no English can silently leak into a translated UI.

use super::state::{ContactPage, SettingsPage};
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

/// The UI language — a device-local typed setting (display.lang, x-string code), seeded once from the OS locale at first launch, the user's after that.
/// Variants are declared in ADDITION ORDER, and `index()` follows it: the index is the session atomic's storage form, so a shipped index never moves. Picker order is `ALL`, which is free to re-sort (see the note there).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Es,
    Mi,
    Pt,
    Fr,
    Id,
    De,
    Sw,
    Vi,
    Tr,
    Ru,
    Hi,
    Uk,
    It,
    Pl,
    Fil,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Es => "es",
            Lang::Mi => "mi",
            Lang::Pt => "pt",
            Lang::Fr => "fr",
            Lang::Id => "id",
            Lang::De => "de",
            Lang::Sw => "sw",
            Lang::Vi => "vi",
            Lang::Tr => "tr",
            Lang::Ru => "ru",
            Lang::Hi => "hi",
            Lang::Uk => "uk",
            Lang::It => "it",
            Lang::Pl => "pl",
            Lang::Fil => "fil",
        }
    }
    /// The ONE seam the OS-locale seeding reads (platform::locale::os_language), so a language is discoverable from the host the moment its code lands here.
    pub fn from_code(c: &str) -> Option<Lang> {
        match c {
            "en" => Some(Lang::En),
            "es" => Some(Lang::Es),
            "mi" => Some(Lang::Mi),
            "pt" => Some(Lang::Pt),
            "fr" => Some(Lang::Fr),
            "id" => Some(Lang::Id),
            "de" => Some(Lang::De),
            "sw" => Some(Lang::Sw),
            "vi" => Some(Lang::Vi),
            "tr" => Some(Lang::Tr),
            "ru" => Some(Lang::Ru),
            "hi" => Some(Lang::Hi),
            "uk" => Some(Lang::Uk),
            "it" => Some(Lang::It),
            "pl" => Some(Lang::Pl),
            // Filipino answers to both its ISO 639-3 code and Tagalog's older two-letter one, which is what most hosts still report.
            "fil" | "tl" => Some(Lang::Fil),
            _ => None,
        }
    }
    // Autonyms are invariant across languages by design — a lost user must always recognise their own tongue in the picker. Bare language names, parallel form (Nick 2026-09-03: "Māori" not "Te Reo Māori" — te reo just means "the language").
    /// The form a speaker would use saying "I speak ___" in that language — so Tiếng Việt and Bahasa Indonesia keep their language word, which is NOT a contradiction of the bare-name rule: te reo detaches from Māori, tiếng and bahasa do not, and "Việt"/"Indonesia" alone name a people and a country rather than a language (Nick 2026-09-15).
    pub fn autonym(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Es => "Espa\u{f1}ol",
            Lang::Mi => "M\u{101}ori",
            Lang::Pt => "Portugu\u{EA}s",
            Lang::Fr => "Fran\u{E7}ais",
            Lang::Id => "Bahasa Indonesia",
            Lang::De => "Deutsch",
            Lang::Sw => "Kiswahili",
            Lang::Vi => "Ti\u{1EBF}ng Vi\u{1EC7}t",
            Lang::Tr => "T\u{FC}rk\u{E7}e",
            Lang::Ru => "\u{420}\u{443}\u{441}\u{441}\u{43A}\u{438}\u{439}",
            Lang::Hi => "\u{939}\u{93F}\u{928}\u{94D}\u{926}\u{940}",
            Lang::Uk => "\u{423}\u{43A}\u{440}\u{430}\u{457}\u{43D}\u{441}\u{44C}\u{43A}\u{430}",
            Lang::It => "Italiano",
            Lang::Pl => "Polski",
            Lang::Fil => "Filipino",
        }
    }
    /// STORAGE order — the session atomic's form. A shipped index NEVER moves; new languages append. (The picker uses ALL, not this.)
    pub fn index(self) -> usize {
        match self {
            Lang::En => 0,
            Lang::Es => 1,
            Lang::Mi => 2,
            Lang::Pt => 3,
            Lang::Fr => 4,
            Lang::Id => 5,
            Lang::De => 6,
            Lang::Sw => 7,
            Lang::Vi => 8,
            Lang::Tr => 9,
            Lang::Ru => 10,
            Lang::Hi => 11,
            Lang::Uk => 12,
            Lang::It => 13,
            Lang::Pl => 14,
            Lang::Fil => 15,
        }
    }
    pub fn from_index(i: usize) -> Lang {
        match i {
            1 => Lang::Es,
            2 => Lang::Mi,
            3 => Lang::Pt,
            4 => Lang::Fr,
            5 => Lang::Id,
            6 => Lang::De,
            7 => Lang::Sw,
            8 => Lang::Vi,
            9 => Lang::Tr,
            10 => Lang::Ru,
            11 => Lang::Hi,
            12 => Lang::Uk,
            13 => Lang::It,
            14 => Lang::Pl,
            15 => Lang::Fil,
            _ => Lang::En,
        }
    }
    /// DISPLAY order — sorted by autonym, because a user who cannot read the current UI language finds their own tongue by scanning, not by knowing where it was added. Latin-script names sort first and the Cyrillic/Devanagari names follow, which is simply codepoint order and stays deterministic.
    /// Safe to re-sort freely: the picker resolves a tap thru this array, never thru `index()`.
    pub const ALL: [Lang; 16] = [
        Lang::Id,
        Lang::De,
        Lang::En,
        Lang::Es,
        Lang::Fil,
        Lang::Fr,
        Lang::It,
        Lang::Sw,
        Lang::Mi,
        Lang::Pl,
        Lang::Pt,
        Lang::Vi,
        Lang::Tr,
        Lang::Ru,
        Lang::Uk,
        Lang::Hi,
    ];
}

// Session mirror of the language setting — a static so tr() reads the language without threading &self thru every draw call (same shape as DOZENAL_UI).
static CURRENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn lang() -> Lang {
    Lang::from_index(CURRENT.load(std::sync::atomic::Ordering::Relaxed) as usize)
}

pub fn set_lang(l: Lang) {
    CURRENT.store(l.index() as u8, std::sync::atomic::Ordering::Relaxed);
}

pub fn tr(msg: Msg) -> Cow<'static, str> {
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

pub enum Msg<'a> {
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
    EndCall,
    HangUp,
    Attest,
    Cancel,
    Arm,
    Disarm,
    Add,
    Update,
    // ---- page names (nav rail + contact tabs) ----
    PageName(SettingsPage),
    ContactPageName(ContactPage),
    // ---- call screen ----
    SpeakerToggleOn,
    SpeakerToggleOff,
    SpeakerPlain,
    EarpiecePlain,
    AddHandle,
    AddHandlePlain,
    BackToContact,
    CallStart,
    // Beam = video (wave's sibling) — stubbed, unwired; the label ships so the button can exist before the feature.
    BeamStart,
    /// The ring panel's audio answer (and the wave card's option): answering is choosing audio.
    WaveBack,
    /// The video answer / the wave card's option — a stub until video lands.
    BeamBack,
    /// The in-call video switch — a stub until video lands.
    BeamToggle,
    IncomingCall,
    IncomingCallNoPath,
    CallActiveNoPath(&'a str),
    CallReconnecting,
    /// Answer tapped but no frame could go out (the friendship is mid-ceremony) — the ring keeps going, the person needs to know why nothing happened.
    AnswerFailedReconnecting(&'a str),
    CallDroppedRow,
    /// A dropped wave with its live duration — the wave card header.
    CallDroppedDur(&'a str),
    CallChipElsewhere(&'a str),
    CallChipElsewhereUnknown,
    StopPlayback,
    CallEndedDur(&'a str),
    CallingName(&'a str),
    CallRow,
    MissedCallRow,
    CallDeclinedRow,
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
    LaunchJoinConfirmNote,
    StartFreshIdle,
    StartFreshArmed,
    JoinerSelected,
    JoinerConfirmOther,
    OffGridListening,
    AddedHandle(&'a str),
    AlreadyAdded(&'a str),
    NotFound,
    SearchError(&'a str),
    AlreadyInContacts,
    Attesting,
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
    /// Plain reading of a DMS value (bit length of seconds ago) — the legend's third column; empty for values the legend doesn't list.
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
    /// The length legend: doublings of the hydrogen line's wavelength; the reading takes the signed doubling count.
    DmsLengthHead,
    DmsLengthIntro,
    DmsLengthReading(i32),
    /// The hex page's one line on lengths: millimetres, linear.
    HexLengthNote,
    /// Zero has no logarithm: an age of nothing and a size of nothing read as words (pure-log DMS, 2026-09-11).
    DmsNow,
    DmsEmpty,
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
    NotesToSelf,
    // The param is the CLUTCH ladder status string (already narrated elsewhere), passed thru verbatim.
    ClutchStatus(&'a str),
    // The ceremony ladder's step text, zero-indexed 0..=11 — the prefix (.⟨dozenal digit⟩ or n/12) is the caller's (Contact::clutch_status_detail).
    ClutchStep(u8),
    ClutchSecured,
    // ---- compact call bar ----
    CallBarCalling(&'a str),
    CallBarInCall(&'a str),
    CallBarKeepRecording,
    NoDirectPathSuffix,
    // ---- message details strip + action pills ----
    SentDetail { age: &'a str, state: &'a str },
    ReceivedDetail(&'a str),
    DeliveryDelivered,
    DeliveryReplicated,
    DeliverySending,
    RecoveredSuffix,
    EditedSuffix,
    /// One prior version of an edited row in the selected meta (chat.history on): its text as it stood, stamped with its age.
    EditWasLine { age: &'a str, text: &'a str },
    BlobDeliveredSuffix,
    BlobSendingSuffix,
    BlobNotHereSuffix,
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
    /// LEAVER's Security page while its departure request is pending: the approval words the approver must type.
    DepartWordsShow(&'a str),
    /// LEAVER's pending line under the words.
    DepartWaitingLine,
    /// APPROVER's Fleet card: the leaver declared new-owner intent.
    DepartIntentNewOwner,
    /// APPROVER's Fleet card: the leaver declared desk (keeping-it) intent.
    DepartIntentDesk,
    /// APPROVER's words-entry prompt above the box.
    DepartWordsPrompt,
    /// Typed words don't match the request's commitment.
    DepartWordsMismatch,
    /// New-owner departure fully completed (countersigned + brand released).
    DepartCompleteNewOwner(&'a str),
    /// Countersign landed but the release half failed — the retired row's Release pill is the retry.
    DepartReleasePending(&'a str),
    /// Attest refused because the hardware is still branded to another identity — the honest joiner-side message.
    DeviceBrandedHint,
    CopiedPill,
    ResendPill,
    FetchPill,
    SavePill,
    /// A wave's recording to a file (the wave card's word for save) and a wave off the timeline (its word for delete), Nick 2026-09-12.
    ExportPill,
    DiscardPill,
    /// "Keep it, but not here": drop this device's copy of an incoming pigeon's bytes — the row and re-fetch stay.
    LoftPill,
    /// The details strip's stats for an attachment row (Nick 2026-09-12: "stats up top: name, time, size, type"): pre-formatted parts, the age follows on the same line.
    AttachStats { name: &'a str, kind: &'a str, size: &'a str, dims: &'a str },
    /// The kind as a word for the stats line.
    AttachKindName(crate::types::AttachKind),
    PlayPill,
    DeletePill,
    DeletingPill,
    StopPill,
    // ---- contact panel (About / Stats / Manage) ----
    OwnNotesConversation,
    NameNotShared,
    NameShared(&'a str),
    AvatarNotShared,
    AvatarShared,
    SelfNoCeremony,
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
    OwnNotesCantBoot,
    SiblingSignsItselfOut,
    BootPill { armed: bool },
    // ---- groups (docs/groups.md §10.5) ----
    /// The Manage page pill that opens the group picker.
    BringIntoGroup,
    /// The picker's "start a new group" row.
    NewGroup,
    /// The New-group title box hint.
    GroupTitlePrompt,
    /// The New-group commit pill.
    FoundGroupPill,
    /// History policy pills, fixed at birth (D5).
    HistoryFromGenesis,
    HistoryFromJoin,
    /// Header suffix when nobody else stands.
    GroupAlone,
    /// Header suffix while our Join awaits the sponsor's wrap.
    JoiningStatus,
    /// Header suffix while an era's wrap has not reached this device.
    CatchingUpStatus,
    /// Header suffix after we left.
    LeftStatus,
    /// A member whose fold has not yet succeeded — drawn as a contact without a name is.
    PendingMember,
    /// The compose bar's honest label while group sends are not yet wired.
    GroupComposeSoon,
    /// Group panel (docs/groups.md §10.5).
    GroupPageName(crate::ui::state::GroupPage),
    Members,
    MemberStanding,
    MemberDeparted,
    MemberPendingName,
    /// Our own row in a member list.
    YouLabel,
    EraIndex { n: &'a str },
    MutePill { muted: bool },
    LeaveGroupPill { armed: bool },
    LeaveGroupNote,
    AddToGroupNote,
    NobodyToAdd,
    YouLeftNote,
    /// The offer card (docs/groups.md §10.1), invitee side: "<sponsor> brought you into <title> · <n>".
    OfferLine { sponsor: &'a str, title: &'a str, n: &'a str },
    /// The card once joined.
    OfferJoined { title: &'a str },
    /// The card once the sponsor is gone.
    OfferExpired,
    /// The Join pill.
    OfferJoin,
    /// Sponsor side: waiting on the invitee.
    OfferWaiting { name: &'a str, title: &'a str },
    /// Sponsor side: the invitee stands.
    OfferAccepted { name: &'a str, title: &'a str },
    BootRemovesEverywhere,
    BootOstracism,
    // ---- add device / pairing ----
    AddDeviceTitle,
    AddDeviceConfirmOnce,
    TapNearby,
    YesGreenFinish,
    TapOrbCancel,
    AddDeviceNearby(&'a str),
    TypeWords,
    InvalidWord(&'a str),
    WaitingForDevice,
    DevicesAskingToJoin(usize),
    NoMatchingDevice(&'a str),
    MatchingDevice(&'a str),
    MatchingMultiple(usize),
    WordsMatched,
    Finishing,
    Preparing,
    NoFleetKey,
    NoDeviceKey,
    BoundDeviceConfirm(&'a str),
    AddingDevice(&'a str),
    DeviceAdded,
    AddDeviceError(&'a str),
    AddFromSignedIn,
    JoinFailed(&'a str),
    RequestFailed(&'a str),
    // ---- fleet page ----
    FleetTitle,
    FleetTapToCopy,
    ThisDevice,
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
    ADevice,
    FoundNearby(&'a str),
    DepartureApproval(&'a str),
    FleetSignedOut(&'a str),
    SignOutPublishFailed,
    LastDeviceCantSignOut,
    NoIdentityToRemove,
    SignOutRequested,
    SignOutBuildFailed,
    DeviceReleased(&'a str),
    ReleaseFailed,
    DeviceReinstatedToast(&'a str),
    RevokedNotice(&'a str),
    ConfirmReinstate(&'a str),
    ConfirmRevoke { name: &'a str, last_unlocker: bool },
    CopiedName(&'a str),
    ReleasePill { armed: bool },
    BridgePill,
    ApproveSignOutPill { armed: bool },
    ReinstatePill { armed: bool },
    RevokePill { armed: bool },
    SingleCopyWarning,
    DeviceSignsItselfOut,
    AddDevicePill,
    RenamePill,
    // ---- security page ----
    SecurityLock,
    /// KILL (Nick 2026-09-10): one tap drops the identity and ends the process this instant; the vault stays.
    SecurityKill,
    SecurityKillHint,
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
    RingIncomingCall,
    /// Notifications page: hold every wave recording on this device (replication by default).
    HoldWavesOnDevice,
    KeepEditHistory,
    VibrateIncomingCall,
    PresenceCheckbox,
    PerContactOverride,
    // ---- updates ----
    UpdatesTitle,
    /// "What's new in {version}" — the release-notes heading on the Updates page.
    WhatsNew(&'a str),
    /// Heading for the not-yet-shipped changes a dev build carries.
    UpcomingChanges,
    /// Under the dev pill on the Updates page.
    DevChannelHint,
    PhotonVersion(&'a str),
    AutoUpdateCheck,
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
    LogSize(&'a str),
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
    LogTitle,
    DiagBack,
    DiagClear,
    DiagSnapshot,
    DiagSubmit,
    DiagView,
    OptionalNote,
    // ---- you / profile ----
    YouAddCustomField,
    YouIdentity,
    YouNote,
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
    OpenPill,
    ViewerBack,
    ViewerOriginal,
    ViewerDecoding,
    /// The call panel's live line: rung name (a proper noun, untranslated), round trip as a frequency, loss of 256, buffer frames.
    CallLiveStats { rung: &'a str, freq: &'a str, loss: &'a str, buf: &'a str },
    /// Viewer exposure row: the clip-view pill, and the caption's exposure readout (stops, signed).
    ClipPill,
    ExposureStops(&'a str),
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
    // The decimal-mode swap: the question turned round. Shown INSTEAD of WhyDozenal/WhyDozenalProse while the toggle is off — no scold line, the red box carries the disapproval.
    WhyYouDozenal,
    // '\n'-joined paragraphs like WhyDozenalProse. Dozenal glyph control codes (0x10..0x1B) ride inline; the font chain draws them from any family.
    WhyYouDozenalProse,
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
