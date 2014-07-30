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

extern crate std;
extern crate cgmath;

use cgmath::angle::rad;
use cgmath::matrix::{Matrix, Matrix3};
use cgmath::vector::Vector;
use cgmath::vector::Vector2;
use cgmath::vector::Vector3;

static CAMERA_SPEED : f64 = 30.0;
static FAST_MULTIPLIER : f64 = 10.0;

pub struct Camera {
    pub position : Vector3<f64>,
    pub velocity : Vector3<f64>,
    pub angle : Vector2<f64>,
    fast : bool,
}

impl Camera {
    pub fn new(position: Vector3<f64>) -> Camera {
        Camera {
            position: position,
            velocity: Vector3::zero(),
            angle: Vector2::zero(),
            fast: false,
        }
    }

    pub fn accelerate(&mut self, acceleration: Vector3<f64>) {
        self.velocity.add_self_v(&acceleration);
    }

    pub fn fast(&mut self, fast: bool) {
        self.fast = fast;
    }

    pub fn look(&mut self, cursor: Vector2<f64>) {
        self.angle.x = ((cursor.y * 0.0005) % 1.0) * std::f64::consts::PI * 2.0;
        self.angle.y = ((cursor.x * 0.0005) % 1.0) * std::f64::consts::PI * 2.0;
    }

    pub fn tick(&mut self, tick_length: f64) {
        let mut speed = CAMERA_SPEED;
        if self.fast {
            speed *= FAST_MULTIPLIER;
        }

        let inv_camera_rotation = Matrix3::from_euler(rad(-self.angle.x), rad(-self.angle.y), rad(0.0));
        let absolute_camera_velocity = inv_camera_rotation.mul_v(&self.velocity).mul_s(speed).mul_s(tick_length);
        self.position.add_self_v(&absolute_camera_velocity);
    }
}
