use std::num::NonZero;

use anyhow::Context;
use dunge::{
    Config, Layer, Shader, Vertex,
    buffer::{Buffer, BufferData, Texture2d, TextureData},
    color::{Color, Format},
    mesh::{Mesh, MeshData},
    render::Input,
    sl::{self, PassVertex, Render, RenderInput},
};
use glam::Vec3;
use slint::{PhysicalSize, Rgba8Pixel, SharedPixelBuffer};

pub struct Map {
    canvas_size: PhysicalSize,
    cx: dunge::Context,
    // shader: Shader<RenderInput<Vert, ()>, ()>,
    layer: Layer<Input<Vert, (), ()>>,
    mesh: Mesh<Vert>,
    texture: Texture2d<dunge::usage::Texture<true, true, true, true>>,
    texture_format: Format,
    buffer: Buffer<dunge::usage::MapRead<true>>,
    pixel_buffer: SharedPixelBuffer<Rgba8Pixel>,
}

// Create a vertex type
#[repr(C)]
#[derive(Vertex)]
struct Vert {
    pos: Vec3,
    col: Vec3,
}

impl Map {
    pub async fn new(canvas_size: PhysicalSize) -> anyhow::Result<Self> {
        let cx = dunge::context().await?;

        // Create a shader program
        let triangle = |PassVertex(v): PassVertex<Vert>| {
            // Describe the vertex position:
            // take the vertex data as vec2 and expand it to vec4
            let place = sl::vec4(v.pos.x(), v.pos.y(), v.pos.z(), 1.);

            // Then describe the vertex color:
            // first you need to pass the color from
            // vertex shader stage to fragment shader stage
            let fragment_col = sl::fragment(v.col);

            // Now create the final color by adding an alpha value
            let color = sl::vec4_append(fragment_col, 1.);

            // As a result, return a program that describes how to
            // compute the vertex position and the fragment color
            Render { place, color }
        };

        let shader = cx.make_shader(triangle);

        let layer = cx.make_layer(&shader, Config::default());

        let mesh = {
            const VERTS: [Vert; 3] = [
                Vert {
                    pos: Vec3::new(-0.5, -0.5, 0.0),
                    col: Vec3::new(1., 0., 0.),
                },
                Vert {
                    pos: Vec3::new(0.5, -0.5, 0.0),
                    col: Vec3::new(0., 1., 0.),
                },
                Vert {
                    pos: Vec3::new(0., 0.5, 0.0),
                    col: Vec3::new(0., 0., 1.),
                },
            ];

            cx.make_mesh(&MeshData::from_verts(&VERTS).expect("mesh data"))
        };

        let w = NonZero::new(canvas_size.width).context("width was 0")?;
        let h = NonZero::new(canvas_size.height).context("height was 0")?;
        let format = Format::SrgbAlpha;
        let texture = cx.make_texture(
            TextureData::empty((w, h), format)
                .render()
                .bind()
                .copy_from()
                .copy_to(),
        );

        let buffer = cx.make_buffer(
            BufferData::empty(texture.bytes_per_row_aligned() * u32::from(texture.size().height))
                .read()
                .copy_to(),
        );

        Ok(Map {
            canvas_size,
            cx,
            // shader,
            layer,
            mesh,
            buffer,
            pixel_buffer: SharedPixelBuffer::new(
                texture.bytes_per_row_aligned() / 4,
                canvas_size.height,
            ),
            texture,
            texture_format: format,
        })
    }

    pub async fn render(&mut self) -> anyhow::Result<SharedPixelBuffer<Rgba8Pixel>> {
        let texture_dim = [
            // TODO: dulge adds padding on each row. this means the
            // texture will be up to ~16(?) pixels wider than it should be
            self.texture.bytes_per_row_aligned() / self.texture_format.bytes(),
            u32::from(self.texture.size().height),
        ];
        let pixel_buf_dim = self.pixel_buffer.size().to_array();
        let [w, h] = texture_dim;
        if pixel_buf_dim != texture_dim {
            self.pixel_buffer = SharedPixelBuffer::new(w, h);
        }

        self.cx
            .shed(|s| {
                let background = Color::from_standard([0.0, 0.0, 0.0, 0.0]);
                s.render(&self.texture, background)
                    .layer(&self.layer)
                    .draw(&self.mesh);
                s.copy(&self.texture, &self.buffer);
            })
            .await;

        let len = (w * h * self.texture_format.bytes()) as usize;
        let texture_data = self.cx.read(&mut self.buffer).await?;

        self.pixel_buffer.make_mut_bytes()[..len].copy_from_slice(&texture_data[..len]);

        Ok(self.pixel_buffer.clone())
    }

    pub fn resize(&mut self, size: PhysicalSize) -> anyhow::Result<()> {
        if size != self.canvas_size {
            self.canvas_size = size;
            let w = NonZero::new(size.width).context("width was 0")?;
            let h = NonZero::new(size.height).context("height was 0")?;
            let format = Format::SrgbAlpha;
            self.texture = self.cx.make_texture(
                TextureData::empty((w, h), format)
                    .render()
                    .bind()
                    .copy_from()
                    .copy_to(),
            );

            self.buffer = self.cx.make_buffer(
                BufferData::empty(
                    self.texture.bytes_per_row_aligned() * u32::from(self.texture.size().height),
                )
                .read()
                .copy_to(),
            );
        }
        Ok(())
    }
}
