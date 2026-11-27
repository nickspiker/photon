//! A granted region reached through its handle (ferros SANDBOX.md "Handles"; ferros_sys `Handle`, restated): Photon is masked code, so a load of a region outside its arc lands in the arc, and a shared run or a mailbox view is read and written only as `base + (offset & mask)` with the pair the kernel wrote in the handle table at the arc's start.
//! The kernel puts a view's handle at the first free pair, so its slot is known only at run time; the table's load takes the slot as an immediate (the only form the checker allows), so each access dispatches over the table's 32 slots to one instance per slot.

/// The handle table's pairs: 512 bytes of 16-byte pairs.
pub const SLOTS: u64 = 1 << 5;

#[inline(always)]
fn pair_c<const S: u64>() -> (u64, u64) {
    let (b, m): (u64, u64);
    // SAFETY: the table load into the pair registers, the form the checker allows (ferros_sys Handle::pair); the table is read-only to the process and always mapped.
    unsafe { core::arch::asm!("ldp x23, x22, [x28, #{t}]", "mov {b}, x23", "mov {m}, x22", b = out(reg) b, m = out(reg) m, t = const 16 * S, options(nostack, readonly, preserves_flags)) };
    (b, m)
}

#[inline(always)]
fn read64_c<const S: u64>(off: u64) -> u64 {
    let v: u64;
    // SAFETY: the offset masked by the region's own mask before the load (ferros_sys Handle::read64): every address it forms is inside the granted region.
    unsafe { core::arch::asm!("ldp x23, x22, [x28, #{t}]", "and x21, {o}, x22", "ldr {v}, [x23, x21]", o = in(reg) off, v = out(reg) v, t = const 16 * S, options(nostack, readonly, preserves_flags)) };
    v
}

#[inline(always)]
fn write64_c<const S: u64>(off: u64, v: u64) {
    // SAFETY: as `read64_c`, a store (ferros_sys Handle::write64); the region is granted read-write or the kernel faults the store.
    unsafe { core::arch::asm!("ldp x23, x22, [x28, #{t}]", "and x21, {o}, x22", "str {v}, [x23, x21]", o = in(reg) off, v = in(reg) v, t = const 16 * S, options(nostack, preserves_flags)) };
}

macro_rules! by_slot {
    ($slot:expr, $f:ident $(, $a:expr)*) => {
        match $slot {
            0 => $f::<0>($($a),*), 1 => $f::<1>($($a),*), 2 => $f::<2>($($a),*), 3 => $f::<3>($($a),*),
            4 => $f::<4>($($a),*), 5 => $f::<5>($($a),*), 6 => $f::<6>($($a),*), 7 => $f::<7>($($a),*),
            8 => $f::<8>($($a),*), 9 => $f::<9>($($a),*), 10 => $f::<10>($($a),*), 11 => $f::<11>($($a),*),
            12 => $f::<12>($($a),*), 13 => $f::<13>($($a),*), 14 => $f::<14>($($a),*), 15 => $f::<15>($($a),*),
            16 => $f::<16>($($a),*), 17 => $f::<17>($($a),*), 18 => $f::<18>($($a),*), 19 => $f::<19>($($a),*),
            20 => $f::<20>($($a),*), 21 => $f::<21>($($a),*), 22 => $f::<22>($($a),*), 23 => $f::<23>($($a),*),
            24 => $f::<24>($($a),*), 25 => $f::<25>($($a),*), 26 => $f::<26>($($a),*), 27 => $f::<27>($($a),*),
            28 => $f::<28>($($a),*), 29 => $f::<29>($($a),*), 30 => $f::<30>($($a),*), 31 => $f::<31>($($a),*),
            // A slot past the table is a bug here: every slot read and written was found by `slot_of` or named by the kernel and checked under SLOTS.
            s => panic!("handle slot {} past the table", s),
        }
    };
}

/// Slot `slot`'s (base, mask): (0, _) for an empty pair.
pub fn pair(slot: u64) -> (u64, u64) {
    by_slot!(slot, pair_c)
}

/// The slot whose region starts at `base` (the address a view's open answered), searched from `from`: None when no pair holds it.
pub fn slot_of(base: u64, from: u64) -> Option<u64> {
    (from..SLOTS).find(|&s| pair(s).0 == base)
}

/// The word at byte `off` of slot `slot`'s region (8-byte aligned: single-copy atomic).
#[inline(always)]
pub fn read64(slot: u64, off: u64) -> u64 {
    by_slot!(slot, read64_c, off)
}

/// The word at byte `off` of slot `slot`'s region.
#[inline(always)]
pub fn write64(slot: u64, off: u64, v: u64) {
    by_slot!(slot, write64_c, off, v)
}
