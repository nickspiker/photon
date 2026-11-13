pub mod blind;
pub mod chain;
pub mod clutch;
pub mod era;
pub mod handle_proof;
pub mod keys;
pub mod log_seal;
pub mod photon_keys;
pub mod self_verify;
pub mod shards;

/// Keep the memory holding a long-lived secret out of swap (CSTV-08): `mlock` on unix, `VirtualLock` on Windows, so a key that should live only in RAM is never written to a swap file or pagefile.
/// Pins the pages under `value` at the address it has now, so only a secret that never moves is worth pinning: a static, or the inside of an `Arc` that lives as long as the process. Never unpinned; the process holds these keys until it exits.
/// Best effort: the OS caps locked memory (16 MB for a user on a stock Linux), a refusal is logged once and the key stays usable, and nothing here stops hibernation or a live-memory read.
pub fn pin_in_ram<T>(what: &str, value: &T) {
    let addr = value as *const T as *const core::ffi::c_void;
    let len = core::mem::size_of::<T>();
    #[cfg(all(unix, not(target_os = "ferros")))]
    // SAFETY: the range is one live value of ours; mlock only marks its pages resident and reads nothing.
    let pinned = unsafe { libc::mlock(addr, len) } == 0;
    #[cfg(windows)]
    let pinned = {
        #[link(name = "kernel32")]
        extern "system" {
            fn VirtualLock(address: *const core::ffi::c_void, size: usize) -> i32;
        }
        // SAFETY: as above; VirtualLock marks the pages resident in our working set.
        let locked = unsafe { VirtualLock(addr, len) };
        locked != 0
    };
    #[cfg(not(any(all(unix, not(target_os = "ferros")), windows)))]
    let pinned = {
        let _ = (addr, len);
        true
    };
    if !pinned {
        crate::logf!("CRYPTO: {} could not be pinned in RAM (the OS refused the lock); it stays usable, and may reach swap under memory pressure", what);
    }
}

/// No core image of this process's memory, keys included, when it crashes (CSTV-08): no core file on any unix, and on Linux not dumpable at all, which also keeps another process of the same user from attaching to read it.
/// Called once at start; children inherit the zero core limit, which costs them nothing.
pub fn no_core_dumps() {
    #[cfg(all(unix, not(target_os = "ferros")))]
    {
        let zero = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: a plain resource-limit call with a valid struct.
        if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &zero) } != 0 {
            crate::log("CRYPTO: the core-file limit could not be set to zero; a crash may write a core image");
        }
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: PR_SET_DUMPABLE takes one integer argument; the rest are unused.
        if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
            crate::log("CRYPTO: the process could not be marked not dumpable");
        }
    }
}
