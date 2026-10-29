/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The client side of the player physics: the client runs the same physics ticks as the
//! server with its inputs, so the player moves without waiting for the server, and corrects
//! itself with the server's body. Corrections are smoothed away and the drawn position lies
//! between the last two ticks, so the player moves smoothly at any frame rate.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::camera::Camera;
#[cfg(not(feature = "std"))]
use crate::math::Float;
use crate::protocol::{Body, Controls, Request};
use crate::server::TICK;

// Smoothing hides corrections of the predicted position this fast, farther ones jump.
const CORRECTION_SPEED: f32 = 12.0;
const MAX_CORRECTION: f64 = 2.0;

/// The physics of the player between the server's states.
#[derive(Default)]
pub struct Prediction {
    // The last input, the inputs the server has not handled yet, the time towards the next
    // tick, the position before the last tick and the hidden part of a correction.
    sequence: u32,
    inputs: VecDeque<(u32, Controls)>,
    accumulator: f32,
    previous: [f64; 3],
    correction: [f64; 3],
}

impl Prediction {
    /// Starts predicting a player at the camera.
    pub fn new(camera: &Camera) -> Self {
        Self {
            previous: camera.position,
            ..Self::default()
        }
    }

    /// Runs the physics ticks of the passed time with the controls, like the server does with
    /// the inputs, and returns the inputs for the server. `ticked` gets the camera before and
    /// after each tick and the blocks, for the footstep sounds.
    pub fn tick<B: FnMut([i64; 3]) -> Option<u8>>(
        &mut self,
        dt: f32,
        camera: &mut Camera,
        controls: Controls,
        mut block: B,
        mut ticked: impl FnMut(&Camera, &Camera, &mut B),
    ) -> Vec<Request> {
        self.accumulator = (self.accumulator + dt).min(TICK * 5.0);
        let decay = f64::from((-dt * CORRECTION_SPEED).exp());
        self.correction = self.correction.map(|value| value * decay);
        let mut requests = Vec::new();
        while self.accumulator >= TICK {
            self.accumulator -= TICK;
            self.sequence += 1;
            self.previous = camera.position;
            let before = *camera;
            controls.apply(camera);
            camera.tick(TICK, &mut block);
            self.inputs.push_back((self.sequence, controls));
            requests.push(Request::Input {
                sequence: self.sequence,
                controls,
            });
            ticked(&before, camera, &mut block);
        }
        requests
    }

    /// Moves the camera to the server's body and predicts the inputs it did not handle yet,
    /// the difference is smoothed away.
    pub fn reconcile(
        &mut self,
        sequence: u32,
        body: Body,
        camera: &mut Camera,
        mut block: impl FnMut([i64; 3]) -> Option<u8>,
    ) {
        let (predicted, view) = (camera.position, (camera.yaw, camera.pitch));
        body.apply(camera);
        self.inputs.retain(|&(input, _)| input > sequence);
        for (_, controls) in &self.inputs {
            controls.apply(camera);
            camera.tick(TICK, &mut block);
        }
        (camera.yaw, camera.pitch) = view;
        let error: [f64; 3] = core::array::from_fn(|axis| predicted[axis] - camera.position[axis]);
        if error.iter().map(|value| value * value).sum::<f64>() < MAX_CORRECTION * MAX_CORRECTION {
            for ((previous, correction), error) in self
                .previous
                .iter_mut()
                .zip(&mut self.correction)
                .zip(error)
            {
                *previous -= error;
                *correction += error;
            }
        } else {
            self.previous = camera.position;
            self.correction = [0.0; 3];
        }
    }

    /// The camera where it is drawn: between the last two physics ticks and with the rest of a
    /// correction.
    pub fn view(&self, camera: &Camera) -> Camera {
        let alpha = f64::from(self.accumulator / TICK);
        Camera {
            position: core::array::from_fn(|axis| {
                self.previous[axis]
                    + (camera.position[axis] - self.previous[axis]) * alpha
                    + self.correction[axis]
            }),
            ..*camera
        }
    }
}
