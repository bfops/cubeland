extern mod extra;
extern mod glfw;
extern mod gl;
extern mod cgmath;
extern mod noise;

use std::cast;
use std::ptr;
use std::hashmap::HashMap;
use std;

use gl::types::*;

use cgmath::vector::Vector;
use cgmath::vector::Vec3;
use cgmath::vector::Vec4;

use noise::Perlin;

use CHUNK_SIZE;
use WORLD_SIZE;
use GraphicsResources;

static MAX_CHUNKS : uint = 32;

pub struct ChunkLoader<'a> {
    seed : u32,
    graphics_resources : &'a GraphicsResources,
    cache : HashMap<(i64, i64), ~Chunk>
}

impl<'a> ChunkLoader<'a> {
    pub fn new<'a>(seed : u32, graphics_resources : &'a GraphicsResources) -> ChunkLoader<'a> {
        ChunkLoader {
            seed: seed,
            graphics_resources : graphics_resources,
            cache: HashMap::new(),
        }
    }

    pub fn update(&mut self, x : i64, z: i64) {
        let mask : i64 = !(CHUNK_SIZE as i64 - 1);
        let mut loaded = false;

        for ix in std::iter::range_inclusive(-(WORLD_SIZE as i64)/2, (WORLD_SIZE as i64)/2) {
            for iz in std::iter::range_inclusive(-(WORLD_SIZE as i64)/2, (WORLD_SIZE as i64)/2) {
                let cx : i64 = (x & mask) + ix*CHUNK_SIZE as i64;
                let cz : i64 = (z & mask) + iz*CHUNK_SIZE as i64;
                if !self.cache.contains_key(&(cx, cz)) {
                    if !loaded {
                        println!("loading chunk ({}, {})", cx, cz);
                        let chunk = chunk_gen(self.graphics_resources, self.seed, cx, cz);
                        self.cache.insert((cx, cz), chunk);
                        loaded = true;
                    }
                } else {
                    let chunk = self.cache.find_mut(&(cx, cz)).unwrap();
                    chunk.touch();
                }
            }
        }

        while self.cache.len() > MAX_CHUNKS {
            let (&k, _) = self.cache.iter().min_by(|&(_, chunk)| chunk.used_time).unwrap();
            self.cache.remove(&k);
        }
    }
}

pub struct Chunk {
    x: i64,
    z: i64,
    map: ~Map,
    vao: GLuint,
    vertex_buffer: GLuint,
    normal_buffer: GLuint,
    element_buffer: GLuint,
    num_elements: uint,
    used_time: u64,
}

impl Chunk {
    fn touch(&mut self) {
        self.used_time = extra::time::precise_time_ns();
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        unsafe {
            println!("unloading chunk ({}, {})", self.x, self.z);
            gl::DeleteBuffers(1, &self.vertex_buffer);
            gl::DeleteBuffers(1, &self.normal_buffer);
            gl::DeleteBuffers(1, &self.element_buffer);
            gl::DeleteVertexArrays(1, &self.vao);
        }
    }
}

struct Block {
    visible: bool,
}

struct Map {
    blocks: [[[Block, ..CHUNK_SIZE], ..CHUNK_SIZE], ..CHUNK_SIZE],
}

struct Face {
    normal: Vec3<f32>,
    vertices: [Vec3<f32>, ..4],
}

pub fn chunk_gen(res: &GraphicsResources, seed: u32, chunk_x: i64, chunk_z: i64) -> ~Chunk {
    let def_block = Block { visible: false };
    let mut map = ~Map {
        blocks: [[[def_block, ..CHUNK_SIZE], ..CHUNK_SIZE], ..CHUNK_SIZE],
    };

    let block_exists = |x: int, y: int, z: int| -> bool {
        if x < 0 || x >= CHUNK_SIZE as int || y < 0 || y >= CHUNK_SIZE as int || z < 0 || z >= CHUNK_SIZE as int {
            return false;
        }

        map.blocks[x][y][z].visible
    };

    let perlin = Perlin::from_seed([seed as uint]);

    for block_x in range(0, CHUNK_SIZE) {
        for block_z in range(0, CHUNK_SIZE) {
            let noise = perlin.gen([
                (chunk_x + block_x as i64) as f64 * 0.1,
                (chunk_z + block_z as i64) as f64 * 0.1
            ]);
            let height = ((noise + 1.0) * (CHUNK_SIZE as f64 / 8.0)) as uint;
            for y in range(0, height) {
                map.blocks[block_x][y][block_z] = Block { visible: true };
            }
        }
    }

    let mut vertices : ~[Vec3<f32>] = ~[];
    let mut normals : ~[Vec3<f32>] = ~[];
    let mut elements : ~[GLuint] = ~[];

    let mut idx = 0;

    for x in range(0, CHUNK_SIZE) {
        for y in range(0, CHUNK_SIZE) {
            for z in range(0, CHUNK_SIZE) {
                let block = &map.blocks[x][y][z];

                if (!block.visible) {
                    continue;
                }

                let block_position = Vec3 { x: x as f32, y: y as f32, z: z as f32 };

                for face in faces.iter() {
                    let neighbor_position = block_position.add_v(&face.normal);
                    if block_exists(neighbor_position.x as int, neighbor_position.y as int, neighbor_position.z as int) {
                        continue;
                    }

                    for v in face.vertices.iter() {
                        vertices.push(v.add_v(&block_position));
                        normals.push(face.normal);
                    }

                    for e in face_elements.iter() {
                        elements.push((idx * face.vertices.len()) as GLuint + *e);
                    }

                    idx += 1;
                }
            }
        }
    }

    let mut vao = 0;
    let mut vertex_buffer = 0;
    let mut normal_buffer = 0;
    let mut element_buffer = 0;

    unsafe {
        // Create Vertex Array Object
        gl::GenVertexArrays(1, &mut vao);
        gl::BindVertexArray(vao);

        // Create a Vertex Buffer Object and copy the vertex data to it
        gl::GenBuffers(1, &mut vertex_buffer);
        gl::BindBuffer(gl::ARRAY_BUFFER, vertex_buffer);
        gl::BufferData(gl::ARRAY_BUFFER,
                        (vertices.len() * std::mem::size_of::<Vec3<f32>>()) as GLsizeiptr,
                        cast::transmute(&vertices[0]),
                        gl::STATIC_DRAW);

        // Create a Vertex Buffer Object and copy the normal data to it
        gl::GenBuffers(1, &mut normal_buffer);
        gl::BindBuffer(gl::ARRAY_BUFFER, normal_buffer);
        gl::BufferData(gl::ARRAY_BUFFER,
                        (normals.len() * std::mem::size_of::<Vec3<f32>>()) as GLsizeiptr,
                        cast::transmute(&normals[0]),
                        gl::STATIC_DRAW);

        // Create a Vertex Buffer Object and copy the element data to it
        gl::GenBuffers(1, &mut element_buffer);
        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, element_buffer);
        gl::BufferData(gl::ELEMENT_ARRAY_BUFFER,
                        (elements.len() * std::mem::size_of::<GLuint>()) as GLsizeiptr,
                        cast::transmute(&elements[0]),
                        gl::STATIC_DRAW);

        // Specify the layout of the vertex data
        let vert_attr = "position".with_c_str(|ptr| gl::GetAttribLocation(res.program, ptr));
        assert!(vert_attr as u32 != gl::INVALID_VALUE);
        gl::BindBuffer(gl::ARRAY_BUFFER, vertex_buffer);
        gl::EnableVertexAttribArray(vert_attr as GLuint);
        gl::VertexAttribPointer(vert_attr as GLuint, 3, gl::FLOAT,
                                gl::FALSE as GLboolean, 0, ptr::null());

        let normal_attr = "normal".with_c_str(|ptr| gl::GetAttribLocation(res.program, ptr));
        assert!(normal_attr as u32 != gl::INVALID_VALUE);
        gl::BindBuffer(gl::ARRAY_BUFFER, normal_buffer);
        gl::EnableVertexAttribArray(normal_attr as GLuint);
        gl::VertexAttribPointer(normal_attr as GLuint, 3, gl::FLOAT,
                                gl::FALSE as GLboolean, 0, ptr::null());
    }

    gl::BindVertexArray(0);

    return ~Chunk {
        x: chunk_x,
        z: chunk_z,
        map: map,
        vao: vao,
        vertex_buffer: vertex_buffer,
        normal_buffer: normal_buffer,
        element_buffer: element_buffer,
        num_elements: elements.len(),
        used_time: extra::time::precise_time_ns(),
    };
}

static face_elements : [GLuint, ..6] = [
    0, 1, 2, 3, 2, 1,
];

static faces : [Face, ..6] = [
    /* front */
    Face {
        normal: Vec3 { x: 0.0, y: 0.0, z: 1.0 },
        vertices: [
            Vec3 { x: 0.0, y: 0.0, z: 1.0 }, /* bottom left */
            Vec3 { x: 1.0, y: 0.0, z: 1.0 },  /* bottom right */
            Vec3 { x: 0.0, y: 1.0, z: 1.0 }, /* top left */
            Vec3 { x: 1.0, y: 1.0, z: 1.0 },  /* top right */
        ],
    },

    /* back */
    Face {
        normal: Vec3 { x: 0.0, y: 0.0, z: -1.0 },
        vertices: [
            Vec3 { x: 1.0, y: 0.0, z: 0.0 }, /* bottom right */
            Vec3 { x: 0.0, y: 0.0, z: 0.0 },  /* bottom left */
            Vec3 { x: 1.0, y: 1.0, z: 0.0 }, /* top right */
            Vec3 { x: 0.0, y: 1.0, z: 0.0 },  /* top left */
        ],
    },

    /* right */
    Face {
        normal: Vec3 { x: 1.0, y: 0.0, z: 0.0 },
        vertices: [
            Vec3 { x: 1.0, y: 0.0, z: 1.0 }, /* bottom front */
            Vec3 { x: 1.0, y: 0.0, z: 0.0 }, /* bottom back */
            Vec3 { x: 1.0, y: 1.0, z: 1.0 }, /* top front */
            Vec3 { x: 1.0, y: 1.0, z: 0.0 }, /* top back */
        ],
    },

    /* left */
    Face {
        normal: Vec3 { x: -1.0, y: 0.0, z: 0.0 },
        vertices: [
            Vec3 { x: 0.0, y: 0.0, z: 0.0 }, /* bottom back */
            Vec3 { x: 0.0, y: 0.0, z: 1.0 }, /* bottom front */
            Vec3 { x: 0.0, y: 1.0, z: 0.0 }, /* top back */
            Vec3 { x: 0.0, y: 1.0, z: 1.0 }, /* top front */
        ],
    },

    /* top */
    Face {
        normal: Vec3 { x: 0.0, y: 1.0, z: 0.0 },
        vertices: [
            Vec3 { x: 0.0, y: 1.0, z: 1.0 }, /* front left */
            Vec3 { x: 1.0, y: 1.0, z: 1.0 }, /* front right */
            Vec3 { x: 0.0, y: 1.0, z: 0.0 }, /* back left */
            Vec3 { x: 1.0, y: 1.0, z: 0.0 }, /* back right */
        ],
    },

    /* bottom */
    Face {
        normal: Vec3 { x: 0.0, y: -1.0, z: 0.0 },
        vertices: [
            Vec3 { x: 0.0, y: 0.0, z: 0.0 }, /* back left */
            Vec3 { x: 1.0, y: 0.0, z: 0.0 }, /* back right */
            Vec3 { x: 0.0, y: 0.0, z: 1.0 }, /* front left */
            Vec3 { x: 1.0, y: 0.0, z: 1.0 }, /* front right */
        ],
    },
];
