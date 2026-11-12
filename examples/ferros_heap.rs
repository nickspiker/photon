// What Photon's heap holds on a ferros device at the Pro's full screen, measured on the desktop: PhotonApp driven as fluor's ferros host drives it (host/ferros: its scratch at Photon's depth, G2t16, its clip mask, its text renderer; init, on_resize, then tick, the app's own events and a whole-frame render each pass), with a counting allocator under it. For ferros's std/photon/build.rs, which sets the heap ceiling from it (ferros docs/ARCS.md step 2: a ceiling the arc grows to, never a reservation).
// Run it with nothing of the person's in reach: a fresh home and runtime dir (no identity, the state a ferros device starts in) and no network:
//   cargo build --example ferros_heap && unshare -r -n env HOME=<empty dir> XDG_CONFIG_HOME=<empty dir>/.config XDG_DATA_HOME=<empty dir>/.local/share XDG_CACHE_HOME=<empty dir>/.cache XDG_STATE_HOME=<empty dir>/.local/state XDG_RUNTIME_DIR=<empty dir>/run target/debug/examples/ferros_heap   (--where: who made each frame-sized allocation; build with --profile debuggable for names)
// 2026-11-12: a peak of 142.6 MiB at the first screen, steady after the first frame: the host's scratch (G2t16, 30.7 MiB) and clip mask (3.8), the chrome's two full-screen G2t16 layers (61.4) and its u16 hit map (7.7), the background noise field (15.4), the status queue (sync_channel of 2^14, 7.3), the text renderer's fonts (10.3); 79 threads, whose stacks come from the heap on ferros (256 KiB each).
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fluor::canvas::{Damage, PixelRect};
use fluor::host::app::{Context, FluorApp};
use fluor::host::{WakeError, WakeSender, WindowHandle};
use fluor::pixel::Pixel;
use photon_messenger::ui::photon_app::PhotonApp;
use photon_messenger::ui::Px;

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
/// The largest single allocation seen, and how many of at least a full frame (1344 x 2992 pixels at 4 bytes, the smallest a frame buffer is) were made.
static BIGGEST: AtomicUsize = AtomicUsize::new(0);
static FRAME_SIZED: AtomicUsize = AtomicUsize::new(0);
/// The frame-sized allocations' sizes in the order made (the first 32), for the line: what each one is shows in its bytes a pixel.
static SIZES: [AtomicUsize; 32] = [const { AtomicUsize::new(0) }; 32];
const FRAME_PX: usize = 1344 * 2992;

fn note(n: usize) {
    let now = NOW.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

/// `--where`: each frame-sized allocation's callers printed (eight frames of its backtrace past the allocator's own; a debuggable build has their names), never from inside itself.
static WHERE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
thread_local! { static IN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

fn note_size(size: usize) {
    BIGGEST.fetch_max(size, Ordering::Relaxed);
    if size >= FRAME_PX {
        if WHERE.load(Ordering::Relaxed) && !IN.with(|i| i.replace(true)) {
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            let at: Vec<&str> = bt.lines().map(str::trim).filter(|l| !l.starts_with("at ")).skip(6).take(8).collect();
            eprintln!("{:.1} bytes a pixel: {}", size as f64 / FRAME_PX as f64, at.join(" <- "));
            IN.with(|i| i.set(false));
        }
        let k = FRAME_SIZED.fetch_add(1, Ordering::Relaxed);
        if k < 32 {
            SIZES[k].store(size, Ordering::Relaxed);
        }
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            note(l.size());
            note_size(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            note(l.size());
            note_size(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        NOW.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, size) };
        if !q.is_null() {
            if size > l.size() {
                note(size - l.size());
                note_size(size);
            } else {
                NOW.fetch_sub(l.size() - size, Ordering::Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

const MIB: f64 = (1u64 << 20) as f64;

/// The process's threads (Linux): on ferros each one's stack comes from the heap, 256 KiB unless it asks for more (ferros-rust std).
fn threads() -> usize {
    std::fs::read_to_string("/proc/self/status").ok().and_then(|s| s.lines().find_map(|l| l.strip_prefix("Threads:").map(|n| n.trim().parse().unwrap_or(0)))).unwrap_or(0)
}

fn line(what: &str) {
    println!("{what}: heap {:.1} MiB now, {:.1} MiB peak; largest allocation {:.1} MiB, {} at least a frame's 4 bytes a pixel; {} threads", NOW.load(Ordering::Relaxed) as f64 / MIB, PEAK.load(Ordering::Relaxed) as f64 / MIB, BIGGEST.load(Ordering::Relaxed) as f64 / MIB, FRAME_SIZED.load(Ordering::Relaxed), threads());
    let n = FRAME_SIZED.load(Ordering::Relaxed).min(32);
    let sizes: Vec<String> = SIZES[..n].iter().map(|s| format!("{:.1}", s.load(Ordering::Relaxed) as f64 / FRAME_PX as f64)).collect();
    println!("    frame-sized allocations, bytes a pixel each: {}", sizes.join(" "));
}

struct Window;
impl WindowHandle for Window {
    fn request_redraw(&self) {}
}

/// The app's background sends, taken in each pass as the ferros host's FerrosWakeSender queue is.
struct Queue<E>(Mutex<Vec<E>>);
impl<E: Send> WakeSender<E> for Queue<E> {
    fn send(&self, event: E) -> Result<(), WakeError> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

type Event = <PhotonApp as FluorApp<Px>>::UserEvent;

fn main() {
    WHERE.store(std::env::args().any(|a| a == "--where"), Ordering::Relaxed);
    let (w, h) = (1344usize, 2992usize);
    let viewport = fluor::Viewport::new(w as u32, h as u32).with_ru(1.0);
    // fluor's ferros host (FerrosShell::new): the scratch at the app's depth, the clip mask, its own text renderer.
    let mut scratch = vec![<Px as Pixel>::EMPTY; w * h];
    let mut clip_mask = vec![255u8; w * h];
    let mut text = fluor::text::TextRenderer::new();
    let mut damage = Damage::new();
    let window = Window;
    line("the host's frame, clip mask and text");
    let mut app = PhotonApp::new();
    let queue: Arc<Queue<Event>> = Arc::new(Queue(Mutex::new(Vec::new())));
    FluorApp::<Px>::set_event_proxy(&mut app, queue.clone());
    line("PhotonApp::new");
    let whole = PixelRect::new(0, 0, w, h);
    macro_rules! ctx {
        () => {
            Context { pressed_hit: fluor::paint::HIT_NONE, viewport, text: &mut text, clip_mask: &mut clip_mask, damage: &mut damage, window: &window, modifiers: Default::default(), cursor_x: 0.0, cursor_y: 0.0, is_maximized: true, window_origin: (0, 0), damage_clip: whole }
        };
    }
    FluorApp::<Px>::init(&mut app, &mut ctx!());
    FluorApp::<Px>::on_resize(&mut app, w as u32, h as u32, &mut ctx!());
    line("init and on_resize");
    // The passes: its events, its tick, then the whole frame drawn (the host's first frame is whole; every frame here is, the most a frame can hold).
    for pass in 0..256u32 {
        let events: Vec<Event> = std::mem::take(&mut *queue.0.lock().unwrap());
        for e in events {
            let _ = FluorApp::<Px>::on_user_event(&mut app, e, &mut ctx!());
        }
        let _ = FluorApp::<Px>::tick(&mut app, &mut ctx!());
        scratch.fill(<Px as Pixel>::EMPTY);
        damage.clear();
        FluorApp::<Px>::render(&mut app, &mut scratch, &mut ctx!());
        if pass == 0 || pass == 1 || pass == 15 || pass == 255 {
            line(&format!("after frame {pass}"));
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
