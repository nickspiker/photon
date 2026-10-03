//! macOS: Apple's Voice Processing I/O unit in front of the mic (Nick 2026-10-03, "what do we need to do to wire up macOS?" — "Go for it!").
//!
//! One Core Audio unit of the VoiceProcessingIO subtype owns BOTH directions: the far party's frames go in on its output bus thru a render callback, and the mic comes out of its input bus already echo-cancelled against what it played.
//! That is the same shape the Android backend has (audio_aaudio.rs): `next_render_frame_at` feeds the DAC, the input callback feeds `push_captured`, and every frame is stamped from the host clock the unit hands each buffer, mapped onto the boot clock exactly as the cpal path maps its callback delays.
//! The unit's gain control is turned OFF (the one property the phone never offered), so the calibrated level plan keeps its meaning here; its echo cancellation and noise suppression stay on.
//! Chosen by `platform::audio::voice_dsp_wanted`: the built-in speakers always, headphones and headsets only on the Wave page's say-so. The plain cpal path is untouched for every other case.
//! The FFI is hand-declared (a dozen functions, four structs, the constants) rather than bindgen'd: the bindings crate needs the macOS SDK at build time, which the Linux build machine has no way to offer, and the surface here is small and frozen since 10.15.

use super::audio::{next_render_frame_at, push_captured, FRAME_SAMPLES, SAMPLE_RATE};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

type OSStatus = i32;
type AudioComponent = *mut c_void;
type AudioUnit = *mut c_void;

#[repr(C)]
struct AudioComponentDescription {
    component_type: u32,
    sub_type: u32,
    manufacturer: u32,
    flags: u32,
    flags_mask: u32,
}

#[repr(C)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C)]
struct AudioBuffer {
    number_channels: u32,
    data_byte_size: u32,
    data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    number_buffers: u32,
    buffers: [AudioBuffer; 1],
}

#[repr(C)]
struct SmpteTime {
    subframes: i16,
    subframe_divisor: i16,
    counter: u32,
    kind: u32,
    flags: u32,
    hours: i16,
    minutes: i16,
    seconds: i16,
    frames: i16,
}

#[repr(C)]
struct AudioTimeStamp {
    sample_time: f64,
    host_time: u64,
    rate_scalar: f64,
    word_clock_time: u64,
    smpte_time: SmpteTime,
    flags: u32,
    reserved: u32,
}

type AURenderCallback = unsafe extern "C" fn(*mut c_void, *mut u32, *const AudioTimeStamp, u32, u32, *mut AudioBufferList) -> OSStatus;

#[repr(C)]
struct AURenderCallbackStruct {
    input_proc: Option<AURenderCallback>,
    input_proc_ref_con: *mut c_void,
}

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioComponentFindNext(in_component: AudioComponent, desc: *const AudioComponentDescription) -> AudioComponent;
    fn AudioComponentInstanceNew(component: AudioComponent, out: *mut AudioUnit) -> OSStatus;
    fn AudioComponentInstanceDispose(unit: AudioUnit) -> OSStatus;
    fn AudioUnitSetProperty(unit: AudioUnit, id: u32, scope: u32, element: u32, data: *const c_void, size: u32) -> OSStatus;
    fn AudioUnitInitialize(unit: AudioUnit) -> OSStatus;
    fn AudioUnitUninitialize(unit: AudioUnit) -> OSStatus;
    fn AudioOutputUnitStart(unit: AudioUnit) -> OSStatus;
    fn AudioOutputUnitStop(unit: AudioUnit) -> OSStatus;
    fn AudioUnitRender(unit: AudioUnit, flags: *mut u32, ts: *const AudioTimeStamp, bus: u32, frames: u32, list: *mut AudioBufferList) -> OSStatus;
}

extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut [u32; 2]) -> i32;
}

// FourCC constants, as Apple spells them.
const K_AUDIO_UNIT_TYPE_OUTPUT: u32 = u32::from_be_bytes(*b"auou");
const K_AUDIO_UNIT_SUB_TYPE_VOICE_PROCESSING_IO: u32 = u32::from_be_bytes(*b"vpio");
const K_AUDIO_UNIT_MANUFACTURER_APPLE: u32 = u32::from_be_bytes(*b"appl");
const K_AUDIO_FORMAT_LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
const K_AUDIO_FORMAT_FLAG_IS_FLOAT: u32 = 1;
const K_AUDIO_FORMAT_FLAG_IS_PACKED: u32 = 8;
const K_AUDIO_UNIT_SCOPE_GLOBAL: u32 = 0;
const K_AUDIO_UNIT_SCOPE_INPUT: u32 = 1;
const K_AUDIO_UNIT_SCOPE_OUTPUT: u32 = 2;
const K_AUDIO_UNIT_PROPERTY_STREAM_FORMAT: u32 = 8;
const K_AUDIO_UNIT_PROPERTY_MAXIMUM_FRAMES_PER_SLICE: u32 = 14;
const K_AUDIO_UNIT_PROPERTY_SET_RENDER_CALLBACK: u32 = 23;
const K_AUDIO_OUTPUT_UNIT_PROPERTY_ENABLE_IO: u32 = 2003;
const K_AUDIO_OUTPUT_UNIT_PROPERTY_SET_INPUT_CALLBACK: u32 = 2005;
/// The unit's automatic gain control, off: the level plan's calibration stays this mic's own (the phone never offered this switch).
const K_AU_VOICE_IO_PROPERTY_VOICE_PROCESSING_ENABLE_AGC: u32 = 2101;
/// The unit's input bus (the mic) and output bus (the speaker).
const BUS_INPUT: u32 = 1;
const BUS_OUTPUT: u32 = 0;
/// The most frames one callback may ask for; the unit's own slices are smaller.
const MAX_FRAMES_PER_SLICE: u32 = 4096;

/// The capture side's state, shared with the unit's input callback.
struct Capture {
    /// Resampled 48 kHz mono samples not yet framed (the unit runs at our rate, so this only holds the remainder of a slice).
    pending: Vec<f32>,
    /// The 48 kHz position of `pending[0]`.
    framed_pos: i64,
    /// The float-cast carry (as the cpal path: floor with the fraction carried, zero-mean at 24 bits).
    cast_carry: f64,
    /// The scratch the unit renders the mic into.
    scratch: Vec<f32>,
}

/// The render side's state, shared with the unit's render callback.
struct Render {
    staged: VecDeque<f32>,
}

static CAPTURE: Mutex<Capture> = Mutex::new(Capture { pending: Vec::new(), framed_pos: 0, cast_carry: 0.0, scratch: Vec::new() });
static RENDER: Mutex<Render> = Mutex::new(Render { staged: VecDeque::new() });
/// The live unit (a pointer, as a usize so the static is Send).
static UNIT: Mutex<usize> = Mutex::new(0);
static LIVE: AtomicBool = AtomicBool::new(false);
/// mach timebase numerator / denominator: host ticks × numer / denom = nanoseconds.
static TIMEBASE: Mutex<(u32, u32)> = Mutex::new((1, 1));

/// Host ticks → nanoseconds.
fn ticks_to_ns(ticks: i64) -> i64 {
    let (n, d) = *TIMEBASE.lock().unwrap();
    (ticks as i128 * n as i128 / d.max(1) as i128) as i64
}

/// The boot-clock instant of a host time stamp: now, less however far back the stamp lies (both clocks tick together while awake).
fn boot_at_host_time(host_time: u64) -> i64 {
    let now_ticks = unsafe { mach_absolute_time() };
    let behind = ticks_to_ns(now_ticks as i64 - host_time as i64);
    crate::network::time_base::boot_now() - crate::network::time_base::boot_ns_to_osc(behind)
}

fn set_u32(unit: AudioUnit, id: u32, scope: u32, element: u32, v: u32) -> OSStatus {
    unsafe { AudioUnitSetProperty(unit, id, scope, element, &v as *const u32 as *const c_void, std::mem::size_of::<u32>() as u32) }
}

fn mono_f32_48k() -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        sample_rate: SAMPLE_RATE as f64,
        format_id: K_AUDIO_FORMAT_LINEAR_PCM,
        format_flags: K_AUDIO_FORMAT_FLAG_IS_FLOAT | K_AUDIO_FORMAT_FLAG_IS_PACKED,
        bytes_per_packet: 4,
        frames_per_packet: 1,
        bytes_per_frame: 4,
        channels_per_frame: 1,
        bits_per_channel: 32,
        reserved: 0,
    }
}

/// The unit has a mic slice for us: render it out of the input bus, cast it at 24 bits, frame it, stamp each frame from the slice's host time.
unsafe extern "C" fn input_proc(ref_con: *mut c_void, flags: *mut u32, ts: *const AudioTimeStamp, _bus: u32, frames: u32, _list: *mut AudioBufferList) -> OSStatus {
    let unit = ref_con as AudioUnit;
    if unit.is_null() || ts.is_null() || frames == 0 {
        return 0;
    }
    let mut cap = CAPTURE.lock().unwrap();
    let n = frames as usize;
    if cap.scratch.len() < n {
        cap.scratch.resize(n, 0.0);
    }
    let mut list = AudioBufferList {
        number_buffers: 1,
        buffers: [AudioBuffer { number_channels: 1, data_byte_size: (n * 4) as u32, data: cap.scratch.as_mut_ptr() as *mut c_void }],
    };
    let st = AudioUnitRender(unit, flags, ts, BUS_INPUT, frames, &mut list);
    if st != 0 {
        return st;
    }
    // When this slice's first sample was captured: the stamp's host time on the boot clock.
    let cap_boot = boot_at_host_time((*ts).host_time);
    let anchor = (cap.framed_pos + cap.pending.len() as i64, crate::network::time_base::eagle_at_boot_rt(cap_boot));
    let slice: Vec<f32> = cap.scratch[..n].to_vec();
    cap.pending.extend_from_slice(&slice);
    while cap.pending.len() >= FRAME_SAMPLES {
        let mut carry = cap.cast_carry;
        let frame: Vec<i32> = cap
            .pending
            .drain(..FRAME_SAMPLES)
            .map(|s| {
                // WHY/PROOF: the 24-bit domain's rails, applied where the unit's float enters (as the cpal and AAudio paths do).
                let acc = (s as f64 * 8_388_608.0).clamp(-8_388_608.0, 8_388_607.0) + carry;
                let out = acc.floor();
                carry = acc - out;
                out as i32
            })
            .collect();
        cap.cast_carry = carry;
        let at = anchor.1 + ((cap.framed_pos - anchor.0) as i128 * crate::OSC_PER_SEC as i128 / SAMPLE_RATE as i128) as i64;
        push_captured(at, cap.framed_pos, frame);
        cap.framed_pos += FRAME_SAMPLES as i64;
    }
    0
}

/// The unit wants a speaker slice: pull named frames for the instant this slice reaches the DAC and hand them over as float.
unsafe extern "C" fn render_proc(_ref_con: *mut c_void, _flags: *mut u32, ts: *const AudioTimeStamp, _bus: u32, frames: u32, list: *mut AudioBufferList) -> OSStatus {
    if list.is_null() || (*list).number_buffers == 0 {
        return 0;
    }
    let buf = &mut (*list).buffers[0];
    let n = frames as usize;
    if buf.data.is_null() || (buf.data_byte_size as usize) < n * 4 {
        return 0;
    }
    let out = std::slice::from_raw_parts_mut(buf.data as *mut f32, n);
    // When this slice's first sample reaches the DAC: the stamp's host time, ahead of now.
    let dac_boot = if ts.is_null() { crate::network::time_base::boot_now() } else { boot_at_host_time((*ts).host_time) };
    let mut r = RENDER.lock().unwrap();
    while r.staged.len() < n {
        let offset = r.staged.len() as i128 * crate::OSC_PER_SEC as i128 / SAMPLE_RATE as i128;
        let frame = next_render_frame_at(crate::network::time_base::eagle_at_boot_rt(dac_boot + offset as i64));
        r.staged.extend(frame.iter().map(|&s| s as f32 / 32768.0));
    }
    for o in out.iter_mut() {
        *o = r.staged.pop_front().unwrap_or(0.0);
    }
    0
}

/// Open and start the unit. `false` (with the reason logged) hands the wave to the plain cpal path.
pub fn start() -> bool {
    if LIVE.load(Ordering::SeqCst) {
        return true;
    }
    let mut tb = [0u32; 2];
    if unsafe { mach_timebase_info(&mut tb) } == 0 && tb[1] != 0 {
        *TIMEBASE.lock().unwrap() = (tb[0], tb[1]);
    }
    let desc = AudioComponentDescription {
        component_type: K_AUDIO_UNIT_TYPE_OUTPUT,
        sub_type: K_AUDIO_UNIT_SUB_TYPE_VOICE_PROCESSING_IO,
        manufacturer: K_AUDIO_UNIT_MANUFACTURER_APPLE,
        flags: 0,
        flags_mask: 0,
    };
    let comp = unsafe { AudioComponentFindNext(std::ptr::null_mut(), &desc) };
    if comp.is_null() {
        crate::log("AUDIO: voice processing unit not found on this Mac — the plain path runs");
        return false;
    }
    let mut unit: AudioUnit = std::ptr::null_mut();
    let st = unsafe { AudioComponentInstanceNew(comp, &mut unit) };
    if st != 0 || unit.is_null() {
        crate::logf!("AUDIO: voice processing unit refused to instantiate ({}) — the plain path runs", st);
        return false;
    }
    // Each step names its own failure; a refusal anywhere tears the unit down and the plain path runs.
    let fail = |what: &str, st: OSStatus| -> bool {
        crate::logf!("AUDIO: voice processing unit — {} failed ({}); the plain path runs", what, st);
        unsafe {
            AudioComponentInstanceDispose(unit);
        }
        false
    };
    let st = set_u32(unit, K_AUDIO_OUTPUT_UNIT_PROPERTY_ENABLE_IO, K_AUDIO_UNIT_SCOPE_INPUT, BUS_INPUT, 1);
    if st != 0 {
        return fail("enabling the mic bus", st);
    }
    let st = set_u32(unit, K_AUDIO_OUTPUT_UNIT_PROPERTY_ENABLE_IO, K_AUDIO_UNIT_SCOPE_OUTPUT, BUS_OUTPUT, 1);
    if st != 0 {
        return fail("enabling the speaker bus", st);
    }
    // Our formats: 48 kHz mono float on the mic bus's output side (what the unit hands us) and the speaker bus's input side (what we hand it).
    let fmt = mono_f32_48k();
    let st = unsafe { AudioUnitSetProperty(unit, K_AUDIO_UNIT_PROPERTY_STREAM_FORMAT, K_AUDIO_UNIT_SCOPE_OUTPUT, BUS_INPUT, &fmt as *const _ as *const c_void, std::mem::size_of::<AudioStreamBasicDescription>() as u32) };
    if st != 0 {
        return fail("the mic format (48 kHz mono float)", st);
    }
    let st = unsafe { AudioUnitSetProperty(unit, K_AUDIO_UNIT_PROPERTY_STREAM_FORMAT, K_AUDIO_UNIT_SCOPE_INPUT, BUS_OUTPUT, &fmt as *const _ as *const c_void, std::mem::size_of::<AudioStreamBasicDescription>() as u32) };
    if st != 0 {
        return fail("the speaker format (48 kHz mono float)", st);
    }
    let st = set_u32(unit, K_AUDIO_UNIT_PROPERTY_MAXIMUM_FRAMES_PER_SLICE, K_AUDIO_UNIT_SCOPE_GLOBAL, 0, MAX_FRAMES_PER_SLICE);
    if st != 0 {
        return fail("the slice ceiling", st);
    }
    // The gain control OFF: the mic's level stays its own, the level plan keeps its calibration. (A refusal here is logged, not fatal — an older unit without the switch still cancels.)
    let st = set_u32(unit, K_AU_VOICE_IO_PROPERTY_VOICE_PROCESSING_ENABLE_AGC, K_AUDIO_UNIT_SCOPE_GLOBAL, 0, 0);
    if st != 0 {
        crate::logf!("AUDIO: voice processing unit — its gain control could not be switched off ({}); the level plan stands down as on the phone", st);
    }
    let agc_off = st == 0;
    let input_cb = AURenderCallbackStruct { input_proc: Some(input_proc), input_proc_ref_con: unit as *mut c_void };
    let st = unsafe { AudioUnitSetProperty(unit, K_AUDIO_OUTPUT_UNIT_PROPERTY_SET_INPUT_CALLBACK, K_AUDIO_UNIT_SCOPE_GLOBAL, 0, &input_cb as *const _ as *const c_void, std::mem::size_of::<AURenderCallbackStruct>() as u32) };
    if st != 0 {
        return fail("the mic callback", st);
    }
    let render_cb = AURenderCallbackStruct { input_proc: Some(render_proc), input_proc_ref_con: std::ptr::null_mut() };
    let st = unsafe { AudioUnitSetProperty(unit, K_AUDIO_UNIT_PROPERTY_SET_RENDER_CALLBACK, K_AUDIO_UNIT_SCOPE_INPUT, BUS_OUTPUT, &render_cb as *const _ as *const c_void, std::mem::size_of::<AURenderCallbackStruct>() as u32) };
    if st != 0 {
        return fail("the speaker callback", st);
    }
    {
        let mut cap = CAPTURE.lock().unwrap();
        cap.pending.clear();
        cap.framed_pos = 0;
        cap.cast_carry = 0.0;
        RENDER.lock().unwrap().staged.clear();
    }
    let st = unsafe { AudioUnitInitialize(unit) };
    if st != 0 {
        return fail("initialising", st);
    }
    let st = unsafe { AudioOutputUnitStart(unit) };
    if st != 0 {
        unsafe {
            AudioUnitUninitialize(unit);
        }
        return fail("starting", st);
    }
    *UNIT.lock().unwrap() = unit as usize;
    LIVE.store(true, Ordering::SeqCst);
    // What is in front of the mic now: the level plan, the duck and the loss read this (see platform::audio::VOICE_DSP_ACTIVE). With the gain control off the calibrated plan keeps running; with it on, the plan stands down as on the phone.
    super::audio::VOICE_DSP_ACTIVE.store(!agc_off, Ordering::Relaxed);
    super::audio::VOICE_DSP_CANCELS.store(true, Ordering::Relaxed);
    super::audio::set_mic_identity("mic:voice-processing".to_string());
    crate::logf!("AUDIO: voice processing unit up — 48 kHz mono float both ways, echo cancellation on, gain control {}", if agc_off { "off" } else { "ON (could not be disabled)" });
    true
}

/// Stop and dispose the unit.
pub fn stop() {
    if !LIVE.swap(false, Ordering::SeqCst) {
        return;
    }
    let unit = std::mem::take(&mut *UNIT.lock().unwrap()) as AudioUnit;
    if !unit.is_null() {
        unsafe {
            AudioOutputUnitStop(unit);
            AudioUnitUninitialize(unit);
            AudioComponentInstanceDispose(unit);
        }
    }
    super::audio::VOICE_DSP_ACTIVE.store(false, Ordering::Relaxed);
    super::audio::VOICE_DSP_CANCELS.store(false, Ordering::Relaxed);
    crate::log("AUDIO: voice processing unit down");
}

/// Is the unit live.
pub fn live() -> bool {
    LIVE.load(Ordering::SeqCst)
}
