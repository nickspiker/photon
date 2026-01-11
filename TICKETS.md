No uniform scaling code/variable, ctrl + and _ and pinch zoom on touch need added
No notifications for messages
somewhat out of order for message display
send button doesn't work (use enter key)
Text on send overwrites the send button
Extra hash on executable?
self updates need added
network broadcast gets stuck/lost
needs to keep text entry box selected whilist sending messageths
chrome downloads rename to .zip and trigger extract, not install instead of apk on Android
rotated images that have exif flags set to rotate after decode needs implemented

● Update(src/ui/compositing.rs) Gotta actually test this
  ⎿  Updated src/ui/compositing.rs with 8 additions                                                          
       2061    
       2062                // Always present buffer once per frame
       2063                buffer.present().unwrap();
       2064 +          } else {
       2065 +              // macOS with transparent windows + softbuffer doesn't retain buffer contents
       2066 +              // between frames. Must re-present even when nothing changed or window goes black.
       2067 +              #[cfg(target_os = "macos")]
       2068 +              {
       2069 +                  let mut buffer = self.renderer.lock_buffer();
       2070 +                  buffer.present().unwrap();
       2071 +              }
       2072            }
       2073            self.window_dirty = false;
       2074            self.text_dirty = false;


Need to colourize handles
fn colourize(hash: [u8; 32], num_handles: usize) -> [f32; 3] {
    // Convert hash to index in [0, num_handles)
    let index = hash_to_index(hash, num_handles);
    
    // Generate Fibonacci lattice point for this index
    let (x, y, z) = fibonacci_sphere_point(index, num_handles);
    
    // Sphere is centered at origin, radius 1
    // Project ray from cube center (0.5, 0.5, 0.5) through sphere point
    // Find intersection with RGB cube [0,1]³
    
    let ray_dir = (x, y, z); // normalized direction
    let ray_origin = (0.5, 0.5, 0.5);
    
    // Find t where ray intersects cube face
    let t = intersect_cube(ray_origin, ray_dir);
    
    let r = 0.5 + t * x;
    let g = 0.5 + t * y;
    let b = 0.5 + t * z;
    
    [r, g, b]
}

fn hash_to_index(hash: [u8; 32], n: usize) -> usize {
    // Use first 8 bytes as u64, modulo n
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash[0..8]);
    u64::from_le_bytes(bytes) as usize % n
}

fn fibonacci_sphere_point(i: usize, n: usize) -> (f32, f32, f32) {
    const PHI: f32 = 1.618033988749895; // golden ratio
    
    let i_f = i as f32;
    let n_f = n as f32;
    
    let theta = 2.0 * PI * i_f / PHI;
    let phi = (1.0 - 2.0 * (i_f + 0.5) / n_f).acos();
    
    let x = phi.sin() * theta.cos();
    let y = phi.sin() * theta.sin();
    let z = phi.cos();
    
    (x, y, z)
}

fn intersect_cube(origin: (f32, f32, f32), dir: (f32, f32, f32)) -> f32 {
    // Ray: P = origin + t * dir
    // Find smallest positive t where P intersects cube faces [0,1]³
    
    let mut t_min = f32::INFINITY;
    
    // Check each axis for intersection with min/max faces
    for axis in 0..3 {
        let o = [origin.0, origin.1, origin.2][axis];
        let d = [dir.0, dir.1, dir.2][axis];
        
        if d.abs() > 1e-6 {
            // Intersect with face at 0
            let t0 = (0.0 - o) / d;
            if t0 > 0.0 && in_cube_bounds(origin, dir, t0, axis) {
                t_min = t_min.min(t0);
            }
            
            // Intersect with face at 1
            let t1 = (1.0 - o) / d;
            if t1 > 0.0 && in_cube_bounds(origin, dir, t1, axis) {
                t_min = t_min.min(t1);
            }
        }
    }
    
    t_min
}

fn in_cube_bounds(origin: (f32, f32, f32), dir: (f32, f32, f32), t: f32, skip_axis: usize) -> bool {
    let p = (
        origin.0 + t * dir.0,
        origin.1 + t * dir.1,
        origin.2 + t * dir.2,
    );
    
    let coords = [p.0, p.1, p.2];
    
    for axis in 0..3 {
        if axis != skip_axis {
            if coords[axis] < 0.0 || coords[axis] > 1.0 {
                return false;
            }
        }
    }
    true
}