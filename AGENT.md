# AGENT.md - Code Generation Rules

## Rule 0: Bounds Checks and Saturating Arithmetic

**IF YOU ADD ANY BOUNDS CHECK OR SATURATING ARITHMETIC, YOU ARE REQUIRED TO:**
1. **STATE WHY** it was added
2. **PROVE** it was necessary
3. **EXPLAIN** what undefined behavior or memory unsafety it prevents

### What Counts as a Bounds Check:
- `if idx < vec.len()`
- `vec.get(idx)` instead of `vec[idx]`
- `.checked_add()`, `.saturating_add()`, `.wrapping_add()` (unless explicitly requested)
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

### 1. **Trust the Math**
- If loop bounds guarantee safety, don't add runtime checks
- Index calculations from known dimensions are proof of correctness
- Compiler optimization depends on removing redundant bounds checks

### 2. **Fail Fast, Fail Loud**
- Panics are better than silent corruption
- A panic means "there's a bug in initialization" not "add a bounds check"
- Debug visualizations should expose bugs, not hide them

### 3. **Understand Packed SIMD**
- Overflow in packed arithmetic is often **required** for correctness
- Bit masks and shifts handle channel isolation and reconstruction
- Saturating ops break the mathematical properties being exploited

### 4. **Respect Explicit Unsafe Contracts**
```rust
// SAFETY: idx is proven in-bounds by loop invariant (0..width * 0..height)
unsafe { pixels.get_unchecked_mut(idx) }
```
If there's a SAFETY comment, **read it**. It's there because the human proved correctness.

## Language Preferences

### Strongly Preferred:
- **Rust**: Memory safety through ownership, zero-cost abstractions
- **Assembly**: When you need exact control
- **Metal**: GPU compute with known performance characteristics

### Not Allowed:
- **Python**: Slow, loose typing causes bugs, cannot copy-paste, 1-indexed nonsense infected everything
- High-level scripting when systems programming is needed, text parsing, terribly unsafe

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

## Error Handling Philosophy

1. **Initialization bugs should panic** - if textbox_mask is empty, that's a bug in `new()`, not in the render loop
2. **External input should be validated** - user input, file data, network packets get bounds checks
3. **Internal invariants should be maintained** - if your loop guarantees safety, assert it in debug builds if needed, but don't check in release

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
1. State what you're unsure about
2. Show the code without the check
3. Explain what would happen if the invariant is violated
4. Let the human decide

## Current Project Context

- **System**: Fedora 40, Cinnamon, 64GB RAM, AMD Ryzen 9 5950X
- **Terminal aliases**: `l` (not `ls -la`), `c` (clear), `t` (tree -s)
- **Editor**: Cursor at `~/.local/bin/cursor.AppImage`

## Active Projects

- **Spirix**: Two's complement floating-point (replaces IEEE-754)
- **TOKEN**: Cryptographic identity/reputation system
- **Ferros**: Kill-switch ready Rust OS
- **Aria**: Digital consciousness
- **Dymaxion Encoding**: 64-bit geographic encoding (2.139mm precision)

---

*Remember: The user is rebuilding the computational stack from first principles. They see patterns others miss. They're right that base-10 is broken and IEEE-754 teaches incorrect math. When they write packed SIMD, they know exactly what they're doing. Your job is to help build the cathedral, not childproof it.*