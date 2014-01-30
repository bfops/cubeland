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

extern mod extra;
extern mod cgmath;

use std::comm::Data;
use std::hashmap::HashMap;
use std::hashmap::HashSet;
use std::rt::default_sched_threads;

use extra::comm::DuplexStream;
use extra::time::precise_time_ns;

use cgmath::vector::Vector;
use cgmath::vector::Vec3;

use CHUNK_SIZE;
use VISIBLE_RADIUS;
use WORLD_HEIGHT;
use terrain::Terrain;
use mesh::Mesh;
use ratelimiter::RateLimiter;

static MAX_CHUNKS : uint = (VISIBLE_RADIUS*2)*(VISIBLE_RADIUS*2)*WORLD_HEIGHT*2;
static MAX_INFLIGHT : uint = 32;

pub struct ChunkLoader {
    seed : u32,
    cache : HashMap<(i64, i64, i64), ~Chunk>,
    needed_chunks : ~[Vec3<i64>],
    inflight: HashSet<(i64, i64, i64)>,
    streams: ~[DuplexStream<Vec3<i64>, ~Chunk>],
    load_rate_display_limiter: RateLimiter,
    load_rate_counter: uint,
}

impl ChunkLoader {
    pub fn new(seed : u32) -> ChunkLoader {
        let streams =
            range(0, default_sched_threads()).
            map(|_| ChunkLoader::spawn_worker(seed)).
            to_owned_vec();

        println!("spawned {} workers", streams.len());

        ChunkLoader {
            seed: seed,
            cache: HashMap::new(),
            needed_chunks: ~[],
            inflight: HashSet::new(),
            streams: streams,
            load_rate_display_limiter: RateLimiter::new(1000*1000*1000),
            load_rate_counter: 0,
        }
    }

    fn spawn_worker(seed : u32) -> DuplexStream<Vec3<i64>, ~Chunk> {
        let (loader_stream, worker_stream) = DuplexStream::new();

        do spawn {
            loop {
                let coord : Vec3<i64> = worker_stream.recv();
                println!("loading chunk ({}, {}, {})", coord.x, coord.y, coord.z);
                worker_stream.send(chunk_gen(seed, coord));
            }
        }

        loader_stream
    }

    pub fn get<'a>(&'a self, c: Vec3<i64>) -> Option<&'a ~Chunk> {
        self.cache.find(&(c.x, c.y, c.z))
    }

    pub fn request(&mut self, coords: &[Vec3<i64>]) {
        self.needed_chunks.clear();

        for &c in coords.iter() {
            if self.inflight.contains(&(c.x, c.y, c.z)) {
                continue;
            }

            match self.cache.find_mut(&(c.x, c.y, c.z)) {
                Some(chunk) => {
                    chunk.touch();
                }
                None => {
                    self.needed_chunks.push(c);
                }
            }
        }
    }

    pub fn work(&mut self) {
        for stream in self.streams.iter() {
            loop {
                match stream.try_recv() {
                    Data(mut chunk) => {
                        let c = chunk.coord;
                        chunk.touch();
                        chunk.mesh.finish();
                        self.cache.insert((c.x, c.y, c.z), chunk);
                        self.inflight.remove(&(c.x, c.y, c.z));
                        self.load_rate_counter += 1;
                    },
                    _ => break,
                }
            }
        }

        while self.cache.len() > MAX_CHUNKS {
            let (&k, _) = self.cache.iter().min_by(|&(_, chunk)| chunk.used_time).unwrap();
            self.cache.remove(&k);
        }

        while self.inflight.len() < MAX_INFLIGHT && !self.needed_chunks.is_empty() {
            let c = self.needed_chunks.shift().unwrap();
            self.inflight.insert((c.x, c.y, c.z));
            let worker_index = (c.x, c.y, c.z).hash() as uint % self.streams.len();
            self.streams[worker_index].send(c);
        }

        if self.load_rate_counter > 0 && self.load_rate_display_limiter.limit() {
            println!("loaded {} chunks over the last second", self.load_rate_counter);
            self.load_rate_counter = 0;
        }
    }
}

pub struct Chunk {
    coord: Vec3<i64>,
    terrain: ~Terrain,
    mesh: ~Mesh,
    used_time: u64,
}

impl Chunk {
    pub fn touch(&mut self) {
        self.used_time = extra::time::precise_time_ns();
    }
}

pub fn chunk_gen(seed: u32, coord: Vec3<i64>) -> ~Chunk {
    let p = Vec3::new(coord.x as f64, coord.y as f64, coord.z as f64).mul_s(CHUNK_SIZE as f64);

    let terrain = Terrain::gen(seed, p);

    let mesh = Mesh::gen(terrain);

    return ~Chunk {
        coord: coord,
        terrain: terrain,
        mesh: mesh,
        used_time: extra::time::precise_time_ns(),
    };
}
