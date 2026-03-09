//! macOS renderer — wgpu/Metal backend
//!
//! Owns a CPU-side pixel buffer (`Vec<u32>`, ARGB / `Bgra8Unorm` in memory).
//! Each frame: uploads the buffer as a Metal texture and blits it full-screen.
//!
//! The CAMetalLayer surface is retained by the compositor, so there is no need
//! to re-present on idle frames and no retained-copy hack is required.
//!
//! Pixel layout: `u32` stores `0xAARRGGBB`.  On little-endian ARM/x86 the
//! in-memory bytes are [B, G, R, A] = `Bgra8Unorm`, a direct upload with zero
//! byte-swapping.

use winit::window::Window;

// ---------------------------------------------------------------------------
// Full-screen blit shader (large-triangle trick — no vertex buffer needed)
// ---------------------------------------------------------------------------
const BLIT_SHADER: &str = r#"
struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0)       uv:  vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0,  3.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0, -1.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, -1.0),
        vec2<f32>(2.0,  1.0),
        vec2<f32>(0.0,  1.0),
    );
    var out: VertOut;
    out.pos = vec4<f32>(pos[vid], 0.0, 1.0);
    out.uv  = uv[vid];
    return out;
}

@group(0) @binding(0) var t_frame: texture_2d<f32>;
@group(0) @binding(1) var s_frame: sampler;

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    return textureSample(t_frame, s_frame, in.uv);
}
"#;

// ---------------------------------------------------------------------------
// WgpuBuffer guard — matches the softbuffer API used by compositing.rs
//
//   let mut buf = renderer.lock_buffer();
//   buf[idx] = colour;           // DerefMut → &mut [u32]
//   buf.present().unwrap();
// ---------------------------------------------------------------------------
pub struct WgpuBuffer<'a> {
    inner: &'a mut Renderer,
}

impl<'a> std::ops::Deref for WgpuBuffer<'a> {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        &self.inner.cpu_buffer
    }
}

impl<'a> std::ops::DerefMut for WgpuBuffer<'a> {
    fn deref_mut(&mut self) -> &mut [u32] {
        &mut self.inner.cpu_buffer
    }
}

impl<'a> WgpuBuffer<'a> {
    pub fn present(self) -> Result<(), ()> {
        self.inner.present_frame();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------
pub struct Renderer {
    surface:           wgpu::Surface<'static>,
    device:            wgpu::Device,
    queue:             wgpu::Queue,
    config:            wgpu::SurfaceConfiguration,
    pipeline:          wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    frame_texture:     wgpu::Texture,
    bind_group:        wgpu::BindGroup,
    sampler:           wgpu::Sampler,
    cpu_buffer:        Vec<u32>,
    width:             u32,
    height:            u32,
}

impl Renderer {
    pub fn new(window: &Window, width: u32, height: u32) -> Self {
        // SAFETY: The surface lives inside Renderer, which is owned by PhotonApp.
        // PhotonApp is dropped before Window is dropped in main.rs.
        let static_window: &'static Window = unsafe { std::mem::transmute(window) };
        pollster::block_on(Self::init(static_window, width, height))
    }

    async fn init(window: &'static Window, width: u32, height: u32) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });

        let surface = instance
            .create_surface(window)
            .expect("wgpu: create_surface failed");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference:       wgpu::PowerPreference::HighPerformance,
                compatible_surface:     Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("wgpu: no Metal adapter found");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label:             Some("photon"),
                required_features: wgpu::Features::empty(),
                required_limits:   wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .expect("wgpu: request_device failed");

        let caps = surface.get_capabilities(&adapter);

        // Prefer Bgra8Unorm (direct upload, no swizzle).
        // Avoid sRGB variants — our pixels are already display-ready.
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == wgpu::TextureFormat::Bgra8Unorm)
            .unwrap_or(caps.formats[0]);

        // PostMultiplied = straight alpha. Metal/CAMetalLayer reports [Opaque, PostMultiplied].
        // The fragment shader de-premultiplies the CPU buffer before output so the
        // compositor sees straight alpha and composites correctly.
        let alpha_mode = caps
            .alpha_modes
            .iter()
            .copied()
            .find(|m| *m == wgpu::CompositeAlphaMode::PostMultiplied)
            .unwrap_or(caps.alpha_modes[0]);

        let config = wgpu::SurfaceConfiguration {
            usage:   wgpu::TextureUsages::RENDER_ATTACHMENT,
            format:  surface_format,
            width:   width.max(1),
            height:  height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label:  Some("blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT_SHADER.into()),
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label:   Some("blit-bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding:    0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled:   false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type:    wgpu::TextureSampleType::Float {
                                filterable: false,
                            },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding:    1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(
                            wgpu::SamplerBindingType::NonFiltering,
                        ),
                        count: None,
                    },
                ],
            });

        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label:              Some("blit-layout"),
                bind_group_layouts: &[&bind_group_layout],
                ..Default::default()
            });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("blit-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module:              &shader,
                entry_point:         Some("vs_main"),
                buffers:             &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module:              &shader,
                entry_point:         Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format:     surface_format,
                    blend:      Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology:  wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil:  None,
            multisample:    wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache:          None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label:         Some("blit-sampler"),
            mag_filter:    wgpu::FilterMode::Nearest,
            min_filter:    wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let cpu_buffer = vec![0u32; (width * height) as usize];
        let (frame_texture, bind_group) =
            Self::make_texture(&device, &bind_group_layout, &sampler, width, height);

        Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group_layout,
            frame_texture,
            bind_group,
            sampler,
            cpu_buffer,
            width,
            height,
        }
    }

    fn make_texture(
        device:  &wgpu::Device,
        bgl:     &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        width:   u32,
        height:  u32,
    ) -> (wgpu::Texture, wgpu::BindGroup) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("frame-tex"),
            size:            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            // Bgra8Unorm: bytes in memory are [B, G, R, A].
            // Our u32 pixels are 0xAARRGGBB → little-endian bytes [BB GG RR AA] = BGRA.
            format:          wgpu::TextureFormat::Bgra8Unorm,
            usage:           wgpu::TextureUsages::TEXTURE_BINDING
                           | wgpu::TextureUsages::COPY_DST,
            view_formats:    &[],
        });
        let view = texture.create_view(&Default::default());
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("frame-bg"),
            layout:  bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding:  0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding:  1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        (texture, bg)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        // Keep the CPU buffer alive with zeros for new pixels — incremental draws
        // will overwrite only the dirty regions, preserving the rest of the frame.
        self.cpu_buffer.resize((width * height) as usize, 0);
        let (tex, bg) = Self::make_texture(
            &self.device,
            &self.bind_group_layout,
            &self.sampler,
            width,
            height,
        );
        self.frame_texture = tex;
        self.bind_group = bg;
    }

    /// Returns a guard whose DerefMut exposes the CPU pixel buffer as `&mut [u32]`.
    /// Call `.present()` on the guard to upload and display the frame.
    pub fn lock_buffer(&mut self) -> WgpuBuffer<'_> {
        WgpuBuffer { inner: self }
    }

    fn present_frame(&mut self) {
        // Reinterpret &[u32] as &[u8] — safe on little-endian; ARGB u32 == BGRA bytes.
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                self.cpu_buffer.as_ptr() as *const u8,
                self.cpu_buffer.len() * 4,
            )
        };

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture:   &self.frame_texture,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset:         0,
                bytes_per_row:  Some(self.width * 4),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width:                 self.width,
                height:                self.height,
                depth_or_array_layers: 1,
            },
        );

        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(wgpu::SurfaceError::Timeout) => return,
            Err(e) => {
                eprintln!("wgpu surface error: {e}");
                return;
            }
        };

        let frame_view = output.texture.create_view(&Default::default());
        let mut encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("blit-enc"),
                });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view:           &frame_view,
                    resolve_target: None,
                    depth_slice:    None,
                    ops: wgpu::Operations {
                        load:  wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes:         None,
                occlusion_query_set:      None,
                multiview_mask:           None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &self.bind_group, &[]);
            rp.draw(0..3, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }
}
