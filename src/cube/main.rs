// Copyright 2014 The Gfx-rs Developers.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![feature(plugin, custom_attribute)]
#![plugin(gfx_macros)]

extern crate cgmath;
extern crate gfx;
extern crate gfx_window_glfw;
extern crate glfw;

use cgmath::FixedArray;
use cgmath::{Matrix, Point3, Vector3};
use cgmath::{Transform, AffineMatrix3};
use gfx::traits::*;

#[vertex_format]
#[derive(Clone, Copy)]
struct Vertex {
    #[as_float]
    #[name = "a_Pos"]
    pos: [i8; 3],

    #[as_float]
    #[name = "a_TexCoord"]
    tex_coord: [u8; 2],
}

// The shader_param attribute makes sure the following struct can be used to
// pass parameters to a shader.
#[shader_param]
struct Params<R: gfx::Resources> {
    #[name = "u_Transform"]
    transform: [[f32; 4]; 4],

    #[name = "t_Color"]
    color: gfx::shade::TextureParam<R>,
}


//----------------------------------------

pub fn main() {
    let mut glfw = glfw::init(glfw::FAIL_ON_ERRORS).unwrap();
    glfw.set_error_callback(glfw::FAIL_ON_ERRORS);
    let (mut window, events) = glfw
        .create_window(640, 480, "Cube example", glfw::WindowMode::Windowed)
        .unwrap();
    window.set_key_polling(true);

    let mut canvas = gfx_window_glfw::init(window).into_canvas();

    let vertex_data = [
        // top (0, 0, 1)
        Vertex { pos: [-1, -1,  1], tex_coord: [0, 0] },
        Vertex { pos: [ 1, -1,  1], tex_coord: [1, 0] },
        Vertex { pos: [ 1,  1,  1], tex_coord: [1, 1] },
        Vertex { pos: [-1,  1,  1], tex_coord: [0, 1] },
        // bottom (0, 0, -1)
        Vertex { pos: [-1,  1, -1], tex_coord: [1, 0] },
        Vertex { pos: [ 1,  1, -1], tex_coord: [0, 0] },
        Vertex { pos: [ 1, -1, -1], tex_coord: [0, 1] },
        Vertex { pos: [-1, -1, -1], tex_coord: [1, 1] },
        // right (1, 0, 0)
        Vertex { pos: [ 1, -1, -1], tex_coord: [0, 0] },
        Vertex { pos: [ 1,  1, -1], tex_coord: [1, 0] },
        Vertex { pos: [ 1,  1,  1], tex_coord: [1, 1] },
        Vertex { pos: [ 1, -1,  1], tex_coord: [0, 1] },
        // left (-1, 0, 0)
        Vertex { pos: [-1, -1,  1], tex_coord: [1, 0] },
        Vertex { pos: [-1,  1,  1], tex_coord: [0, 0] },
        Vertex { pos: [-1,  1, -1], tex_coord: [0, 1] },
        Vertex { pos: [-1, -1, -1], tex_coord: [1, 1] },
        // front (0, 1, 0)
        Vertex { pos: [ 1,  1, -1], tex_coord: [1, 0] },
        Vertex { pos: [-1,  1, -1], tex_coord: [0, 0] },
        Vertex { pos: [-1,  1,  1], tex_coord: [0, 1] },
        Vertex { pos: [ 1,  1,  1], tex_coord: [1, 1] },
        // back (0, -1, 0)
        Vertex { pos: [ 1, -1,  1], tex_coord: [0, 0] },
        Vertex { pos: [-1, -1,  1], tex_coord: [1, 0] },
        Vertex { pos: [-1, -1, -1], tex_coord: [1, 1] },
        Vertex { pos: [ 1, -1, -1], tex_coord: [0, 1] },
    ];

    let mesh = canvas.factory.create_mesh(&vertex_data);

    let index_data: &[u8] = &[
         0,  1,  2,  2,  3,  0, // top
         4,  5,  6,  6,  7,  4, // bottom
         8,  9, 10, 10, 11,  8, // right
        12, 13, 14, 14, 15, 12, // left
        16, 17, 18, 18, 19, 16, // front
        20, 21, 22, 22, 23, 20, // back
    ];

    let texture = canvas.factory.create_texture_rgba8(1, 1, true).unwrap();
    canvas.factory.update_texture(
        &texture, &texture.get_info().to_image_info(),
        &[0x20u8, 0xA0u8, 0xC0u8, 0x00u8],
        None).unwrap();

    let sampler = canvas.factory.create_sampler(
        gfx::tex::SamplerInfo::new(gfx::tex::FilterMethod::Bilinear,
                                   gfx::tex::WrapMode::Clamp)
    );

    let program = {
        let vs = gfx::ShaderSource {
            glsl_120: Some(include_bytes!("cube_120.glslv")),
            glsl_150: Some(include_bytes!("cube_150.glslv")),
            .. gfx::ShaderSource::empty()
        };
        let fs = gfx::ShaderSource {
            glsl_120: Some(include_bytes!("cube_120.glslf")),
            glsl_150: Some(include_bytes!("cube_150.glslf")),
            .. gfx::ShaderSource::empty()
        };
        canvas.factory.link_program_source(vs, fs, &canvas.device.get_capabilities())
                      .unwrap()
    };

    let view: AffineMatrix3<f32> = Transform::look_at(
        &Point3::new(1.5f32, -5.0, 3.0),
        &Point3::new(0f32, 0.0, 0.0),
        &Vector3::unit_z(),
    );
    let proj = cgmath::perspective(cgmath::deg(45.0f32),
                                   canvas.get_aspect_ratio(), 1.0, 10.0);

    let data = Params {
        transform: proj.mul_m(&view.mat).into_fixed(),
        color: (texture, Some(sampler)),
    };

    let mut batch = gfx::batch::OwnedBatch::new(mesh, program, data).unwrap();
    batch.slice = canvas.factory.create_buffer_index::<u8>(index_data)
                                .to_slice(gfx::PrimitiveType::TriangleList);
    batch.state.depth(gfx::state::Comparison::LessEqual, true);

    while !canvas.output.window.should_close() {
        glfw.poll_events();
        for (_, event) in glfw::flush_messages(&events) {
            match event {
                glfw::WindowEvent::Key(glfw::Key::Escape, _, glfw::Action::Press, _) =>
                    canvas.output.window.set_should_close(true),
                _ => {},
            }
        }

        canvas.clear(gfx::ClearData {
            color: [0.3, 0.3, 0.3, 1.0],
            depth: 1.0,
            stencil: 0,
        });
        canvas.draw(&batch).unwrap();
        canvas.present();
    }
}
