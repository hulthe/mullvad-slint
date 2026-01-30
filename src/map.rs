// dunge has a lot of complex types
#![allow(clippy::type_complexity)]

use std::{f32::consts::PI, num::NonZero};

use anyhow::Context;
use dunge::{
    Config, Layer, RenderBuffer,
    buffer::{Buffer, BufferData, Texture2d, TextureData},
    color::{Color, Format},
    mesh::{Mesh, MeshData},
    render::Input,
    set::UniqueSet,
    sl::{self, Global, Groups, PassVertex, Render, Ret},
    storage::Uniform,
    types::Pointer,
    usage::Texture,
};
use glam::{Affine3A, Mat4, Vec2, Vec3, Vec4};
use slint::{PhysicalSize, Rgba8Pixel, SharedPixelBuffer};
use zerocopy::FromBytes;

// const GOTHENBURG: Vec2 = Vec2::new(57.7, 12.0);

const LAND_COLOR: Vec4 = Vec4::new(0.16, 0.302, 0.45, 1.0);
// const LAND_COLOR: Vec4 = Vec4::new(0.049, 0.094, 0.1384, 1.0);
const OCEAN_COLOR: Vec4 = Vec4::new(0.098, 0.18, 0.271, 1.0);
// HACK: Setting the contour color to the ocean color hides the contours inside the globe
const CONTOUR_COLOR: Vec4 = OCEAN_COLOR;

pub struct Map {
    last_input: Option<MapInput>,
    cx: dunge::Context,
    // shader: Shader<RenderInput<Vert, ()>, ()>,
    layer: Layer<
        Input<
            Vec3,
            (),
            (
                Ret<Global, Pointer<dunge::types::Vec4<f32>>>,
                Ret<Global, Pointer<dunge::types::Mat4>>,
                Ret<Global, Pointer<dunge::types::Mat4>>,
            ),
        >,
    >,
    contour_layer: Layer<
        Input<
            Vec3,
            (),
            (
                Ret<Global, Pointer<dunge::types::Vec4<f32>>>,
                Ret<Global, Pointer<dunge::types::Mat4>>,
                Ret<Global, Pointer<dunge::types::Mat4>>,
            ),
        >,
    >,
    land_mesh: Mesh<Vec3>,
    contour_mesh: Mesh<Vec3>,
    texture:
        RenderBuffer<Texture2d<Texture<true, true, true, true>>, Texture<true, true, true, true>>,
    texture_format: Format,
    buffer: Buffer<dunge::usage::MapRead<true>>,
    pixel_buffer: SharedPixelBuffer<Rgba8Pixel>,
    land_set: UniqueSet<(
        Ret<Global, Pointer<dunge::types::Vec4<f32>>>,
        Ret<Global, Pointer<dunge::types::Mat4>>,
        Ret<Global, Pointer<dunge::types::Mat4>>,
    )>,
    contour_set: UniqueSet<(
        Ret<Global, Pointer<dunge::types::Vec4<f32>>>,
        Ret<Global, Pointer<dunge::types::Mat4>>,
        Ret<Global, Pointer<dunge::types::Mat4>>,
    )>,
    // land_color: Uniform<Vec4>,
    // contour_color: Uniform<Vec4>,
    projection: Uniform<Mat4>,
    land_model_view: Uniform<Mat4>,
    contour_model_view: Uniform<Mat4>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapInput {
    pub size: PhysicalSize,
    pub coords: Vec2,
    pub zoom: f32,
}

impl Map {
    pub async fn new(size: PhysicalSize) -> anyhow::Result<Self> {
        let cx = dunge::context().enable_polygon_mode_line().await?;
        let format = Format::RgbAlpha;

        let shader = |PassVertex(v): PassVertex<Vec3>,
                      Groups((color, model_view, projection)): Groups<(
            Uniform<Vec4>,
            Uniform<Mat4>,
            Uniform<Mat4>,
        )>| {
            // Apply the projection and view matrices to the vertex position
            let projection = projection.load();
            let model_view = model_view.load();
            let place = projection * model_view * sl::vec4_append(v, 1.0);

            // Set vertex color
            let color = sl::fragment(color.load());

            // As a result, return a program that describes how to
            // compute the vertex position and the fragment color
            Render { place, color }
        };

        let shader = cx.make_shader(shader);

        let layer = cx.make_layer(
            &shader,
            Config {
                format,
                depth: true,
                ..Default::default()
            },
        );

        let contour_layer = cx.make_layer(
            &shader,
            Config {
                format,
                depth: true,
                polygon: dunge::Polygon::Line,
                topology: dunge::Topology::LineStrip,
                ..Default::default()
            },
        );

        let land_color = cx.make_uniform(&LAND_COLOR);
        let contour_color = cx.make_uniform(&CONTOUR_COLOR);
        let land_model_view = cx.make_uniform(&Mat4::IDENTITY);
        let contour_model_view = cx.make_uniform(&Mat4::IDENTITY);
        let projection = cx.make_uniform(&projection_matrix(size.width as f32, size.height as f32));
        let land_set = cx.make_set(&shader, (&land_color, &land_model_view, &projection));
        let contour_set = cx.make_set(&shader, (&contour_color, &contour_model_view, &projection));

        let land_points = include_bytes!("../geo/land_positions.gl");
        let land_points = <[Vec3]>::ref_from_bytes(land_points.as_slice()).unwrap();

        let land_indices = include_bytes!("../geo/land_triangle_indices.gl");
        let land_indices = <[[u32; 3]]>::ref_from_bytes(land_indices).unwrap();

        let contour_indices = include_bytes!("../geo/land_contour_indices.gl");
        let contour_indices = <[u32]>::ref_from_bytes(contour_indices).unwrap();
        let contour_points = contour_indices
            .iter()
            .map(|&i| land_points[i as usize])
            .collect::<Vec<_>>();

        let contour_mesh =
            cx.make_mesh(&MeshData::from_verts(&contour_points).expect("Land points was empty"));
        let land_mesh = cx.make_mesh(&MeshData::new(land_points, land_indices).expect("mesh data"));

        let w = NonZero::new(size.width).context("width was 0")?;
        let h = NonZero::new(size.height).context("height was 0")?;
        let color_texture = cx.make_texture(
            TextureData::empty((w, h), format)
                .render()
                .bind()
                .copy_from()
                .copy_to(),
        );

        let depth_texture = cx.make_texture(
            TextureData::empty((w, h), Format::Depth)
                .render()
                .bind()
                .copy_from()
                .copy_to(),
        );

        let texture = RenderBuffer::new(color_texture, depth_texture);

        let buffer = cx.make_buffer(
            BufferData::empty(
                texture.color().bytes_per_row_aligned() * u32::from(texture.size().height),
            )
            .read()
            .copy_to(),
        );

        Ok(Map {
            last_input: None,
            cx,
            layer,
            contour_layer,
            land_mesh,
            contour_mesh,
            buffer,
            pixel_buffer: SharedPixelBuffer::new(
                texture.color().bytes_per_row_aligned() / 4,
                size.height,
            ),
            texture,
            texture_format: format,
            land_set,
            contour_set,
            // land_color,
            // contour_color,
            projection,
            land_model_view,
            contour_model_view,
        })
    }

    pub async fn render(&mut self, input: MapInput) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
        if self.last_input.as_ref() == Some(&input) {
            return None;
        }

        if let Some(last_input) = self.last_input
            && input.size == last_input.size
        {
        } else if self.resize(input.size).is_err() {
            return None;
        }

        self.last_input = Some(input);

        let now = std::time::Instant::now();

        // TODO: move to self.resize?
        let texture_dim = [
            // TODO: dulge adds padding on each row. this means the
            // texture will be up to 64 pixels wider than it should be
            self.texture.color().bytes_per_row_aligned() / self.texture_format.bytes(),
            u32::from(self.texture.size().height),
        ];
        let pixel_buf_dim = self.pixel_buffer.size().to_array();
        let [w, h] = texture_dim;
        if pixel_buf_dim != texture_dim {
            self.pixel_buffer = SharedPixelBuffer::new(w, h);
            self.projection.update(
                &self.cx,
                &projection_matrix(input.size.width as f32, input.size.height as f32),
            );
        }

        let model_view = model_view(input.zoom, input.coords);
        self.contour_model_view.update(&self.cx, &model_view);

        let model_view = model_view * Affine3A::from_scale(Vec3::splat(0.9999));
        self.land_model_view.update(&self.cx, &model_view);

        self.cx
            .shed(|s| {
                let background = Color::from_standard([0.0, 0.0, 0.0, 0.0]);
                s.render(
                    &self.texture,
                    dunge::Options::from(background).clear_depth(1.0),
                )
                .layer(&self.layer)
                .set(&self.land_set)
                .draw(&self.land_mesh)
                .layer(&self.contour_layer)
                // TODO: contours is broken
                .set(&self.contour_set)
                .draw(&self.contour_mesh);

                s.copy(self.texture.color(), &self.buffer);
            })
            .await;

        let len = (w * h * self.texture_format.bytes()) as usize;
        match self.cx.read(&mut self.buffer).await {
            Ok(texture_data) => {
                // TODO: this will probably always clone the buffer,
                // since the UI has a reference to it. Can we avoid this?
                self.pixel_buffer.make_mut_bytes()[..len].copy_from_slice(&texture_data[..len]);
            }
            Err(e) => {
                tracing::error!("Error: failed to copy texture: {e:?}");
            }
        }

        let time = now.elapsed();
        tracing::trace!("map render took {time:?}");

        Some(self.pixel_buffer.clone())
    }

    fn resize(&mut self, size: PhysicalSize) -> anyhow::Result<()> {
        let w = NonZero::new(size.width).context("width was 0")?;
        let h = NonZero::new(size.height).context("height was 0")?;
        let texture = self.cx.make_texture(
            TextureData::empty((w, h), self.texture_format)
                .render()
                .bind()
                .copy_from()
                .copy_to(),
        );

        let depth_texture = self.cx.make_texture(
            TextureData::empty((w, h), Format::Depth)
                .render()
                .bind()
                .copy_from()
                .copy_to(),
        );

        self.texture = RenderBuffer::new(texture, depth_texture);
        self.buffer = self.cx.make_buffer(
            BufferData::empty(
                self.texture.color().bytes_per_row_aligned()
                    * u32::from(self.texture.size().height),
            )
            .read()
            .copy_to(),
        );
        Ok(())
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

    //const DISCONNECTED_ZOOM: f32 = 1.35;
    // const CONNECTED_ZOOM: f32 = 1.25;

    // Offset Y for placing the marker at the same area as the spinner. The zoom calculation is
    // required for the unsecured and secured markers to be placed in the same spot.
    // The constants look arbitrary. They are found by just trying stuff until it looks good.
    // let offset_y = 0.088 + (zoom - CONNECTED_ZOOM) * 0.3;
    let offset_y = 0.0;

    // Move the camera back `this.zoom` away from the center of the globe.
    // let view_matrix = view_matrix.append_translation(&Matrix3x1::new(0.0, offset_y, -args.zoom));
    view_matrix *= Affine3A::from_translation(Vec3::new(0.0, offset_y, -zoom));

    // Rotate the globe so the camera ends up looking down on `coords`.
    let (theta, phi) = coordinates_to_theta_phi(coords);
    // let view_matrix = rotate_x(view_matrix, phi);
    // let view_matrix = rotate_y(view_matrix, -theta);
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
