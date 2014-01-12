// Copyright 2014 Rich Lane.

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

#[feature(globs)];

extern mod native;
extern mod extra;
extern mod glfw;
extern mod gl;
extern mod cgmath;
extern mod noise;

use std::ptr;
use std::str;
use std::vec;
use std::libc;
use std::io::Timer;

use gl::types::*;

use cgmath::matrix::Matrix;
use cgmath::matrix::Mat3;
use cgmath::matrix::Mat4;
use cgmath::matrix::ToMat4;
use cgmath::vector::Vector;
use cgmath::vector::Vec3;
use cgmath::vector::Vec4;
use cgmath::angle::{rad, deg};
use cgmath::ptr::Ptr;

use spiral::Spiral;

#[cfg(target_os = "linux")]
#[link(name="GLU")]
extern {}

mod chunk;
mod ratelimiter;
mod texture;
mod spiral;

static vertex_shader_src : &'static str = r"
#version 110

uniform mat4 modelview;
uniform mat4 projection;

attribute vec3 position;
attribute vec3 normal;

varying vec4 frag_diffuse_factor;
varying vec2 frag_texcoord;
varying float frag_fog_factor;

const vec3 light_direction = vec3(0.408248, -0.816497, 0.408248);
const vec4 light_diffuse = vec4(0.8, 0.8, 0.8, 0.0);
const vec4 light_ambient = vec4(0.2, 0.2, 0.2, 1.0);

const float planet_radius = 6371000.0 / 5000.0;
const float fog_density = 0.003;

void main() {
    vec4 eye_position = modelview * vec4(position, 1.0);

    /* Curvature of the planet */
    float distance_squared = pow(eye_position.x, 2.0) + pow(eye_position.z, 2.0);
    eye_position.y -= planet_radius - sqrt(pow(planet_radius, 2.0) - distance_squared);

    gl_Position = projection * eye_position;

    vec4 diffuse_factor
        = max(-dot(normal, light_direction), 0.0) * light_diffuse;
    frag_diffuse_factor = diffuse_factor + light_ambient;

    frag_fog_factor = clamp(exp2(-pow(length(eye_position), 2.0) * pow(fog_density, 2.0) * 1.44), 0.0, 1.0);

    if (normal.x != 0.0) {
        frag_texcoord = position.yz;
    } else if (normal.y != 0.0) {
        frag_texcoord = position.xz;
    } else {
        frag_texcoord = position.xy;
    }
    frag_texcoord *= 16.0/1024.0;
}
";

static fragment_shader_src : &'static str = r"
#version 110

const vec4 fog_color = vec4(0.0, 0.75, 1.0, 1.0);

uniform sampler2D texture;

varying vec4 frag_diffuse_factor;
varying vec2 frag_texcoord;
varying float frag_fog_factor;

void main() {
    gl_FragColor = texture2D(texture, frag_texcoord) * frag_diffuse_factor;
    gl_FragColor = mix(fog_color, gl_FragColor, frag_fog_factor);
}
";

pub static VISIBLE_RADIUS: uint = 4;
pub static CHUNK_SIZE: uint = 64;
pub static WORLD_SEED: u32 = 42;

static FRAME_TIME_TARGET_MS : u64 = 16;
static CAMERA_SPEED : f32 = 30.0f32;
static LOD_FACTOR : f32 = 150.0f32;

struct GraphicsResources {
    program: GLuint,
    texture: GLuint,
    uniform_modelview: GLint,
    uniform_projection: GLint,
    uniform_texture: GLint,
}

#[start]
fn start(argc: int, argv: **u8) -> int {
    do native::start(argc, argv) {
        main();
    }
}

fn main() {
   glfw::set_error_callback(~ErrorContext);

    do glfw::start {
        let mut window_width = 800;
        let mut window_height = 600;

        glfw::window_hint::samples(8);

        let window = glfw::Window::create(window_width, window_height,
            "Cubeland", glfw::Windowed)
            .expect("Failed to create GLFW window.");

        window.set_cursor_mode(glfw::CursorDisabled);
        window.make_context_current();

        gl::load_with(glfw::get_proc_address);

        gl::Enable(gl::TEXTURE_2D);
        gl::Enable(gl::DEPTH_TEST);
        gl::Enable(gl::CULL_FACE);

        glfw::set_swap_interval(1);

        let graphics_resources = load_graphics_resources();
        check_gl("after loading graphics resources");

        gl::UseProgram(graphics_resources.program);

        gl::ActiveTexture(gl::TEXTURE0);
        gl::Uniform1i(graphics_resources.uniform_texture, 0);

        gl::BindTexture(gl::TEXTURE_2D, graphics_resources.texture);

        let mut chunk_loader = chunk::ChunkLoader::new(WORLD_SEED);

        window.set_key_callback(~KeyContext);

        let (fb_size_port, fb_size_chan): (Port<(u32,u32)>, Chan<(u32,u32)>) = std::comm::Chan::new();
        window.set_framebuffer_size_callback(~FramebufferSizeContext { chan: fb_size_chan });

        let mut fps_display_limiter = ratelimiter::RateLimiter::new(1000*1000*1000);
        let mut fps_frame_counter = 0;

        let mut camera_position = Vec3::<f32>::new(0.0f32, 30.0f32, 40.0f32);

        //let mut timer = Timer::new().unwrap();

        let mut needed_chunks : ~[(i64, i64, uint)] = ~[];
        let mut load_limiter = ratelimiter::RateLimiter::new(1000*1000*10);

        let mut last_tick = extra::time::precise_time_ns();

        while !window.should_close() {
            let frame_start_time = extra::time::precise_time_ns();

            glfw::poll_events();

            loop {
                match fb_size_port.try_recv() {
                    Some((w,h)) => {
                        window_width = w;
                        window_height = h;
                    }
                    None => break
                }
            }

            let (cursor_x, cursor_y) = window.get_cursor_pos();
            let camera_angle_y = ((cursor_x * 0.0005) % 1.0) * std::f64::consts::PI * 2.0;
            let camera_angle_x = ((cursor_y * 0.0005) % 1.0) * std::f64::consts::PI * 2.0;

            let mut camera_velocity = Vec3::<f32>::new(0.0f32, 0.0f32, 0.0f32);

            match window.get_key(glfw::KeySpace) {
                glfw::Press => { camera_velocity.y += 1.0f32 }
                _ => {}
            }

            match window.get_key(glfw::KeyLeftControl) {
                glfw::Press => { camera_velocity.y += -1.0f32 }
                _ => {}
            }

            match window.get_key(glfw::KeyS) {
                glfw::Press => { camera_velocity.z += 1.0f32 }
                _ => {}
            }

            match window.get_key(glfw::KeyW) {
                glfw::Press => { camera_velocity.z += -1.0f32 }
                _ => {}
            }

            match window.get_key(glfw::KeyD) {
                glfw::Press => { camera_velocity.x += 1.0f32 }
                _ => {}
            }

            match window.get_key(glfw::KeyA) {
                glfw::Press => { camera_velocity.x += -1.0f32 }
                _ => {}
            }

            let now = extra::time::precise_time_ns();
            let tick_length = (now - last_tick) as f32 / (1000 * 1000 * 1000) as f32;
            last_tick = now;

            gl::Viewport(0,0, window_width as GLint, window_height as GLint);

            gl::ClearColor(0.0, 0.75, 1.0, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);

            let projection = cgmath::projection::perspective(
                deg(75.0f32),
                window_width as f32 / window_height as f32,
                0.1f32, 1000.0f32);

            unsafe {
                gl::UniformMatrix4fv(graphics_resources.uniform_projection, 1, gl::FALSE, projection.ptr());
            }

            let camera_translation = Mat4::<f32>::from_cols(
                Vec4::<f32>::unit_x(),
                Vec4::<f32>::unit_y(),
                Vec4::<f32>::unit_z(),
                camera_position.mul_s(-1.0f32).extend(1.0f32));
            let camera_rotation_x = Mat3::<f32>::from_angle_x(rad(camera_angle_x as f32)).to_mat4();
            let camera_rotation_y = Mat3::<f32>::from_angle_y(rad(camera_angle_y as f32)).to_mat4();
            let camera = camera_rotation_x.mul_m(&camera_rotation_y).mul_m(&camera_translation);

            let inv_camera_rotation = Mat3::<f32>::from_euler(rad(-camera_angle_x as f32), rad(-camera_angle_y as f32), rad(0.0f32));
            let absolute_camera_velocity = inv_camera_rotation.mul_v(&camera_velocity).mul_s(CAMERA_SPEED).mul_s(tick_length);
            camera_position.add_self_v(&absolute_camera_velocity);

            let coords = visible_chunks(camera_position.x as i64,
                                        camera_position.z as i64);

            let mut culled = 0;
            let mut rendered = 0;

            for &(cx, cz, lod) in coords.iter() {
                let mut found_lod = 0;

                if !chunk_loader.cache.contains_key(&(cx, cz, lod)) {
                    if !needed_chunks.contains(&(cx, cz, lod)) {
                        needed_chunks.push((cx, cz, lod));
                    }

                    while found_lod < 5 {
                        if chunk_loader.cache.contains_key(&(cx, cz, found_lod)) {
                            break;
                        }
                        found_lod += 1;
                    }
                } else {
                    found_lod = lod;
                }

                match chunk_loader.cache.find_mut(&(cx, cz, found_lod)) {
                    Some(chunk) => {
                        chunk.touch();

                        let chunk_translation = Mat4::<f32>::from_cols(
                            Vec4::<f32>::unit_x(),
                            Vec4::<f32>::unit_y(),
                            Vec4::<f32>::unit_z(),
                            Vec4::<f32>::new(chunk.x as f32, 0.0f32, chunk.z as f32, 1.0f32));

                        let modelview = camera.mul_m(&chunk_translation);
                        let modelviewprojection = projection.mul_m(&modelview);

                        if view_frustum_cull(&modelviewprojection) {
                            culled += 1;
                            continue;
                        }

                        rendered += 1;

                        chunk.bind_arrays(&graphics_resources);

                        unsafe {
                            gl::UniformMatrix4fv(graphics_resources.uniform_modelview, 1, gl::FALSE, modelview.ptr());
                            gl::DrawElements(gl::TRIANGLES, chunk.num_elements as i32, gl::UNSIGNED_INT, ptr::null());
                        }
                    },
                    None => {}
                }
            }

            if false {
                println!("culled={} rendered={}", culled, rendered);
            }

            window.swap_buffers();

            check_gl("main loop");

            if !needed_chunks.is_empty() && load_limiter.limit() {
                let (cx, cz, lod) = needed_chunks.shift();
                chunk_loader.load(cx, cz, lod);
            }

            fps_frame_counter += 1;
            if fps_display_limiter.limit() {
                println!("{} frames per second", fps_frame_counter);
                fps_frame_counter = 0;
            }

            let frame_end_time = extra::time::precise_time_ns();
            let frame_time_ms = (frame_end_time - frame_start_time)/(1000*1000);
            if (frame_time_ms < FRAME_TIME_TARGET_MS) {
                //timer.sleep(FRAME_TIME_TARGET_MS - frame_time_ms);
            }
        }
    }
}

fn visible_chunks(x: i64, z: i64) -> ~[(i64, i64, uint)] {
    static num_chunks : uint = (VISIBLE_RADIUS * 2 + 1) * (VISIBLE_RADIUS * 2 + 1);
    let mask : i64 = !(CHUNK_SIZE as i64 - 1);
    let mut coords = ~[];

    for v in Spiral::<i64>::new(num_chunks) {
        let cx : i64 = (x & mask) + v.x*CHUNK_SIZE as i64;
        let cz : i64 = (z & mask) + v.y*CHUNK_SIZE as i64;
        let dist : f32 = std::num::sqrt(std::num::pow((cx - x) as f32, 2.0f32) + std::num::pow((cz - z) as f32, 2.0f32));
        let lod = (dist / LOD_FACTOR) as uint;
        coords.push((cx, cz, lod));
    }
    coords
}

fn view_frustum_cull(m : &Mat4<f32>) -> bool {
    static L : f32 = CHUNK_SIZE as f32;

    static vertices : [Vec4<f32>, ..8] = [
        Vec4 { x: 0.0, y: 0.0, z: L,   w: 1.0 }, /* front bottom left */
        Vec4 { x: L,   y: 0.0, z: L,   w: 1.0 }, /* front bottom right */
        Vec4 { x: 0.0, y: L,   z: L,   w: 1.0 }, /* front top left */
        Vec4 { x: L,   y: L,   z: L,   w: 1.0 }, /* front top right */
        Vec4 { x: L,   y: 0.0, z: 0.0, w: 1.0 }, /* back bottom right */
        Vec4 { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }, /* back bottom left */
        Vec4 { x: L,   y: L,   z: 0.0, w: 1.0 }, /* back top right */
        Vec4 { x: 0.0, y: L,   z: 0.0, w: 1.0 }, /* back top left */
    ];

    let clip_vertices = vertices.map(|v| m.mul_v(v));

    if clip_vertices.iter().all(|v| v.x < -v.w) {
        return true;
    }

    if clip_vertices.iter().all(|v| v.x > v.w) {
        return true;
    }

    if clip_vertices.iter().all(|v| v.y < -v.w) {
        return true;
    }

    if clip_vertices.iter().all(|v| v.y > v.w) {
        return true;
    }

    if clip_vertices.iter().all(|v| v.z < -v.w) {
        return true;
    }

    if clip_vertices.iter().all(|v| v.z > v.w) {
        return true;
    }

    return false;
}

fn load_graphics_resources() -> GraphicsResources {
    let vs = compile_shader(vertex_shader_src.as_bytes(), gl::VERTEX_SHADER);
    let fs = compile_shader(fragment_shader_src.as_bytes(), gl::FRAGMENT_SHADER);
    let program = link_program(vs, fs);

    let texture = texture::make_noise_texture();

    let uniform_modelview = unsafe { "modelview".with_c_str(|ptr| gl::GetUniformLocation(program, ptr)) };
    let uniform_projection = unsafe { "projection".with_c_str(|ptr| gl::GetUniformLocation(program, ptr)) };
    let uniform_texture = unsafe { "texture".with_c_str(|ptr| gl::GetUniformLocation(program, ptr)) };

    return GraphicsResources {
        program: program,
        texture: texture,
        uniform_modelview: uniform_modelview,
        uniform_projection: uniform_projection,
        uniform_texture: uniform_texture,
    };

}

extern "C" {
    fn gluErrorString(error: GLenum) -> *GLubyte;
}

fn check_gl(message : &str) {
    let err = gl::GetError();
    if (err != gl::NO_ERROR) {
        unsafe {
            let err = std::str::raw::from_c_str(gluErrorString(err) as *i8);
            fail!("GL error {} at {}", err, message);
        }
    }
}

fn compile_shader(src: &[u8], ty: GLenum) -> GLuint {
    let shader = gl::CreateShader(ty);
    unsafe {
        // Attempt to compile the shader
        //transmute is used here because `as` causes ICE
        //wait a sec, is `src` null-terminated properly?
        gl::ShaderSource(shader, 1, std::cast::transmute(std::ptr::to_unsafe_ptr(&src.as_ptr())), ptr::null());
        gl::CompileShader(shader);

        // Get the compile status
        let mut status = gl::FALSE as GLint;
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut status);

        // Fail on error
        if status != (gl::TRUE as GLint) {
            let mut len = 0;
            gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut len);
            let mut buf = vec::from_elem(len as uint - 1, 0u8);     // subtract 1 to skip the trailing null character
            gl::GetShaderInfoLog(shader, len, ptr::mut_null(), buf.as_mut_ptr() as *mut GLchar);
            fail!(str::raw::from_utf8(buf).to_owned());
        }
    }
    shader
}

fn link_program(vs: GLuint, fs: GLuint) -> GLuint {
    let program = gl::CreateProgram();
    gl::AttachShader(program, vs);
    gl::AttachShader(program, fs);
    gl::LinkProgram(program);
    unsafe {
        // Get the link status
        let mut status = gl::FALSE as GLint;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut status);

        // Fail on error
        if status != (gl::TRUE as GLint) {
            let mut len: GLint = 0;
            gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut len);
            let mut buf = vec::from_elem(len as uint - 1, 0u8);     // subtract 1 to skip the trailing null character
            gl::GetProgramInfoLog(program, len, ptr::mut_null(), buf.as_mut_ptr() as *mut GLchar);
            fail!(str::raw::from_utf8(buf).to_owned());
        }
    }
    program
}

struct ErrorContext;
impl glfw::ErrorCallback for ErrorContext {
    fn call(&self, _: glfw::Error, description: ~str) {
        fail!("GLFW Error: {:s}", description);
    }
}

struct KeyContext;
impl glfw::KeyCallback for KeyContext {
    fn call(&self, window: &glfw::Window, key: glfw::Key, _: libc::c_int, action: glfw::Action, _: glfw::Modifiers) {
        match (action, key) {
            (glfw::Press, glfw::KeyEscape) => window.set_should_close(true),
            _ => {}
        }
    }
}

struct FramebufferSizeContext {
    chan: Chan<(u32,u32)>
}
impl glfw::FramebufferSizeCallback for FramebufferSizeContext {
    fn call(&self, _: &glfw::Window, width: i32, height: i32) {
        self.chan.send((width as u32,height as u32));
    }
}
