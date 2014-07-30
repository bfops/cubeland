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

extern crate gl;
extern crate hgl;

use std;

use collections::bitv::BitvSet;

use gl::types::*;

use cgmath::vector::Vector;
use cgmath::vector::Vector3;

use CHUNK_SIZE;
use terrain::Terrain;
use terrain::BlockAir;

static NUM_FACES : uint = 6;

// Layout of the vertex buffer sent to the GPU
pub struct VertexData {
    pub position : Vector3<f32>,
    pub blocktype : f32,
}

pub struct Face {
    pub index: uint,
    pub normal: Vector3<f32>,
    di: Vector3<int>,
    dj: Vector3<int>,
    dk: Vector3<int>,
    pub vertices: [Vector3<f32>, ..4],
}

pub struct Mesh {
    pub vertex_buffer: Option<hgl::Vbo>,
    pub element_buffer: Option<hgl::Ebo>,
    pub vertices: Vec<VertexData>,
    pub elements: Vec<GLuint>,
    pub face_ranges: [(uint, uint), ..NUM_FACES],
}

impl Mesh {
    pub fn gen(t: &Terrain) -> Box<Mesh> {
        let mut vertices : Vec<VertexData> = Vec::new();
        let mut elements : Vec<GLuint> = Vec::new();

        static expected_vertices : uint = 8000;
        static expected_elements : uint = expected_vertices * 3 / 2;
        vertices.reserve(expected_vertices);
        elements.reserve(expected_elements);

        let mut face_ranges = [(0, 0), ..6];

        for face in faces.iter() {
            let num_elements_start = elements.len();

            let face_normal_int = Vector3 { x: face.normal.x as int, y: face.normal.y as int, z: face.normal.z as int };

            let mut unmeshed_faces = BlockBitmap::new();
            for x in std::iter::range(0, CHUNK_SIZE as int) {
                for y in std::iter::range(0, CHUNK_SIZE as int) {
                    for z in std::iter::range(0, CHUNK_SIZE as int) {
                        let block = &t.get(x, y, z);

                        if block.blocktype == BlockAir {
                            continue;
                        }

                        let neighbor = t.get(
                            x + face_normal_int.x,
                            y + face_normal_int.y,
                            z + face_normal_int.z);

                        if neighbor.is_opaque() {
                            continue;
                        }

                        unmeshed_faces.insert(x, y, z);
                    }
                }
            }

            for i in std::iter::range(0, CHUNK_SIZE as int) {
                for j in std::iter::range(0, CHUNK_SIZE as int) {
                    for k in std::iter::range(0, CHUNK_SIZE as int) {
                        let Vector3 { x: x, y: y, z: z } = face.di.mul_s(i).add_v(&face.dj.mul_s(j)).add_v(&face.dk.mul_s(k));
                        let block = &t.get(x, y, z);

                        if !unmeshed_faces.contains(x, y, z) {
                            continue;
                        }

                        let block_position = Vector3 {
                            x: x as f32,
                            y: y as f32,
                            z: z as f32,
                        };

                        let dim = expand_face(t, &unmeshed_faces, face, Vector3 { x: x, y: y, z: z });
                        let dim_f = Vector3 { x: dim.x as f32, y: dim.y as f32, z: dim.z as f32 };

                        for dx in range(0, dim.x) {
                            for dy in range(0, dim.y) {
                                for dz in range(0, dim.z) {
                                    unmeshed_faces.remove(x + dx, y + dy, z + dz);
                                }
                            }
                        }

                        let vertex_offset = vertices.len();
                        for v in face.vertices.iter() {
                            vertices.push(VertexData {
                                position: v.mul_v(&dim_f).add_v(&block_position),
                                blocktype: block.blocktype as u8 as f32,
                            });
                        }

                        for e in face_elements.iter() {
                            elements.push(vertex_offset as GLuint + *e);
                        }
                    }
                }
            }

            face_ranges[face.index] = (num_elements_start, elements.len() - num_elements_start);
        }

        box Mesh {
            vertex_buffer: None,
            element_buffer: None,
            vertices: vertices,
            elements: elements,
            face_ranges: face_ranges,
        }
    }

    pub fn finish(&mut self) {
        if !self.elements.is_empty() {
            self.vertex_buffer = Some(hgl::Vbo::from_data(self.vertices.slice(0, self.vertices.len()), hgl::StaticDraw));
            self.element_buffer = Some(hgl::Ebo::from_indices(self.elements.slice(0, self.elements.len())));
        }

        self.vertices.clear();
        self.elements.clear();
    }
}

fn expand_face(t : &Terrain,
               unmeshed_faces : &BlockBitmap,
               face: &Face,
               p: Vector3<int>) -> Vector3<int> {

    let len_k = run_length(t, unmeshed_faces, p, face.dk);
    let len_j = range(0, len_k).
        map(|k| run_length(t, unmeshed_faces, p.add_v(&face.dk.mul_s(k)), face.dj)).
        min().unwrap();

    (Vector3 { x: 1, y: 1, z: 1 }).
        add_v(&face.dk.mul_s(len_k - 1)).
        add_v(&face.dj.mul_s(len_j - 1))
}

fn run_length(t : &Terrain,
              unmeshed_faces : &BlockBitmap,
              mut p: Vector3<int>,
              dp: Vector3<int>) -> int {
    let block = &t.get(p.x, p.y, p.z);
    let max_len = Vector3::new(CHUNK_SIZE as int, CHUNK_SIZE as int, CHUNK_SIZE as int).sub_v(&p).dot(&dp);

    let mut len = 1;

    while len < max_len {
        p.add_self_v(&dp);

        if unmeshed_faces.contains(p.x, p.y, p.z) {
            let b = t.get(p.x, p.y, p.z);
            if b.blocktype == block.blocktype {
                len += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    len
}

struct BlockBitmap {
    set : BitvSet
}

impl BlockBitmap {
    pub fn new() -> BlockBitmap {
        BlockBitmap {
            set: BitvSet::new()
        }
    }

    pub fn contains(&self, x: int, y: int, z: int) -> bool {
        self.set.contains(&BlockBitmap::index(x, y, z))
    }

    pub fn insert(&mut self, x: int, y: int, z: int) {
        self.set.insert(BlockBitmap::index(x, y, z));
    }

    pub fn remove(&mut self, x: int, y: int, z: int) {
        self.set.remove(&BlockBitmap::index(x, y, z));
    }

    fn index(x: int, y: int, z: int) -> uint {
        (x*CHUNK_SIZE as int*CHUNK_SIZE as int + y*CHUNK_SIZE as int + z) as uint
    }
}

static face_elements : [GLuint, ..6] = [
    0, 1, 2, 3, 2, 1,
];

pub static faces : [Face, ..NUM_FACES] = [
    /* front */
    Face {
        index: 0,
        normal: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
        di: Vector3 { x: 0, y: 0, z: 1 },
        dj: Vector3 { x: 1, y: 0, z: 0 },
        dk: Vector3 { x: 0, y: 1, z: 0 },
        vertices: [
            Vector3 { x: 0.0, y: 0.0, z: 1.0 }, /* bottom left */
            Vector3 { x: 1.0, y: 0.0, z: 1.0 },  /* bottom right */
            Vector3 { x: 0.0, y: 1.0, z: 1.0 }, /* top left */
            Vector3 { x: 1.0, y: 1.0, z: 1.0 },  /* top right */
        ],
    },

    /* back */
    Face {
        index: 1,
        normal: Vector3 { x: 0.0, y: 0.0, z: -1.0 },
        di: Vector3 { x: 0, y: 0, z: 1 },
        dj: Vector3 { x: 1, y: 0, z: 0 },
        dk: Vector3 { x: 0, y: 1, z: 0 },
        vertices: [
            Vector3 { x: 1.0, y: 0.0, z: 0.0 }, /* bottom right */
            Vector3 { x: 0.0, y: 0.0, z: 0.0 },  /* bottom left */
            Vector3 { x: 1.0, y: 1.0, z: 0.0 }, /* top right */
            Vector3 { x: 0.0, y: 1.0, z: 0.0 },  /* top left */
        ],
    },

    /* right */
    Face {
        index: 2,
        normal: Vector3 { x: 1.0, y: 0.0, z: 0.0 },
        di: Vector3 { x: 1, y: 0, z: 0 },
        dj: Vector3 { x: 0, y: 1, z: 0 },
        dk: Vector3 { x: 0, y: 0, z: 1 },
        vertices: [
            Vector3 { x: 1.0, y: 0.0, z: 1.0 }, /* bottom front */
            Vector3 { x: 1.0, y: 0.0, z: 0.0 }, /* bottom back */
            Vector3 { x: 1.0, y: 1.0, z: 1.0 }, /* top front */
            Vector3 { x: 1.0, y: 1.0, z: 0.0 }, /* top back */
        ],
    },

    /* left */
    Face {
        index: 3,
        normal: Vector3 { x: -1.0, y: 0.0, z: 0.0 },
        di: Vector3 { x: 1, y: 0, z: 0 },
        dj: Vector3 { x: 0, y: 1, z: 0 },
        dk: Vector3 { x: 0, y: 0, z: 1 },
        vertices: [
            Vector3 { x: 0.0, y: 0.0, z: 0.0 }, /* bottom back */
            Vector3 { x: 0.0, y: 0.0, z: 1.0 }, /* bottom front */
            Vector3 { x: 0.0, y: 1.0, z: 0.0 }, /* top back */
            Vector3 { x: 0.0, y: 1.0, z: 1.0 }, /* top front */
        ],
    },

    /* top */
    Face {
        index: 4,
        normal: Vector3 { x: 0.0, y: 1.0, z: 0.0 },
        di: Vector3 { x: 0, y: 1, z: 0 },
        dj: Vector3 { x: 1, y: 0, z: 0 },
        dk: Vector3 { x: 0, y: 0, z: 1 },
        vertices: [
            Vector3 { x: 0.0, y: 1.0, z: 1.0 }, /* front left */
            Vector3 { x: 1.0, y: 1.0, z: 1.0 }, /* front right */
            Vector3 { x: 0.0, y: 1.0, z: 0.0 }, /* back left */
            Vector3 { x: 1.0, y: 1.0, z: 0.0 }, /* back right */
        ],
    },

    /* bottom */
    Face {
        index: 5,
        normal: Vector3 { x: 0.0, y: -1.0, z: 0.0 },
        di: Vector3 { x: 0, y: 1, z: 0 },
        dj: Vector3 { x: 1, y: 0, z: 0 },
        dk: Vector3 { x: 0, y: 0, z: 1 },
        vertices: [
            Vector3 { x: 0.0, y: 0.0, z: 0.0 }, /* back left */
            Vector3 { x: 1.0, y: 0.0, z: 0.0 }, /* back right */
            Vector3 { x: 0.0, y: 0.0, z: 1.0 }, /* front left */
            Vector3 { x: 1.0, y: 0.0, z: 1.0 }, /* front right */
        ],
    },
];
