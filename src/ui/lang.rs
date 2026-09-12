// The language catalog: every user-facing string is a Msg variant, every language is one exhaustive match, and the compiler is the completeness checker.
// Doctrine (docs/languages.md): keys are WHOLE messages with holes (never concatenate translated fragments); variants carry semantic values (counts, names) and each language renders its own word order, plurals, and gender.
// Never translated: handles (byte-precise), voca pairing words (protocol material), log lines (photonlog grep-ability), dozenal digit names (Zil/Ter/Lun/Stel — invented photon vocabulary, universal like the glyphs), VSF field names and storage keys, the brand word "Photon".
// Numbers inside translated strings render at this edge per the number doctrine: EVERY numeral goes thru crate::fmt_num (or fmt_i when signed). The one deliberate exception is Msg::FileBubble, whose size draws in a face that can't resolve the dozenal glyph bytes — see the comment there; it is a font problem, not a licence for raw arabic anywhere else.
// Multi-line passages (About prose, join instructions, the riddle) are ONE variant joined with '\n'; call sites iterate .lines() so translators see whole passages.
// Adding a string = add a Msg variant; every language file then fails to build until its arm exists, so no English can silently leak into a translated UI.

use super::state::{ContactPage, SettingsPage};
use std::borrow::Cow;

pub mod en;
pub mod es;
pub mod mi;

/// The UI language — a device-local typed setting (display.lang, x-string code), seeded once from the OS locale at first launch, the user's after that.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Es,
    Mi,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Es => "es",
            Lang::Mi => "mi",
        }
    }
    pub fn from_code(c: &str) -> Option<Lang> {
        match c {
            "en" => Some(Lang::En),
            "es" => Some(Lang::Es),
            "mi" => Some(Lang::Mi),
            _ => None,
        }
    }
    // Autonyms are invariant across languages by design — a lost user must always recognise their own tongue in the picker. Bare language names, parallel form (Nick 2026-09-03: "Māori" not "Te Reo Māori" — te reo just means "the language").
    pub fn autonym(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Es => "Espa\u{f1}ol",
            Lang::Mi => "M\u{101}ori",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Lang::En => 0,
            Lang::Es => 1,
            Lang::Mi => 2,
        }
    }
    pub fn from_index(i: usize) -> Lang {
        match i {
            1 => Lang::Es,
            2 => Lang::Mi,
            _ => Lang::En,
        }
    }
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Es, Lang::Mi];
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
