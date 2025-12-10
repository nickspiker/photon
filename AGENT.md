# AGENT.md - Code Generation Rules

## Rule 0: Bounds Checks and Saturating Arithmetic

**IF YOU ADD ANY BOUNDS CHECK OR SATURATING ARITHMETIC, YOU ARE REQUIRED TO:**
0. **STATE WHY** it was added
1. **PROVE** it was necessary
2. **EXPLAIN** what undefined behavior or memory unsafety it prevents

### What Counts as a Bounds Check:
- `if idx < vec.len()`
- `vec.get(idx)` instead of `vec[idx]`
- `.checked_add()`, `.saturating_add()`, `.saturating_subtract()` (unless explicitly requested)
- Any conditional that guards array/slice access

### When Bounds Checks ARE Required:
```rust
// Example from text rendering:
if (final_x as usize) < width && (final_y as usize) < height {
    let idx = final_y as usize * width as usize + final_x as usize;
    pixels[idx] = blended as u32;  // Would segfault without the check
}
```
**WHY**: `final_x` and `final_y` come from glyph positioning math and can be negative or exceed buffer dimensions. The cast to `usize` makes negatives wrap to huge values (fail bigger than shit), and the check prevents out-of-bounds memory access.

**PROOF**: Without this check, we'd write outside the pixel buffer and corrupt memory/segfault.

### When Bounds Checks Are FORBIDDEN:
```rust
// Internal loop with known bounds - NO CHECKS
for y in 0..self.height as usize {
    for x in 0..self.width as usize {
        let idx = y * self.width as usize + x;
        pixels[idx] = color;  // idx is MATHEMATICALLY proven in-bounds
    }
}
```
```rust
// Packed SIMD arithmetic - NO SATURATING OPS
let mut blended = bg * inv_alpha + colour * alpha;  // Overflow is part of the algorithm
blended = (blended >> 8) & 0x00FF00FF00FF00FF;     // Bit ops handle the overflow correctly
```

### When to Panic Instead:
```rust
// Debug visualization - if buffer is wrong size, FAIL LOUD
if self.show_textbox_mask {
    for y in 0..self.height as usize {
        for x in 0..self.width as usize {
            let idx = y * self.width as usize + x;
            let alpha = self.textbox_mask[idx];  // PANIC if uninitialized = correct behavior
            pixels[idx] = pack_argb(alpha, alpha, alpha, 255);
        }
    }
}
```

## Core Principles

### 0. **Trust the Math**
- If loop bounds guarantee safety, don't add runtime checks
- Index calculations from known dimensions are proof of correctness
- Compiler optimization depends on removing redundant bounds checks

### 1. **Fail Fast, Fail Loud**
- Panics are better than silent corruption
- A panic means "there's a bug in initialization" not "add a bounds check"
- Debug visualizations should expose bugs, not hide them

### 2. **Understand Packed SIMD**
- Overflow in packed arithmetic is often **required** for correctness
- Bit masks and shifts handle channel isolation and reconstruction
- Saturating ops break the mathematical properties being exploited

### 3. **Respect Explicit Unsafe Contracts**
```rust
// SAFETY: idx is proven in-bounds by loop invariant (0..width * 0..height)
unsafe { pixels.get_unchecked_mut(idx) }
```
If there's a SAFETY comment, **read it**. It's there because the human proved correctness.
HashMap shall NOT be used without explicit consent and proof showing how it is faster/better than a linear search or a simple lookup.

## Language Preferences

### Strongly Preferred:
- **Rust**: Memory safety through ownership, zero-cost abstractions
- **Assembly**: When you need exact control
- **Metal**: GPU compute with known performance characteristics

### Not Allowed:
- **Python**: Slow, loose typing causes bugs, cannot copy-paste, 1-indexed nonsense infected everything
- High-level scripting when systems programming is needed, text parsing, terribly unsafe

## VSF Serialization: Use High-Level APIs

**ALWAYS prefer VSF's schema-validated builders over manual byte manipulation.**

### The Right Way: SectionBuilder + SectionSchema

```rust
use vsf::schema::{SectionSchema, SectionBuilder, TypeConstraint};

// Define schema (or use official: vsf::schema::official::network_peer_schema())
let schema = SectionSchema::new("announce")
    .field("challenge_hash", TypeConstraint::Blake3Rolling)
    .field("handle_hash", TypeConstraint::Blake3Provenance)
    .field("port", TypeConstraint::AnyUnsigned);

// Build with validation
let bytes = schema.build()
    .set("challenge_hash", VsfType::hb(hash))?
    .set("handle_hash", VsfType::hb(handle_hash))?
    .set("port", 41641u16)?
    .encode()?;

// Parse → modify → re-encode
let mut builder = SectionBuilder::parse(schema, &bytes)?;
builder = builder.set("port", 8080u16)?;
let updated = builder.encode()?;
```

### FORBIDDEN: Manual Serialization

```rust
// NO - manual byte pushing, error-prone, no validation
let mut bytes = Vec::new();
bytes.push(b'[');
bytes.extend(VsfType::d("announce".to_string()).flatten());
bytes.push(b'(');
bytes.extend(VsfType::d("port".to_string()).flatten());
bytes.push(b':');
bytes.extend(VsfType::u4(41641).flatten()); // This becomes fragile. If port is less than 255 a u3 is valid.
bytes.push(b')');
bytes.push(b']');
```

### Why High-Level APIs:

0. **Type safety** - TypeConstraint validates values match expected types
1. **Schema validation** - Unknown fields are caught, required fields enforced
2. **Round-trip safe** - parse → modify → encode workflow guaranteed correct
3. **Self-documenting** - Schema IS the documentation
4. **Future-proof** - Wire format changes handled by library, not your code

### Available Official Schemas:

- `vsf::schema::official::image_schema()` - Image metadata
- `vsf::schema::official::camera_schema()` - Camera hardware config
- `vsf::schema::official::audio_schema()` - Audio stream metadata
- `vsf::schema::official::network_peer_schema()` - P2P peer info
- `vsf::schema::official::announce_schema()` - FGTW bootstrap

### For Complete Files: VsfBuilder

```rust
use vsf::vsf_builder::VsfBuilder;

let bytes = VsfBuilder::new()
    .add_section(my_section)
    .provenance(provenance_hash)  // Immutable identity
    .build()?;  // BLAKE3 hash computed automatically
```

**The library handles integrity hashing, header layout, section offsets - you just add content.**

## VSF Transport Rule: COMPLETE FILES ONLY

**ALL network transport and disk storage MUST use complete VSF files.**

A complete VSF file has (bare minimum):
- `RÅ<` magic header
- Version (`z`) and backward compat (`y`)
- Header length (`b`)
- Creation timestamp (`ef5` or `ef6`)
- Provenance hash (`hp`) - REQUIRED for integrity
- Field count (`n`)
- Header end marker (`>`)
- Section structure with `[ ]` delimiters

### FORBIDDEN: Raw VSF Types

```rust
// NO - raw VSF type without file wrapper (no integrity, no timestamp, no version)
let bytes = VsfType::v(b'e', encrypted_data).flatten();
Response::from_bytes(bytes)

// NO - bare section without file header
let bytes = section.encode();
socket.send(&bytes);
```

### REQUIRED: Complete VSF Files

```rust
// YES - complete VSF file with proper header and provenance hash
let bytes = VsfBuilder::new()
    .creation_time_nanos(timestamp)
    .provenance_only()
    .add_section("encrypted_data", vec![
        ("payload".to_string(), VsfType::v(b'e', encrypted_data)),
    ])
    .build()?;
```

**Why**: Raw types have no integrity protection, no timestamps, no version info.
The provenance hash ensures data hasn't been tampered with in transit.
Every VSF on the wire or disk can be inspected with `vsfinfo` and verified.

## Code Style

### Terminal Commands:
```bash
# DON'T add comments in commands
cargo build --release

# DO separate commands that should be run individually
cargo clean
cargo build --release
```

### Prefer Explicit Over "Safe":
```rust
// YES - explicit, clear, no overhead
pixels[idx] = color;

// NO - hiding potential bugs, runtime overhead
if let Some(pixel) = pixels.get_mut(idx) {
    *pixel = color;
}
```

## The Clamp Trap

**`clamp()` is defensive programming that hides bugs.**

```rust
// WASTEFUL - clamp does nothing useful here
let byte_value = pixel_value.clamp(0.0, 255.0) as u8;

// CORRECT - cast already handles bounds
let byte_value = pixel_value as u8;
```

**Why clamp is wrong:**
0. **Casting already handles bounds** - `f32 as u8` truncates automatically, the clamp checks bounds the cast does for free
1. **Hides bugs** - if values are outside range, you WANT to know (forensic), not silently fix it (defensive)
2. **Assumes your math is broken** - if calculations are correct, values should never be out of range anyway

**The forensic approach:** If the cast wraps/truncates unexpectedly, that exposes the real bug in your math. Fix the math, don't hide the symptoms.

## When Unsure

**ASK.** Don't add "defensive" checks. If you're not sure whether a bounds check is needed:
0. State what you're unsure about
1. Show the code without the check
2. Explain what would happen if the invariant is violated
3. Let the human decide

## Error Handling Philosophy

0. **Initialization bugs should panic** - if textbox_mask is empty, that's a bug in `new()`, not in the render loop
1. **External input should be validated** - user input, file data, network packets get bounds checks
2. **Internal invariants should be maintained** - if your loop guarantees safety, assert it in debug builds if needed, but don't check in release

## Dimensional Units

**NEVER use fixed pixel values.** 20px on an 8K TV ≠ 20px on a watch. Use relative scaling:

```rust
// WRONG - fixed pixels break across displays
let margin_pixels = 20;

// CORRECT - scale to display dimensions
let margin = box_width / 40;  // 2.5% of textbox width
let margin = self.min_dim / 64;  // Fraction of minimum dimension
```

**Physics scales, pixels don't.** E=mc² works everywhere because it's based on fundamental relationships, not arbitrary units. Your code should too.

### Universal Scaling Units (Already Available):
- `self.min_dim` - min(width, height), universal scaling base
- `self.perimeter` - width + height, for edge-aware calculations
- `self.diagonal_sq` - width² + height², for distance calculations

Use these. Derive everything from screen dimensions and their mathematical relationships.

## When Unsure

**ASK.** Don't add "defensive" checks. If you're not sure whether a bounds check is needed:
0. State what you're unsure about
1. Show the code without the check
2. Explain what would happen if the invariant is violated
3. Let the human decide

## Active Projects

- **Spirix**: Two's complement floating-point (replaces IEEE-754)
- **TOKEN**: Cryptographic identity/reputation system
- **Ferros**: Kill-switch ready Rust OS

---

*Remember: The user is rebuilding the computational stack from first principles. They see patterns others miss. They're right that base-10 is broken and IEEE-754 teaches incorrect math. When they write packed SIMD, they know exactly what they're doing. Your job is to help build the cathedral, not childproof it.*

Oh, and if you haven't noticed by now but when indexing in //'s and variable names, starte with the first number (0), not the second number (1)
Development builds? use ./build-development.sh DO NOT DO RELEASE BUILDS UNLESS THE USER EXPLICITLY ASKS!