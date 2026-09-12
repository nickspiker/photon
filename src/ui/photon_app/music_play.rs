//! Desktop playback for MUSIC PIGEONS: a rodio sink over the blob's bytes. Android is a stub for now — its audio belongs to the call engine's AAudio path and music will route thru it later.

#[cfg(all(not(target_os = "android"), not(target_os = "redox")))]
mod real {
    pub struct MusicPlay {
        pub hash: [u8; 32],
        pub duration_secs: f32,
        _stream: rodio::OutputStream,
        sink: rodio::Sink,
    }

    impl MusicPlay {
        pub fn start(hash: [u8; 32], bytes: Vec<u8>, duration_secs: f32) -> Option<Self> {
            let (stream, handle) = rodio::OutputStream::try_default().ok()?;
            let dec = rodio::Decoder::new(std::io::Cursor::new(bytes)).ok()?;
            let sink = rodio::Sink::try_new(&handle).ok()?;
            sink.append(dec);
            sink.play();
            Some(Self { hash, duration_secs, _stream: stream, sink })
        }
        pub fn toggle(&self) {
            if self.sink.is_paused() {
                self.sink.play();
            } else {
                self.sink.pause();
            }
        }
        pub fn playing(&self) -> bool {
            !self.sink.is_paused() && !self.sink.empty()
        }
        pub fn done(&self) -> bool {
            self.sink.empty()
        }
        pub fn frac(&self) -> f32 {
            (self.sink.get_pos().as_secs_f32() / self.duration_secs.max(0.001)).clamp(0.0, 1.0)
        }
        pub fn seek_frac(&self, f: f32) {
            let _ = self.sink.try_seek(std::time::Duration::from_secs_f32((f * self.duration_secs).max(0.0)));
            self.sink.play();
        }
    }
}

#[cfg(any(target_os = "android", target_os = "redox"))]
mod real {
    pub struct MusicPlay {
        pub hash: [u8; 32],
        pub duration_secs: f32,
    }

    impl MusicPlay {
        pub fn start(_hash: [u8; 32], _bytes: Vec<u8>, _duration_secs: f32) -> Option<Self> {
            None
        }
        pub fn toggle(&self) {}
        pub fn playing(&self) -> bool {
            false
        }
        pub fn done(&self) -> bool {
            true
        }
        pub fn frac(&self) -> f32 {
            0.0
        }
        pub fn seek_frac(&self, _f: f32) {}
    }
}

pub(super) use real::MusicPlay;
