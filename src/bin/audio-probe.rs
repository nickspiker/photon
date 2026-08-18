//! Loopback probe for the call-audio substrate (docs/calls.md step 1): mic → 200ms delay → speaker, ten seconds, then stats. Proves capture, playback, the resampler pair, and the AEC reference ring on THIS machine before any call code exists. Run with speakers low or a headset — you will hear yourself.

#[cfg(not(any(target_os = "android", target_os = "redox")))]
fn main() {
    use photon_messenger::platform::audio;
    use std::collections::VecDeque;

    println!("audio-probe: starting 10s mic→200ms→speaker loopback (route sniff + frame stats)");
    if !audio::start() {
        eprintln!("audio-probe: audio session failed to start");
        std::process::exit(1);
    }

    let started = std::time::Instant::now();
    let mut delay: VecDeque<Vec<i16>> = VecDeque::new();
    let mut captured = 0usize;
    let mut peak: i16 = 0;
    while started.elapsed() < std::time::Duration::from_secs(10) {
        for frame in audio::captured_frames() {
            captured += 1;
            peak = peak.max(frame.iter().map(|s| s.saturating_abs()).max().unwrap_or(0));
            delay.push_back(frame);
            // 20 frames × 10ms = the 200ms loopback delay.
            if delay.len() > 20 {
                audio::queue_playback(delay.pop_front().unwrap());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let reference = audio::render_reference().len();
    audio::stop();

    println!(
        "audio-probe: {} frames captured ({:.1}s of audio), peak {}, route {:?}, AEC reference ring {} frames",
        captured,
        captured as f64 * 0.010,
        peak,
        audio::route(),
        reference
    );
    if captured == 0 {
        println!("audio-probe: NO capture — check mic device/permissions");
    }
}

#[cfg(any(target_os = "android", target_os = "redox"))]
fn main() {
    println!("audio-probe: desktop-only (Android capture rides the Kotlin service)");
}
