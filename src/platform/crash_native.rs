//! Native-fault catcher — the panic hook's sibling for deaths Rust never sees.
//!
//! The panic hook (main.rs / lib.rs android init) only fires for Rust panics. A native fault — access violation in GDI/driver/FFI code, stack overflow, abort — bypasses it on EVERY platform: Linux prints "segmentation fault", Windows silently hands the process to WER, and either way the submitted log just... ends (Nelson's Windows minimize crash, 2026-08-31). This module writes the same `photon.crash.txt` sidecar the panic hook uses, so `report_prior_crash` folds a `NATIVE FAULT` line into the next run's log and it rides the normal submission — no Event Viewer, no dmesg, no user archaeology.
//!
//! Best-effort by nature: the process is already dying in undefined territory. The Windows filter uses format!/heap (fine for the overwhelmingly common access-violation case; a heap-corruption fault may lose the report). The Unix handler is strictly async-signal-safe — pre-opened fd, stack-buffer hex formatting, raw write() — and runs on a sigaltstack so even stack overflow reports. Both re-raise the default action afterwards, so core dumps / WER still happen.

/// Install the platform's native-fault hook. Call once at startup, right after the panic hook.
pub fn install() {
    imp::install();
}

#[cfg(target_os = "windows")]
mod imp {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::Diagnostics::Debug::{
        SetUnhandledExceptionFilter, EXCEPTION_POINTERS,
    };
    use windows::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };

    /// Resolve which loaded module owns `addr` and the offset into it — module + offset is exactly what Event Viewer's Event 1000 reports, so a NATIVE FAULT log line is directly comparable (and symbolicatable against our own build when the module is photon.exe).
    unsafe fn module_of(addr: usize) -> (String, usize) {
        let mut hmod = HMODULE::default();
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(addr as *const u16),
            &mut hmod,
        )
        .is_ok()
        {
            let mut buf = [0u16; 260];
            let n = GetModuleFileNameW(hmod, &mut buf) as usize;
            let full = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
            // Basename only — the full path carries the Windows username, and sidecar text lands in the submitted log (name-scrub happens at the sink, not here; don't rely on it).
            let name = full.rsplit(['\\', '/']).next().unwrap_or("?").to_string();
            (name, addr.wrapping_sub(hmod.0 as usize))
        } else {
            ("?".to_string(), addr)
        }
    }

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        let (mut code, mut addr) = (0u32, 0usize);
        if !info.is_null() {
            let rec = (*info).ExceptionRecord;
            if !rec.is_null() {
                code = (*rec).ExceptionCode.0 as u32;
                addr = (*rec).ExceptionAddress as usize;
            }
        }
        let (module, offset) = module_of(addr);
        // Sidecar ONLY — no log-sink locks: the faulting thread may hold them, and a wedged filter turns a clean crash into a hang. The RAM batch dies with the process; hard-logs mode is the context channel.
        crate::write_crash_sidecar(&format!(
            "NATIVE FAULT: exception 0x{code:08X} at {module}+0x{offset:X}"
        ));
        // CONTINUE_SEARCH: WER still runs, the process still dies — we only observed.
        0
    }

    pub fn install() {
        unsafe {
            SetUnhandledExceptionFilter(Some(filter));
        }
    }
}

// Real unixes only: Redox is cfg(unix) but its libc has no sigaltstack/SIGSTKSZ/stack_t/si_addr (broke the v0.71.0 deploy's redox leg) — it takes the no-op below until its signal surface grows.
#[cfg(all(unix, not(target_os = "redox")))]
mod imp {
    use std::sync::atomic::{AtomicI32, Ordering};

    /// Sidecar fd, opened at install time — open(2) with a heap CString inside a signal handler is not async-signal-safe, so the handler only ever write(2)s.
    static SIDECAR_FD: AtomicI32 = AtomicI32::new(-1);

    /// Append `v` as lowercase hex into `buf` at `pos` — the async-signal-safe fragment of format!.
    fn put_hex(buf: &mut [u8], pos: &mut usize, mut v: usize) {
        let start = *pos;
        if v == 0 {
            buf[*pos] = b'0';
            *pos += 1;
            return;
        }
        let mut digits = [0u8; 16];
        let mut n = 0;
        while v > 0 {
            digits[n] = b"0123456789abcdef"[v & 0xF];
            v >>= 4;
            n += 1;
        }
        while n > 0 && *pos < buf.len() {
            n -= 1;
            buf[*pos] = digits[n];
            *pos += 1;
        }
        let _ = start;
    }

    fn put_str(buf: &mut [u8], pos: &mut usize, s: &str) {
        let bytes = s.as_bytes();
        let take = bytes.len().min(buf.len() - *pos);
        buf[*pos..*pos + take].copy_from_slice(&bytes[..take]);
        *pos += take;
    }

    unsafe extern "C" fn handler(sig: i32, info: *mut libc::siginfo_t, _ctx: *mut libc::c_void) {
        let fd = SIDECAR_FD.load(Ordering::Relaxed);
        if fd >= 0 {
            let name = match sig {
                libc::SIGSEGV => "SIGSEGV",
                libc::SIGBUS => "SIGBUS",
                libc::SIGILL => "SIGILL",
                libc::SIGFPE => "SIGFPE",
                libc::SIGABRT => "SIGABRT",
                _ => "SIG?",
            };
            let addr = if info.is_null() { 0 } else { (*info).si_addr() as usize };
            let mut buf = [0u8; 128];
            let mut pos = 0;
            put_str(&mut buf, &mut pos, "NATIVE FAULT: ");
            put_str(&mut buf, &mut pos, name);
            put_str(&mut buf, &mut pos, " at 0x");
            put_hex(&mut buf, &mut pos, addr);
            put_str(&mut buf, &mut pos, "\n");
            let _ = libc::write(fd, buf.as_ptr() as *const _, pos);
        }
        // SA_RESETHAND restored the default action; re-raise so the OS finishes the kill (core dump, tombstone) exactly as if we were never here.
        let _ = libc::raise(sig);
    }

    pub fn install() {
        let Some(dir) = crate::log_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("photon.crash.txt");
        let Ok(cpath) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
            return;
        };
        let fd = unsafe {
            libc::open(
                cpath.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return;
        }
        SIDECAR_FD.store(fd, Ordering::Relaxed);
        unsafe {
            // Alternate stack so a stack-overflow SIGSEGV still gets a handler frame — without this the one fault users actually hit on deep recursion reports nothing.
            let stack_size = libc::SIGSTKSZ.max(64 * 1024);
            let stack = libc::mmap(
                std::ptr::null_mut(),
                stack_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            );
            if stack != libc::MAP_FAILED {
                let ss = libc::stack_t { ss_sp: stack, ss_flags: 0, ss_size: stack_size };
                let _ = libc::sigaltstack(&ss, std::ptr::null_mut());
            }
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = handler as usize;
            sa.sa_flags = libc::SA_SIGINFO | libc::SA_RESETHAND | libc::SA_ONSTACK;
            libc::sigemptyset(&mut sa.sa_mask);
            for sig in [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGABRT] {
                let _ = libc::sigaction(sig, &sa, std::ptr::null_mut());
            }
        }
    }
}

#[cfg(not(any(target_os = "windows", all(unix, not(target_os = "redox")))))]
mod imp {
    pub fn install() {}
}
