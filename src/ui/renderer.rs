use winit::window::Window;

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    width: u32,
    height: u32,
    pixel_buffer: Vec<u8>,
    input_texture: wgpu::Texture, // CPU pixel buffer uploaded here
    intermediate_texture: wgpu::Texture, // Compute shader output
    compute_pipeline: wgpu::ComputePipeline,
    blit_pipeline: wgpu::RenderPipeline, // Simple copy to surface
    workgroup_size: u32,                 // Size of compute shader workgroups (8, 16, or 32)
}

impl Renderer {
    pub async fn new(window: &Window, width: u32, height: u32) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // SAFETY: We extend the lifetime to 'static because the surface will live
        // as long as the renderer, and the window is guaranteed to outlive both
        let surface: wgpu::Surface<'static> =
            unsafe { std::mem::transmute(instance.create_surface(window).unwrap()) };

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();

        // Query GPU limits to pick optimal workgroup size
        let limits = device.limits();
        let max_invocations = limits.max_compute_invocations_per_workgroup;

        // Pick largest square workgroup size that fits in hardware limits
        // Options: 8x8 (64), 16x16 (256), 32x32 (1024)
        let workgroup_size = if max_invocations >= 1024 {
            32
        } else if max_invocations >= 256 {
            16
        } else {
            8
        };

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        // CRITICAL: Use PreMultiplied alpha for proper transparency
        let alpha_mode = surface_caps
            .alpha_modes
            .iter()
            .find(|&&mode| mode == wgpu::CompositeAlphaMode::PreMultiplied)
            .or_else(|| {
                surface_caps
                    .alpha_modes
                    .iter()
                    .find(|&&mode| mode == wgpu::CompositeAlphaMode::PostMultiplied)
            })
            .copied()
            .unwrap_or(surface_caps.alpha_modes[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let pixel_buffer = vec![0u8; (width * height * 4) as usize];

        // Input texture: CPU pixel buffer uploaded here
        let input_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Input Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Intermediate texture: Compute shader writes here (supports storage)
        let intermediate_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Intermediate Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm, // Storage-compatible format
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        // Generate compute shader with optimal workgroup size for this GPU
        let compute_shader_source = generate_compute_shader(workgroup_size);
        let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Compute Shader"),
            source: wgpu::ShaderSource::Wgsl(compute_shader_source.into()),
        });

        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(BLIT_SHADER.into()),
        });

        // Compute pipeline: process pixels
        let compute_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                ],
                label: Some("compute_bind_group_layout"),
            });

        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Compute Pipeline Layout"),
                bind_group_layouts: &[&compute_bind_group_layout],
                push_constant_ranges: &[],
            });

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute Pipeline"),
            layout: Some(&compute_pipeline_layout),
            module: &compute_shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        // Blit pipeline: copy intermediate to surface
        let blit_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    },
                    count: None,
                }],
                label: Some("blit_bind_group_layout"),
            });

        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blit Pipeline Layout"),
            bind_group_layouts: &[&blit_bind_group_layout],
            push_constant_ranges: &[],
        });

        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Blit Pipeline"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            surface,
            device,
            queue,
            config,
            width,
            height,
            pixel_buffer,
            input_texture,
            intermediate_texture,
            compute_pipeline,
            blit_pipeline,
            workgroup_size,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);

            self.pixel_buffer = vec![0u8; (width * height * 4) as usize];

            self.input_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Input Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            self.intermediate_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Intermediate Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
        }
    }

    pub fn get_pixel_buffer_mut(&mut self) -> &mut [u8] {
        &mut self.pixel_buffer
    }

    pub fn present(&mut self) {
        // Upload pixel buffer to input texture
        self.queue.write_texture(
            self.input_texture.as_image_copy(),
            &self.pixel_buffer,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * self.width),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(_) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };

        let output_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Frame Encoder"),
            });

        // Step 1: Compute pass - process pixels
        {
            let input_view = self
                .input_texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let intermediate_view = self
                .intermediate_texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            let compute_bind_group_layout = self.compute_pipeline.get_bind_group_layout(0);
            let compute_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &compute_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&input_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&intermediate_view),
                    },
                ],
                label: Some("compute_bind_group"),
            });

            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Compute Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.compute_pipeline);
            compute_pass.set_bind_group(0, &compute_bind_group, &[]);

            let workgroup_count_x = (self.width + self.workgroup_size - 1) / self.workgroup_size;
            let workgroup_count_y = (self.height + self.workgroup_size - 1) / self.workgroup_size;
            compute_pass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 1);
        }

        // Step 2: Copy intermediate to surface (NO TRIANGLES!)
        // Unfortunately we can't copy directly due to format mismatch (Rgba8Unorm vs Bgra8UnormSrgb)
        // So we still need the render pass, but it's just a texture copy
        {
            let intermediate_view = self
                .intermediate_texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            let blit_bind_group_layout = self.blit_pipeline.get_bind_group_layout(0);
            let blit_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &blit_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&intermediate_view),
                }],
                label: Some("blit_bind_group"),
            });

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Blit Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.blit_pipeline);
            render_pass.set_bind_group(0, &blit_bind_group, &[]);
            // This only runs vertex shader 3 times, then fragment shader once per pixel
            // Fragment shader is just: read pixel from intermediate, write to surface
            render_pass.draw(0..3, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }
}

fn generate_compute_shader(workgroup_size: u32) -> String {
    format!(
        r#"
@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var output_texture: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size({}, {})
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let coords = vec2<i32>(global_id.xy);
    let dims = textureDimensions(output_texture);

    if (coords.x >= i32(dims.x) || coords.y >= i32(dims.y)) {{
        return;
    }}

    // Simple pass-through: CPU does all the rendering (pill shape, text, etc.)
    // This shader is here for future effects: blur, glow, particles, animations
    let colour = textureLoad(input_texture, coords, 0);

    // TODO: Add effects here
    // - Gaussian blur for message bubbles
    // - Glow effects
    // - Particle systems for message deletion
    // - Animation effects

    textureStore(output_texture, coords, colour);
}}
"#,
        workgroup_size, workgroup_size
    )
}

const BLIT_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;

    // Generate fullscreen triangle (only need 3 vertices)
    let x = f32((vertex_index & 1u) << 2u);
    let y = f32((vertex_index & 2u) << 1u);

    out.position = vec4<f32>(x - 1.0, 1.0 - y, 0.0, 1.0);
    out.tex_coords = vec2<f32>(x * 0.5, y * 0.5);

    return out;
}

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = textureDimensions(input_texture);
    let coords = vec2<i32>(in.tex_coords * vec2<f32>(f32(dims.x), f32(dims.y)));

    // Simple copy from intermediate to surface
    return textureLoad(input_texture, coords, 0);
}
"#;
