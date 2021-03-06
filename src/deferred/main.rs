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


// This is an example of deferred shading with gfx-rs.
//
// Two render targets are created: a geometry buffer and a result buffer.
//
// Rendering happens in two passes:
// First,  the terrain is rendered, writing position, normal and color to the geometry buffer.
// Second, the lights are rendered as cubes. each fragment reads from the geometry buffer,
//         light is applied, and the result is written to the result buffer.
//
// The result buffer is then displayed.
//
// Press 1-4 to show the immediate buffers. Press 0 to show the final result.

#![feature(plugin, custom_attribute)]
#![plugin(gfx_macros)]

extern crate cgmath;
extern crate env_logger;
extern crate gfx;
extern crate gfx_window_glutin;
extern crate glutin;
extern crate time;
extern crate rand;
extern crate genmesh;
extern crate noise;

use rand::Rng;
use cgmath::FixedArray;
use cgmath::{Matrix, Matrix4, Point3, Vector3, EuclideanVector};
use cgmath::{Transform, AffineMatrix3};
use gfx::traits::*;
use gfx::{Plane, RawBufferHandle};
use genmesh::{Vertices, Triangulate};
use genmesh::generators::{SharedVertex, IndexedPolygon};
use time::precise_time_s;

use noise::{Seed, perlin2};

// Remember to also change the constants in the shaders
const NUM_LIGHTS: usize = 250;

#[vertex_format]
#[derive(Clone, Copy)]
struct TerrainVertex {
    #[name = "a_Pos"]
    pos: [f32; 3],
    #[name = "a_Normal"]
    normal: [f32; 3],
    #[name = "a_Color"]
    color: [f32; 3],
}

#[vertex_format]
#[derive(Clone, Copy)]
struct BlitVertex {
    #[as_float]
    #[name = "a_Pos"]
    pos: [i8; 3],
    #[as_float]
    #[name = "a_TexCoord"]
    tex_coord: [u8; 2],
}

#[vertex_format]
#[derive(Clone, Copy)]
struct CubeVertex {
    #[as_float]
    #[name = "a_Pos"]
    pos: [i8; 3],
}

#[shader_param]
struct TerrainParams<R: gfx::Resources> {
    #[name = "u_Model"]
    model: [[f32; 4]; 4],
    #[name = "u_View"]
    view: [[f32; 4]; 4],
    #[name = "u_Proj"]
    proj: [[f32; 4]; 4],
    #[name = "u_CameraPos"]
    cam_pos: [f32; 3],
    _dummy: std::marker::PhantomData<R>,
}

#[shader_param]
struct LightParams<R: gfx::Resources> {
    #[name = "u_Transform"]
    transform: [[f32; 4]; 4],
    #[name = "u_LightPosBlock"]
    light_pos_buf: gfx::RawBufferHandle<R>,
    #[name = "u_Radius"]
    radius: f32,
    #[name = "u_CameraPos"]
    cam_pos: [f32; 3],
    #[name = "u_FrameRes"]
    frame_res: [f32; 2],
    #[name = "u_TexPos"]
    tex_pos: gfx::shade::TextureParam<R>,
    #[name = "u_TexNormal"]
    tex_normal: gfx::shade::TextureParam<R>,
    #[name = "u_TexDiffuse"]
    tex_diffuse: gfx::shade::TextureParam<R>,
}

#[shader_param]
struct EmitterParams<R: gfx::Resources> {
    #[name = "u_Transform"]
    transform: [[f32; 4]; 4],
    #[name = "u_LightPosBlock"]
    light_pos_buf: gfx::RawBufferHandle<R>,
    #[name = "u_Radius"]
    radius: f32,
}

#[shader_param]
struct BlitParams<R: gfx::Resources> {
    #[name = "u_Tex"]
    tex: gfx::shade::TextureParam<R>,
}

static TERRAIN_VERTEX_SRC: &'static [u8] = b"
    #version 150 core

    uniform mat4 u_Model;
    uniform mat4 u_View;
    uniform mat4 u_Proj;
    in vec3 a_Pos;
    in vec3 a_Normal;
    in vec3 a_Color;
    out vec3 v_FragPos;
    out vec3 v_Normal;
    out vec3 v_Color;

    void main() {
        v_FragPos = (u_Model * vec4(a_Pos, 1.0)).xyz;
        v_Normal = a_Normal;
        v_Color = a_Color;
        gl_Position = u_Proj * u_View * u_Model * vec4(a_Pos, 1.0);
    }
";

static TERRAIN_FRAGMENT_SRC: &'static [u8] = b"
    #version 150 core

    in vec3 v_FragPos;
    in vec3 v_Normal;
    in vec3 v_Color;
    out o_Position;
    out o_Normal;
    out o_Color;

    void main() {
        vec3 n = normalize(v_Normal);

        o_Position = vec4(v_FragPos, 0.0);
        o_Normal = vec4(n, 0.0);
        o_Color = vec4(v_Color, 1.0);
    }
";

static BLIT_VERTEX_SRC: &'static [u8] = b"
    #version 150 core

    in vec3 a_Pos;
    in vec2 a_TexCoord;
    out vec2 v_TexCoord;

    void main() {
        v_TexCoord = a_TexCoord;
        gl_Position = vec4(a_Pos, 1.0);
    }
";

static BLIT_FRAGMENT_SRC: &'static [u8] = b"
    #version 150 core

    uniform sampler2D u_Tex;
    in vec2 v_TexCoord;
    out vec4 o_Color;

    void main() {
        vec4 tex = texture(u_Tex, v_TexCoord);
        o_Color = tex;
    }
";

static LIGHT_VERTEX_SRC: &'static [u8] = b"
    #version 150 core

    uniform mat4 u_Transform;
    uniform float u_Radius;
    in vec3 a_Pos;
    out vec3 v_LightPos;

    const int NUM_LIGHTS = 250;
    layout(std140)
    uniform u_LightPosBlock {
        vec4 offs[NUM_LIGHTS];
    };

    void main() {
        v_LightPos = offs[gl_InstanceID].xyz;
        gl_Position = u_Transform * vec4(u_Radius * a_Pos + offs[gl_InstanceID].xyz, 1.0);
    }
";

static LIGHT_FRAGMENT_SRC: &'static [u8] = b"
    #version 150 core

    uniform float u_Radius;
    uniform vec3 u_CameraPos;
    uniform vec2 u_FrameRes;
    uniform sampler2D u_TexPos;
    uniform sampler2D u_TexNormal;
    uniform sampler2D u_TexDiffuse;
    in vec3 v_LightPos;
    out vec4 o_Color;

    void main() {
        vec2 texCoord = gl_FragCoord.xy / u_FrameRes;
        vec3 pos     = texture(u_TexPos,     texCoord).xyz;
        vec3 normal  = texture(u_TexNormal,  texCoord).xyz;
        vec3 diffuse = texture(u_TexDiffuse, texCoord).xyz;

        vec3 light    = v_LightPos;
        vec3 to_light = normalize(light - pos);
        vec3 to_cam   = normalize(u_CameraPos - pos);

        vec3 n = normalize(normal);
        float s = pow(max(0.0, dot(to_cam, reflect(-to_light, n))), 20.0);
        float d = max(0.0, dot(n, to_light));

        float dist_sq = dot(light - pos, light - pos);
        float scale = max(0.0, 1.0-dist_sq/(u_Radius*u_Radius));

        vec3 res_color = d*vec3(diffuse) + vec3(s);

        o_Color = vec4(scale*res_color, 1.0);
    }
";

static EMITTER_VERTEX_SRC: &'static [u8] = b"
    #version 150 core

    uniform mat4 u_Transform;
    uniform float u_Radius;
    in vec3 a_Pos;

    const int NUM_LIGHTS = 250;
    layout(std140)
    uniform u_LightPosBlock {
        vec4 offs[NUM_LIGHTS];
    };

    void main() {
        gl_Position = u_Transform * vec4(u_Radius * a_Pos + offs[gl_InstanceID].xyz, 1.0);
    }
";

static EMITTER_FRAGMENT_SRC: &'static [u8] = b"
    #version 150 core

    out vec4 o_Color;

    void main() {
        o_Color = vec4(1.0, 1.0, 1.0, 1.0);
    }
";

fn calculate_normal(seed: &Seed, x: f32, y: f32)-> [f32; 3] {
    // determine sample points
    let s_x0 = x - 0.001;
    let s_x1 = x + 0.001;
    let s_y0 = y - 0.001;
    let s_y1 = y + 0.001;

    // calculate gradient in point
    let dzdx = (perlin2(seed, &[s_x1, y]) - perlin2(seed, &[s_x0, y]))/(s_x1 - s_x0);
    let dzdy = (perlin2(seed, &[x, s_y1]) - perlin2(seed, &[x, s_y0]))/(s_y1 - s_y0);

    // cross gradient vectors to get normal
    let normal = Vector3::new(1.0, 0.0, dzdx).cross(&Vector3::new(0.0, 1.0, dzdy)).normalize();

    return normal.into_fixed();
}

fn calculate_color(height: f32) -> [f32; 3] {
    if height > 8.0 {
        [0.9, 0.9, 0.9] // white
    } else if height > 0.0 {
        [0.7, 0.7, 0.7] // greay
    } else if height > -5.0 {
        [0.2, 0.7, 0.2] // green
    } else {
        [0.2, 0.2, 0.7] // blue
    }
}

fn create_g_buffer<R: gfx::Resources, F: Factory<R>>(
                   width: gfx::tex::Size, height: gfx::tex::Size, factory: &mut F)
                   -> (gfx::Frame<R>, gfx::TextureHandle<R>, gfx::TextureHandle<R>,
                       gfx::TextureHandle<R>, gfx::TextureHandle<R>) {
    let texture_info_float = gfx::tex::TextureInfo {
        width: width,
        height: height,
        depth: 1,
        levels: 1,
        kind: gfx::tex::TextureKind::Texture2D,
        format: gfx::tex::Format::Float(gfx::tex::Components::RGBA, gfx::attrib::FloatSize::F32),
    };
    let texture_info_depth = gfx::tex::TextureInfo {
        width: width,
        height: height,
        depth: 1,
        levels: 1,
        kind: gfx::tex::TextureKind::Texture2D,
        format: gfx::tex::Format::DEPTH24_STENCIL8,
    };
    let texture_pos     = factory.create_texture(texture_info_float).unwrap();
    let texture_normal  = factory.create_texture(texture_info_float).unwrap();
    let texture_diffuse = factory.create_texture(texture_info_float).unwrap();
    let texture_depth   = factory.create_texture(texture_info_depth).unwrap();

    let frame = gfx::Frame {
        colors: vec![
            Plane::Texture(texture_pos    .clone(), 0, None),
            Plane::Texture(texture_normal .clone(), 0, None),
            Plane::Texture(texture_diffuse.clone(), 0, None),
        ],
        depth: Some(Plane::Texture(texture_depth  .clone(), 0, None)),
        .. gfx::Frame::empty(width, height)
    };

    (frame, texture_pos, texture_normal, texture_diffuse, texture_depth)
}

fn create_res_buffer<R: gfx::Resources, F: Factory<R>>(
                     width: gfx::tex::Size, height: gfx::tex::Size,
                     factory: &mut F, texture_depth: &gfx::TextureHandle<R>)
                     -> (gfx::Frame<R>, gfx::TextureHandle<R>, gfx::TextureHandle<R>) {
    let texture_info_float = gfx::tex::TextureInfo {
        width: width,
        height: height,
        depth: 1,
        levels: 1,
        kind: gfx::tex::TextureKind::Texture2D,
        format: gfx::tex::Format::Float(gfx::tex::Components::RGBA, gfx::attrib::FloatSize::F32),
    };

    let texture_frame = factory.create_texture(texture_info_float).unwrap();

    let frame = gfx::Frame {
        colors: vec![Plane::Texture(texture_frame.clone(), 0, None)],
        depth: Some(Plane::Texture(texture_depth.clone(), 0, None)),
       .. gfx::Frame::empty(width, height)
    };

    (frame, texture_frame, texture_depth.clone())
}

pub fn main() {
    env_logger::init().unwrap();
    let (wrap, mut device, mut factory) = gfx_window_glutin::init(
        glutin::WindowBuilder::new()
            .with_title("Deferred rendering example with gfx-rs".to_string())
            .with_dimensions(800, 600)
            .with_gl(glutin::GlRequest::Latest)
            .build().unwrap()
    );

    let (w, h) = wrap.get_size();
    let mut renderer = factory.create_renderer();
    let mut context = gfx::batch::Context::new();

    let (g_buffer, texture_pos, texture_normal, texture_diffuse, texture_depth) = create_g_buffer(w, h, &mut factory);
    let (res_buffer, texture_frame, _) = create_res_buffer(w, h, &mut factory, &texture_depth);

    let seed = {
        let rand_seed = rand::thread_rng().gen();
        Seed::new(rand_seed)
    };

    let sampler = factory.create_sampler(
        gfx::tex::SamplerInfo::new(gfx::tex::FilterMethod::Scale,
                                   gfx::tex::WrapMode::Clamp)
    );

    let aspect = w as f32 / h as f32;
    let proj = cgmath::perspective(cgmath::deg(60.0f32), aspect, 5.0, 100.0);

    let terrain_scale = Vector3::new(25.0, 25.0, 25.0);
    let mut terrain = {
        let plane = genmesh::generators::Plane::subdivide(256, 256);
        let vertex_data: Vec<TerrainVertex> = plane.shared_vertex_iter()
            .map(|(x, y)| {
                let h = terrain_scale.z * perlin2(&seed, &[x, y]);
                TerrainVertex {
                    pos: [terrain_scale.x * x, terrain_scale.y * y, h],
                    normal: calculate_normal(&seed, x, y),
                    color: calculate_color(h),
                }
            })
            .collect();

        let index_data: Vec<u32> = plane.indexed_polygon_iter()
            .triangulate()
            .vertices()
            .map(|i| i as u32)
            .collect();

        let mesh = factory.create_mesh(&vertex_data);

        let slice = factory
            .create_buffer_index::<u32>(&index_data)
            .to_slice(gfx::PrimitiveType::TriangleList);

        let program = factory.link_program(TERRAIN_VERTEX_SRC, TERRAIN_FRAGMENT_SRC)
                             .unwrap();
        let state = gfx::DrawState::new().depth(gfx::state::Comparison::LessEqual, true);

        let data = TerrainParams {
            model: Matrix4::identity().into_fixed(),
            view: Matrix4::identity().into_fixed(),
            proj: proj.into_fixed(),
            cam_pos: Vector3::new(0.0, 0.0, 0.0).into_fixed(),
            _dummy: std::marker::PhantomData,
        };

        context.make_batch(&program, data, &mesh, slice, &state)
               .unwrap()
    };

    let mut blit = {
        let vertex_data = [
            BlitVertex { pos: [-1, -1, 0], tex_coord: [0, 0] },
            BlitVertex { pos: [ 1, -1, 0], tex_coord: [1, 0] },
            BlitVertex { pos: [ 1,  1, 0], tex_coord: [1, 1] },
            BlitVertex { pos: [-1, -1, 0], tex_coord: [0, 0] },
            BlitVertex { pos: [ 1,  1, 0], tex_coord: [1, 1] },
            BlitVertex { pos: [-1,  1, 0], tex_coord: [0, 1] },
        ];
        let mesh = factory.create_mesh(&vertex_data);
        let slice = mesh.to_slice(gfx::PrimitiveType::TriangleList);

        let program = factory.link_program(BLIT_VERTEX_SRC, BLIT_FRAGMENT_SRC)
                             .unwrap();
        let state = gfx::DrawState::new();

        let data = BlitParams {
          tex: (texture_pos.clone(), Some(sampler.clone())),
        };

        context.make_batch(&program, data, &mesh, slice, &state)
               .unwrap()
    };

    let light_pos_buffer = factory.create_buffer::<[f32; 4]>(NUM_LIGHTS, gfx::BufferUsage::Stream);

    let (mut light, mut emitter) = {
        let vertex_data = [
            // top (0, 0, 1)
            CubeVertex { pos: [-1, -1,  1] },
            CubeVertex { pos: [ 1, -1,  1] },
            CubeVertex { pos: [ 1,  1,  1] },
            CubeVertex { pos: [-1,  1,  1] },
            // bottom (0, 0, -1)
            CubeVertex { pos: [-1,  1, -1] },
            CubeVertex { pos: [ 1,  1, -1] },
            CubeVertex { pos: [ 1, -1, -1] },
            CubeVertex { pos: [-1, -1, -1] },
            // right (1, 0, 0)
            CubeVertex { pos: [ 1, -1, -1] },
            CubeVertex { pos: [ 1,  1, -1] },
            CubeVertex { pos: [ 1,  1,  1] },
            CubeVertex { pos: [ 1, -1,  1] },
            // left (-1, 0, 0)
            CubeVertex { pos: [-1, -1,  1] },
            CubeVertex { pos: [-1,  1,  1] },
            CubeVertex { pos: [-1,  1, -1] },
            CubeVertex { pos: [-1, -1, -1] },
            // front (0, 1, 0)
            CubeVertex { pos: [ 1,  1, -1] },
            CubeVertex { pos: [-1,  1, -1] },
            CubeVertex { pos: [-1,  1,  1] },
            CubeVertex { pos: [ 1,  1,  1] },
            // back (0, -1, 0)
            CubeVertex { pos: [ 1, -1,  1] },
            CubeVertex { pos: [-1, -1,  1] },
            CubeVertex { pos: [-1, -1, -1] },
            CubeVertex { pos: [ 1, -1, -1] },
        ];

        let index_data: &[u8] = &[
             0,  1,  2,  2,  3,  0, // top
             4,  5,  6,  6,  7,  4, // bottom
             8,  9, 10, 10, 11,  8, // right
            12, 13, 14, 14, 15, 12, // left
            16, 17, 18, 18, 19, 16, // front
            20, 21, 22, 22, 23, 20, // back
        ];

        let mesh = factory.create_mesh(&vertex_data);
        let slice = factory
            .create_buffer_index::<u8>(index_data)
            .to_slice(gfx::PrimitiveType::TriangleList);

        let state = gfx::DrawState::new()
            .depth(gfx::state::Comparison::LessEqual, false)
            .blend(gfx::BlendPreset::Add);

        let light_data = LightParams {
            transform: Matrix4::identity().into_fixed(),
            light_pos_buf: light_pos_buffer.raw().clone(),
            radius: 3.0,
            cam_pos: Vector3::new(0.0, 0.0, 0.0).into_fixed(),
            frame_res: [w as f32, h as f32],
            tex_pos: (texture_pos.clone(), Some(sampler.clone())),
            tex_normal: (texture_normal.clone(), Some(sampler.clone())),
            tex_diffuse: (texture_diffuse.clone(), Some(sampler.clone())),
        };

        let light = {
            let program = factory.link_program(LIGHT_VERTEX_SRC, LIGHT_FRAGMENT_SRC)
                                 .unwrap();

            context.make_batch(&program, light_data, &mesh, slice.clone(), &state)
                   .unwrap()
        };

        let emitter_data = EmitterParams {
            transform: Matrix4::identity().into_fixed(),
            light_pos_buf: light_pos_buffer.raw().clone(),
            radius: 0.2,
        };

        let emitter = {
            let program = factory.link_program(EMITTER_VERTEX_SRC, EMITTER_FRAGMENT_SRC)
                                 .unwrap();

            context.make_batch(&program, emitter_data, &mesh, slice, &state)
                   .unwrap()
        };

        (light, emitter)
    };

    let clear_data = gfx::ClearData {
        color: [0.0, 0.0, 0.0, 1.0],
        depth: 1.0,
        stencil: 0,
    };

    let mut debug_buf: Option<gfx::TextureHandle<_>> = None;

    let mut light_pos_vec: Vec<[f32; 4]> = (0 ..NUM_LIGHTS).map(|_| {
        [0.0, 0.0, 0.0, 0.0]
    }).collect();

     'main: loop {
        // quit when Esc is pressed.
        for event in wrap.window.poll_events() {
            use glutin::{Event, VirtualKeyCode};
            match event {
                Event::Closed => break 'main,
                Event::KeyboardInput(_, _, Some(VirtualKeyCode::Escape)) =>
                    break 'main,
                Event::KeyboardInput(_, _, Some(VirtualKeyCode::Numpad1)) =>
                    debug_buf = Some(texture_pos.clone()),
                Event::KeyboardInput(_, _, Some(VirtualKeyCode::Numpad2)) =>
                    debug_buf = Some(texture_normal.clone()),
                Event::KeyboardInput(_, _, Some(VirtualKeyCode::Numpad3)) =>
                    debug_buf = Some(texture_diffuse.clone()),
                Event::KeyboardInput(_, _, Some(VirtualKeyCode::Numpad4)) =>
                    debug_buf = Some(texture_depth.clone()),
                Event::KeyboardInput(_, _, Some(VirtualKeyCode::Numpad0)) =>
                    debug_buf = None,
                _ => {},
            }
        }

        let time = precise_time_s() as f32;

        // Update camera position
        {
            let cam_pos = {
                // Slowly circle the center
                let x = (0.05*time).sin();
                let y = (0.05*time).cos();
                Point3::new(x * 32.0, y * 32.0, 16.0)
            };
            let view: AffineMatrix3<f32> = Transform::look_at(
                &cam_pos,
                &Point3::new(0.0, 0.0, 0.0),
                &Vector3::unit_z(),
            );
            terrain.params.view = view.mat.into_fixed();
            terrain.params.cam_pos = cam_pos.into_fixed();

            light.params.transform = proj.mul_m(&view.mat).into_fixed();
            light.params.cam_pos = cam_pos.into_fixed();

            emitter.params.transform = proj.mul_m(&view.mat).into_fixed();
        }

        // Update light positions
        for (i, p) in light_pos_vec.iter_mut().enumerate() {
            let (x, y) = {
                let fi = i as f32;
                // Distribute lights nicely
                let r = 1.0 - (fi*fi) / ((NUM_LIGHTS*NUM_LIGHTS) as f32);
                (r * (0.2*time + i as f32).cos(), r * (0.2*time + i as f32).sin())
            };
            let h = perlin2(&seed, &[x, y]);

            p[0] = terrain_scale.x * x;
            p[1] = terrain_scale.y * y;
            p[2] = terrain_scale.z * h + 0.5;
        };
        factory.update_buffer(&light_pos_buffer, &light_pos_vec, 0);

        // Render the terrain to the geometry buffer
        renderer.clear(clear_data, gfx::COLOR|gfx::DEPTH, &g_buffer);
        renderer.draw(&(&terrain, &context), &g_buffer).unwrap();

        match debug_buf {
            Some(ref tex) => {
                // Show one of the immediate buffers
                blit.params.tex = (tex.clone(), Some(sampler.clone()));
                renderer.clear(clear_data, gfx::COLOR | gfx::DEPTH, &wrap);
                renderer.draw(
                    &(&blit, &context),
                    &wrap)
                    .unwrap();
            },
            None => {
                renderer.clear(clear_data, gfx::COLOR, &res_buffer);

                // Apply light
                renderer.draw_instanced(
                    &(&light, &context),
                    NUM_LIGHTS as u32, 0, &res_buffer)
                    .unwrap();
                // Draw light emitters
                renderer.draw_instanced(
                    &(&emitter, &context),
                    NUM_LIGHTS as u32, 0, &res_buffer)
                    .unwrap();

                // Show the result
                renderer.clear(clear_data, gfx::COLOR | gfx::DEPTH, &wrap);
                blit.params.tex = (texture_frame.clone(), Some(sampler.clone()));
                renderer.draw(&(&blit, &context), &wrap).unwrap();
            }
        }
        device.submit(renderer.as_buffer());
        renderer.reset();

        wrap.window.swap_buffers();
        device.after_frame();
        factory.cleanup();
    }
}
