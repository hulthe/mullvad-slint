use std::f32::consts::PI;

use glam::{Affine3A, Mat4, Vec2, Vec3, Vec4};
use slint::wgpu_28::wgpu;
use wgpu::util::DeviceExt;

const LAND_COLOR: Vec4 = Vec4::new(0.16, 0.302, 0.45, 1.0);
const OCEAN_COLOR: Vec4 = Vec4::new(0.098, 0.18, 0.271, 1.0);
// HACK: Setting the contour color to the ocean color hides the contours inside the globe
const CONTOUR_COLOR: Vec4 = OCEAN_COLOR;

/// Uniform buffer layout for rendering
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    projection: [[f32; 4]; 4],
    model_view: [[f32; 4]; 4],
    color: [f32; 4],
    _padding: [f32; 12], // Padding to align to 256 bytes for dynamic offset
}

pub struct Map {
    last_input: Option<MapInput>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    contour_pipeline: wgpu::RenderPipeline,
    land_vertex_buffer: wgpu::Buffer,
    land_index_buffer: wgpu::Buffer,
    land_index_count: u32,
    contour_vertex_buffer: wgpu::Buffer,
    contour_vertex_count: u32,
    land_uniform_buffer: wgpu::Buffer,
    contour_uniform_buffer: wgpu::Buffer,
    land_bind_group: wgpu::BindGroup,
    contour_bind_group: wgpu::BindGroup,
    texture: wgpu::Texture,
    depth_texture: wgpu::Texture,
    texture_size: (u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapInput {
    pub size: slint::PhysicalSize,
    pub coords: Vec2,
    pub zoom: f32,
}

impl Map {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, size: slint::PhysicalSize) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Map Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "map_shader.wgsl"
            ))),
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Map Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Map Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });

        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            }],
        };

        // Land pipeline (triangles)
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Land Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_buffer_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Contour pipeline (line strip)
        let contour_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Contour Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_buffer_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Load geometry data
        let land_points_bytes = include_bytes!("../geo/land_positions.gl");
        let land_points: &[[f32; 3]] = bytemuck::cast_slice(land_points_bytes.as_slice());

        let land_indices_bytes = include_bytes!("../geo/land_triangle_indices.gl");
        let land_indices: &[u32] = bytemuck::cast_slice(land_indices_bytes);

        let contour_indices_bytes = include_bytes!("../geo/land_contour_indices.gl");
        let contour_indices: &[u32] = bytemuck::cast_slice(contour_indices_bytes);
        let contour_points: Vec<[f32; 3]> = contour_indices
            .iter()
            .map(|&i| land_points[i as usize])
            .collect();

        // Create vertex buffers
        let land_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Land Vertex Buffer"),
            contents: bytemuck::cast_slice(land_points),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let land_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Land Index Buffer"),
            contents: bytemuck::cast_slice(land_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let contour_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Contour Vertex Buffer"),
            contents: bytemuck::cast_slice(&contour_points),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Create uniform buffers
        let land_uniforms = Uniforms {
            projection: Mat4::IDENTITY.to_cols_array_2d(),
            model_view: Mat4::IDENTITY.to_cols_array_2d(),
            color: LAND_COLOR.to_array(),
            _padding: [0.0; 12],
        };

        let land_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Land Uniform Buffer"),
            contents: bytemuck::bytes_of(&land_uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let contour_uniforms = Uniforms {
            projection: Mat4::IDENTITY.to_cols_array_2d(),
            model_view: Mat4::IDENTITY.to_cols_array_2d(),
            color: CONTOUR_COLOR.to_array(),
            _padding: [0.0; 12],
        };

        let contour_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Contour Uniform Buffer"),
            contents: bytemuck::bytes_of(&contour_uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create bind groups
        let land_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Land Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: land_uniform_buffer.as_entire_binding(),
            }],
        });

        let contour_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Contour Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: contour_uniform_buffer.as_entire_binding(),
            }],
        });

        let (texture, depth_texture) = Self::create_textures(device, size.width, size.height);

        Self {
            last_input: None,
            device: device.clone(),
            queue: queue.clone(),
            pipeline,
            contour_pipeline,
            land_vertex_buffer,
            land_index_buffer,
            land_index_count: land_indices.len() as u32,
            contour_vertex_buffer,
            contour_vertex_count: contour_points.len() as u32,
            land_uniform_buffer,
            contour_uniform_buffer,
            land_bind_group,
            contour_bind_group,
            texture,
            depth_texture,
            texture_size: (size.width, size.height),
        }
    }

    fn create_textures(
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::Texture) {
        let width = width.max(1);
        let height = height.max(1);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Map Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Map Depth Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        (texture, depth_texture)
    }

    pub fn render(&mut self, input: MapInput) -> Option<wgpu::Texture> {
        // Check if we need to re-render
        if self.last_input.as_ref() == Some(&input) {
            return None;
        }

        let now = std::time::Instant::now();

        // Resize textures if needed
        let new_size = (input.size.width.max(1), input.size.height.max(1));
        if self.texture_size != new_size {
            let (texture, depth_texture) =
                Self::create_textures(&self.device, new_size.0, new_size.1);
            self.texture = texture;
            self.depth_texture = depth_texture;
            self.texture_size = new_size;
        }

        self.last_input = Some(input);

        // Update uniforms
        let projection = projection_matrix(input.size.width as f32, input.size.height as f32);
        let model_view = model_view(input.zoom, input.coords);

        let land_uniforms = Uniforms {
            projection: projection.to_cols_array_2d(),
            model_view: (model_view * Affine3A::from_scale(Vec3::splat(0.9999))).to_cols_array_2d(),
            color: LAND_COLOR.to_array(),
            _padding: [0.0; 12],
        };

        let contour_uniforms = Uniforms {
            projection: projection.to_cols_array_2d(),
            model_view: model_view.to_cols_array_2d(),
            color: CONTOUR_COLOR.to_array(),
            _padding: [0.0; 12],
        };

        self.queue.write_buffer(
            &self.land_uniform_buffer,
            0,
            bytemuck::bytes_of(&land_uniforms),
        );
        self.queue.write_buffer(
            &self.contour_uniform_buffer,
            0,
            bytemuck::bytes_of(&contour_uniforms),
        );

        // Create command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Map Render Encoder"),
            });

        let color_view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = self
            .depth_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Map Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Draw land mesh
            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.land_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.land_vertex_buffer.slice(..));
            render_pass
                .set_index_buffer(self.land_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.land_index_count, 0, 0..1);

            // Draw contour mesh
            render_pass.set_pipeline(&self.contour_pipeline);
            render_pass.set_bind_group(0, &self.contour_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.contour_vertex_buffer.slice(..));
            render_pass.draw(0..self.contour_vertex_count, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));

        let time = now.elapsed();
        tracing::trace!("map render took {time:?}");

        Some(self.texture.clone())
    }
}

fn projection_matrix(width: f32, height: f32) -> Mat4 {
    // Create a perspective matrix, a special matrix that is
    // used to simulate the distortion of perspective in a camera.
    let angle_of_view = 70.0;
    let field_of_view = (angle_of_view / 180.0) * PI; // in radians
    let aspect = width / height;
    let z_near = 0.1;
    let z_far = 10.0;

    Mat4::perspective_rh(field_of_view, aspect, z_near, z_far)
}

fn model_view(zoom: f32, coords: Vec2) -> Mat4 {
    let mut view_matrix = Mat4::IDENTITY;

    // Offset Y for placing the marker at the same area as the spinner.
    let offset_y = 0.0;

    // Move the camera back `zoom` away from the center of the globe.
    view_matrix *= Affine3A::from_translation(Vec3::new(0.0, offset_y, -zoom));

    // Rotate the globe so the camera ends up looking down on `coords`.
    let (theta, phi) = coordinates_to_theta_phi(coords);
    view_matrix *= Affine3A::from_rotation_x(phi);
    view_matrix *= Affine3A::from_rotation_y(-theta);

    view_matrix
}

/// Takes coordinates in degrees and outputs (theta, phi)
fn coordinates_to_theta_phi(coordinate: Vec2) -> (f32, f32) {
    let (latitude, longitude) = (coordinate.x, coordinate.y);
    let phi = latitude * (PI / 180.0);
    let theta = longitude * (PI / 180.0);
    (theta, phi)
}
