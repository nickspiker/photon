//! Test to verify spaghettify determinism across platforms
//!
//! Run on both Linux x86_64 and Android ARM to compare outputs.
//! Expected: Basic IEEE ops match, transcendentals diverge.

use std::convert::TryInto;
use std::f64::consts::PI;

fn main() {
    // Test input that mimics what conversation token derivation would produce
    let test_input: [u8; 64] = [
        // "PHOTON_CONVERSATION_v0" domain prefix + two 32-byte handle hashes
        0x50, 0x48, 0x4f, 0x54, 0x4f, 0x4e, 0x5f, 0x43, 0x4f, 0x4e, 0x56, 0x45, 0x52, 0x53, 0x41,
        0x54, 0x49, 0x4f, 0x4e, 0x5f, 0x76, 0x30, 0x4c, 0x1f, 0x3d, 0x33, 0x98, 0xb9, 0x72, 0xb1,
        0x4f, 0x7f, 0x5a, 0x5c, 0x8f, 0x58, 0xd1, 0x0f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];

    // Reinterpret as f64s (like chaos functions do)
    let f0 = f64::from_be_bytes(test_input[0..8].try_into().unwrap());
    let f1 = f64::from_be_bytes(test_input[8..16].try_into().unwrap());
    let f2 = f64::from_be_bytes(test_input[16..24].try_into().unwrap());
    let f3 = f64::from_be_bytes(test_input[24..32].try_into().unwrap());

    println!("=== IEEE 754 Transcendental Function Test ===");
    println!("Input f64 bit patterns:");
    println!("  f0 = {:016x} ({:e})", f0.to_bits(), f0);
    println!("  f1 = {:016x} ({:e})", f1.to_bits(), f1);
    println!("  f2 = {:016x} ({:e})", f2.to_bits(), f2);
    println!("  f3 = {:016x} ({:e})", f3.to_bits(), f3);

    println!("\n=== Basic IEEE ops (should match across platforms) ===");
    println!("f0 + f1 = {:016x}", (f0 + f1).to_bits());
    println!("f0 - f1 = {:016x}", (f0 - f1).to_bits());
    println!("f0 * f1 = {:016x}", (f0 * f1).to_bits());
    println!("f0 / f1 = {:016x}", (f0 / f1).to_bits());
    println!("f0.sqrt() = {:016x}", f0.sqrt().to_bits());

    println!("\n=== Transcendentals (MAY DIFFER across platforms) ===");
    println!("f0.sin() = {:016x}", f0.sin().to_bits());
    println!("f0.cos() = {:016x}", f0.cos().to_bits());
    println!("f0.exp() = {:016x}", f0.exp().to_bits());
    println!("f0.ln()  = {:016x}", f0.ln().to_bits());
    println!("f0.tan() = {:016x}", f0.tan().to_bits());
    println!("f0.atan() = {:016x}", f0.atan().to_bits());
    println!("f0.powf(f1) = {:016x}", f0.powf(f1).to_bits());
    println!("f0.hypot(f1) = {:016x}", f0.hypot(f1).to_bits());

    // Test with a known good value
    println!("\n=== Known value tests ===");
    let pi = PI;
    println!("PI.sin() = {:016x} (expected ~0)", pi.sin().to_bits());
    println!(
        "(PI/2).sin() = {:016x} (expected 1.0)",
        (pi / 2.0).sin().to_bits()
    );
    println!(
        "(PI/4).tan() = {:016x} (expected 1.0)",
        (pi / 4.0).tan().to_bits()
    );

    // Test NaN propagation (this SHOULD be consistent per IEEE 754)
    let nan = f64::NAN;
    let inf = f64::INFINITY;
    println!("\n=== NaN/Inf tests (should match) ===");
    println!("NaN bits = {:016x}", nan.to_bits());
    println!("Inf bits = {:016x}", inf.to_bits());
    println!("NaN + 1.0 = {:016x}", (nan + 1.0).to_bits());
    println!("0.0 / 0.0 = {:016x}", (0.0_f64 / 0.0).to_bits());
    println!("Inf - Inf = {:016x}", (inf - inf).to_bits());
}
