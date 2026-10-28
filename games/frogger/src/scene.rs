// Copyright (c) 2026 Bastiaan van der Plaat
// SPDX-License-Identifier: MIT

use crate::game::{Game, HALF_WIDTH, Lane, State, Terrain, hash};

const INK: u32 = 0x223b36;
const CREAM: u32 = 0xfff8df;

pub(super) struct Scene {
    pub vertices: Vec<f32>,
    width: f32,
    height: f32,
    scale: f32,
    camera: f32,
    camera_x: f32,
    opacity: f32,
    seed: u32,
    time: f32,
}

impl Scene {
    pub(super) fn new(
        width: f32,
        height: f32,
        game: &Game,
        safe: bwindow::LogicalInsets,
        mobile: bool,
    ) -> Self {
        let view_size = if mobile { 9.5 } else { 16.0 };
        let scale = (width / view_size).min(height / view_size);
        let (frog_x, frog_row, _) = game.position();
        let mut scene = Self {
            vertices: Vec::with_capacity(150_000),
            width,
            height,
            scale,
            camera: if mobile {
                // The closer landscape view still needs room for allowed backward hops.
                game.camera.min(frog_row + height * 0.15 / (scale * 0.70))
            } else {
                game.camera
            },
            camera_x: if mobile {
                frog_x - frog_x.clamp(-2.0, 2.0)
            } else {
                0.0
            },
            opacity: 1.0,
            seed: game.seed,
            time: game.time,
        };
        scene.world(game);
        if mobile {
            scene.system_bar_gradients(safe);
        }
        scene.hud(game, safe.top, mobile);
        scene
    }

    fn vertex(&mut self, point: [f32; 3], color: u32, light: f32) {
        self.vertices.extend_from_slice(&point);
        for shift in [16, 8, 0] {
            self.vertices
                .push(((color >> shift) & 255) as f32 / 255.0 * light);
        }
        self.vertices.push(self.opacity);
    }

    fn quad(&mut self, points: [[f32; 3]; 4], color: u32, light: f32) {
        for index in [0, 1, 2, 0, 2, 3] {
            self.vertex(points[index], color, light);
        }
    }

    fn project(&self, [x, row, y]: [f32; 3]) -> [f32; 3] {
        let r = row - self.camera;
        let px = self.width / 2.0 + (x - self.camera_x + r * 0.22) * self.scale;
        let py = self.height * 0.70 - (r * 0.70 + y * 0.90) * self.scale;
        // Fit depth to the visible row span, leaving room for the HUD and tall objects.
        let depth_span = self.height / (self.scale * 0.70) + 16.0;
        [
            px / self.width * 2.0 - 1.0,
            1.0 - py / self.height * 2.0,
            0.5 + (r - y * (8.0 / 3.0) + x / 60.0) / depth_span * 0.4,
        ]
    }

    fn visible_bounds(&self, row: f32, margin: f32) -> (f32, f32) {
        let center = self.camera_x - (row - self.camera) * 0.22;
        let half_width = self.width / (2.0 * self.scale) + margin;
        (center - half_width, center + half_width)
    }

    const fn ground_color(lane: &Lane) -> u32 {
        match lane.terrain {
            Terrain::Grass if lane.row % 2 == 0 => 0x99c76c,
            Terrain::Grass => 0xadd879,
            Terrain::Road => 0x56616b,
            Terrain::Rail => 0x958878,
            Terrain::Water if lane.row % 2 == 0 => 0x53b8cf,
            Terrain::Water => 0x65c7dc,
        }
    }

    fn shadow_patch(&mut self, [left, right, near, far]: [f32; 4], height: f32, color: u32) {
        if left >= right || near >= far {
            return;
        }
        // Opaque, consistently shaded patches avoid depth artifacts and double-dark overlaps.
        self.quad(
            [
                [left, near, height],
                [right, near, height],
                [right, far, height],
                [left, far, height],
            ]
            .map(|point| self.project(point)),
            color,
            0.72,
        );
    }

    fn shadow(&mut self, [x, row, y]: [f32; 3], [w, d]: [f32; 2], elevation: f32) {
        let size = 1.0 - elevation.clamp(0.0, 1.0) * 0.25;
        let left = x + 0.12 - w * size / 2.0;
        let right = x + 0.12 + w * size / 2.0;
        let near = row - 0.08 - d * size / 2.0;
        let far = row - 0.08 + d * size / 2.0;
        if y > 0.0 {
            // Only log tops receive elevated shadows. Keep the patch inside the actual log.
            let lane = Lane::new(row.round() as i32, self.seed);
            if let Some(center) = lane
                .objects(self.time)
                .into_iter()
                .find(|center| (x - center).abs() <= lane.length / 2.0 - 0.15)
            {
                self.shadow_patch(
                    [
                        left.max(center - lane.length / 2.0),
                        right.min(center + lane.length / 2.0),
                        near.max(lane.row as f32 - 0.38),
                        far.min(lane.row as f32 + 0.38),
                    ],
                    y,
                    0xb48250,
                );
            }
            return;
        }
        // Split at lane boundaries so the shadow follows each receiver's surface color.
        for lane_row in (near + 0.5).floor() as i32..=(far + 0.5).floor() as i32 {
            let lane = Lane::new(lane_row, self.seed);
            self.shadow_patch(
                [
                    left,
                    right,
                    near.max(lane_row as f32 - 0.5),
                    far.min(lane_row as f32 + 0.5),
                ],
                y,
                Self::ground_color(&lane),
            );
        }
    }

    fn block(&mut self, [x, row, y]: [f32; 3], [w, d, h]: [f32; 3], color: u32) {
        let x0 = x - w / 2.0;
        let x1 = x + w / 2.0;
        let r0 = row - d / 2.0;
        let r1 = row + d / 2.0;
        let y1 = y + h;
        for (face, light) in [
            ([[x0, r0, y], [x1, r0, y], [x1, r0, y1], [x0, r0, y1]], 0.73),
            ([[x0, r0, y], [x0, r1, y], [x0, r1, y1], [x0, r0, y1]], 0.86),
            ([[x1, r0, y], [x1, r1, y], [x1, r1, y1], [x1, r0, y1]], 0.62),
            ([[x0, r1, y], [x1, r1, y], [x1, r1, y1], [x0, r1, y1]], 0.8),
            (
                [[x0, r0, y1], [x1, r0, y1], [x1, r1, y1], [x0, r1, y1]],
                1.0,
            ),
        ] {
            self.quad(face.map(|point| self.project(point)), color, light);
        }
    }

    fn world(&mut self, game: &Game) {
        // Include enough rows to cover both ends of any window aspect ratio.
        let behind = (self.height * 0.30 / (self.scale * 0.70)).ceil() + 3.0;
        let ahead = (self.height / self.scale).ceil() + 3.0;
        let near = self.camera - behind;
        let far = self.camera + ahead;
        for row in (near.floor() as i32)..=(far.ceil() as i32) {
            let lane = Lane::new(row, game.seed);
            let color = Self::ground_color(&lane);
            // Keep the ground centered on screen as the oblique view shifts distant rows.
            let ground_x = self.camera_x - (row as f32 - self.camera) * 0.22;
            let ground_width = (self.width / self.scale + 2.0).max(70.0);
            self.block(
                [ground_x, row as f32, -0.28],
                [ground_width, 1.0, 0.25],
                color,
            );
        }
        // Draw every ground receiver before its shadow patches.
        for row in (near.floor() as i32)..=(far.ceil() as i32) {
            let lane = Lane::new(row, game.seed);
            match lane.terrain {
                Terrain::Grass => {
                    for x in -11..=11 {
                        if lane.tree(x, game.seed)
                            || x.abs() > HALF_WIDTH && hash(row as u32 ^ x as u32).is_multiple_of(3)
                        {
                            self.shadow([x as f32, row as f32, -0.015], [0.9, 0.8], 0.0);
                            self.block([x as f32, row as f32, 0.0], [0.22, 0.22, 0.65], 0x91643e);
                            self.block([x as f32, row as f32, 0.50], [0.8, 0.75, 0.65], 0x41885c);
                            self.block([x as f32, row as f32, 1.08], [0.55, 0.52, 0.35], 0x64a75d);
                        } else if hash((row as u32).wrapping_mul(29).wrapping_add(x as u32))
                            .is_multiple_of(4)
                        {
                            let px = x as f32 + 0.28;
                            let r = row as f32 + 0.2;
                            self.block([px, r, 0.0], [0.045, 0.045, 0.14], 0x579950);
                            self.block(
                                [px, r, 0.14],
                                [0.16, 0.16, 0.05],
                                if x % 2 == 0 { 0xffe3a2 } else { 0xf5b4a0 },
                            );
                            self.block([px, r, 0.19], [0.055, 0.055, 0.02], CREAM);
                        }
                    }
                }
                Terrain::Road => {
                    for x in -15..=15 {
                        if x % 2 == 0 {
                            self.block(
                                [x as f32, row as f32 + 0.47, 0.0],
                                [0.65, 0.045, 0.012],
                                0xe8d8b3,
                            );
                        }
                    }
                    let (left, right) = self.visible_bounds(row as f32, lane.length / 2.0 + 0.3);
                    for x in lane.objects_in(game.time, left, right) {
                        self.vehicle(x, row as f32, &lane);
                    }
                }
                Terrain::Rail => {
                    for x in -18..=18 {
                        self.block(
                            [x as f32 * 0.6, row as f32, 0.0],
                            [0.15, 0.86, 0.08],
                            0x6e594d,
                        );
                    }
                    for offset in [-0.29, 0.29] {
                        self.block(
                            [0.0, row as f32 + offset, 0.08],
                            [70.0, 0.065, 0.06],
                            0xd1d8d4,
                        );
                    }
                    for side in [-1.0, 1.0] {
                        let x = side * 6.7;
                        self.block([x, row as f32, 0.0], [0.15, 0.15, 1.5], 0xeae3cb);
                        self.block([x, row as f32, 1.1], [0.7, 0.16, 0.28], INK);
                        for light in [-1.0, 1.0] {
                            let lit = lane.train_phase(game.time) >= 3.5
                                && ((game.time * 5.0) as i32 % 2 == 0) == (light < 0.0);
                            self.block(
                                [x + light * 0.19, row as f32 - 0.10, 1.15],
                                [0.15, 0.05, 0.16],
                                if lit { 0xff665e } else { 0x683f42 },
                            );
                        }
                    }
                    let (left, right) = self.visible_bounds(row as f32, 5.5);
                    for x in lane.trains_in(game.time, left, right) {
                        let direction = lane.speed.signum();
                        for car in 0..4 {
                            let cx = x + direction * (1.5 - car as f32) * 2.5;
                            self.shadow([cx, row as f32, -0.015], [2.35, 0.95], 0.0);
                            if car == 0 {
                                self.locomotive(cx, row as f32, direction);
                            } else {
                                self.block([cx, row as f32, 0.22], [2.35, 0.82, 0.86], 0xe9be55);
                                self.block([cx, row as f32, 1.08], [2.4, 0.88, 0.12], 0x52676c);
                                self.block(
                                    [cx, row as f32 - 0.42, 0.35],
                                    [2.3, 0.03, 0.12],
                                    0xf8e9b8,
                                );
                                for window in [-0.75, -0.25, 0.25, 0.75] {
                                    self.block(
                                        [cx + window, row as f32 - 0.43, 0.70],
                                        [0.32, 0.035, 0.24],
                                        0x447980,
                                    );
                                }
                            }
                            if car < 3 {
                                self.block(
                                    [cx - direction * 1.25, row as f32, 0.26],
                                    [0.25, 0.18, 0.12],
                                    INK,
                                );
                            }
                            for wheel in [-0.8, 0.8] {
                                self.block(
                                    [cx + wheel, row as f32 - 0.36, 0.10],
                                    [0.35, 0.18, 0.26],
                                    INK,
                                );
                            }
                        }
                    }
                }
                Terrain::Water => {
                    for x in -13..=13 {
                        let drift = (game.time * 0.4 + x as f32 * 0.8).sin() * 0.2;
                        self.block(
                            [x as f32 + drift, row as f32 + 0.22, 0.0],
                            [0.38, 0.045, 0.012],
                            0x99dce4,
                        );
                    }
                    let (left, right) = self.visible_bounds(row as f32, lane.length / 2.0 + 0.3);
                    for x in lane.objects_in(game.time, left, right) {
                        self.shadow([x, row as f32, -0.015], [lane.length, 0.76], 0.0);
                        self.block([x, row as f32, 0.02], [lane.length, 0.76, 0.19], 0xb48250);
                    }
                }
            }
        }
        self.opacity = 0.5;
        for side in [-1.0, 1.0] {
            let x = side * (HALF_WIDTH as f32 + 0.5);
            self.quad(
                [
                    [x - 0.035, near, 0.015],
                    [x + 0.035, near, 0.015],
                    [x + 0.035, far, 0.015],
                    [x - 0.035, far, 0.015],
                ]
                .map(|p| self.project(p)),
                0xffffff,
                1.0,
            );
        }
        self.opacity = 1.0;
        self.frog(game);
    }

    fn locomotive(&mut self, x: f32, row: f32, direction: f32) {
        self.block([x, row, 0.22], [2.35, 0.84, 0.18], 0x52676c);
        // A raised cab behind the long hood makes the leading end distinct in either direction.
        self.block(
            [x - direction * 0.62, row, 0.40],
            [0.84, 0.82, 0.91],
            0x4f7774,
        );
        self.block(
            [x - direction * 0.62, row, 1.31],
            [0.98, 0.92, 0.12],
            0x384d53,
        );
        self.block(
            [x + direction * 0.36, row, 0.40],
            [1.40, 0.72, 0.57],
            0x4f7774,
        );
        self.block(
            [x + direction * 1.08, row, 0.40],
            [0.10, 0.76, 0.48],
            0xe9be55,
        );
        self.block(
            [x + direction * 1.17, row, 0.24],
            [0.08, 0.90, 0.14],
            0xc56b5b,
        );
        self.block(
            [x + direction * 0.22, row, 0.97],
            [0.18, 0.22, 0.22],
            0x384d53,
        );
        self.block(
            [x - direction * 0.18, row, 0.96],
            [0.03, 0.56, 0.25],
            0xb8dedb,
        );
        for side in [-1.0, 1.0] {
            self.block(
                [x - direction * 0.62, row + side * 0.42, 0.93],
                [0.52, 0.03, 0.28],
                0xb8dedb,
            );
            self.block([x, row + side * 0.43, 0.43], [2.20, 0.03, 0.10], 0xf8e9b8);
            self.block(
                [x + direction * 1.14, row + side * 0.23, 0.72],
                [0.04, 0.14, 0.12],
                0xfff6c7,
            );
            for vent in [0.0, 0.22, 0.44, 0.66] {
                self.block(
                    [x + direction * vent, row + side * 0.37, 0.62],
                    [0.07, 0.025, 0.20],
                    0x384d53,
                );
            }
        }
    }

    fn vehicle(&mut self, x: f32, row: f32, lane: &Lane) {
        let direction = lane.speed.signum();
        let color =
            [0xf17f74, 0xf3cc65, 0x7f9ee0, 0xc29bd9, 0x75b4ac][hash(row as u32) as usize % 5];
        self.shadow([x, row, -0.015], [lane.length + 0.15, 0.83], 0.0);
        self.block([x, row, 0.16], [lane.length, 0.72, 0.35], color);
        if lane.length > 2.0 {
            self.block(
                [x - direction * 0.44, row, 0.50],
                [1.72, 0.77, 0.80],
                0xf1e8d3,
            );
            self.block([x + direction * 0.97, row, 0.51], [0.7, 0.68, 0.55], color);
            self.block(
                [x + direction * 0.98, row - 0.35, 0.76],
                [0.42, 0.03, 0.24],
                0xb8dedb,
            );
            self.block(
                [x - direction * 0.44, row - 0.40, 0.82],
                [1.50, 0.025, 0.14],
                color,
            );
        } else {
            self.block(
                [x - direction * 0.12, row, 0.51],
                [0.78, 0.61, 0.28],
                0xb8dedb,
            );
            self.block(
                [x - direction * 0.12, row, 0.79],
                [0.60, 0.64, 0.045],
                color,
            );
        }
        for wheel in [-1.0, 1.0] {
            for side in [-1.0, 1.0] {
                self.block(
                    [x + wheel * lane.length * 0.32, row + side * 0.36, 0.04],
                    [0.30, 0.14, 0.28],
                    INK,
                );
                self.block(
                    [x + wheel * lane.length * 0.32, row + side * 0.435, 0.12],
                    [0.12, 0.015, 0.12],
                    0x9eafaa,
                );
            }
        }
        for side in [-1.0, 1.0] {
            self.block(
                [
                    x + direction * (lane.length / 2.0 + 0.01),
                    row + side * 0.23,
                    0.26,
                ],
                [0.04, 0.16, 0.13],
                0xffefb5,
            );
            self.block(
                [
                    x - direction * (lane.length / 2.0 + 0.01),
                    row + side * 0.23,
                    0.26,
                ],
                [0.04, 0.14, 0.12],
                0xef7b70,
            );
        }
    }

    fn frog(&mut self, game: &Game) {
        let (x, row, hop) = game.position();
        let water = Lane::new(game.row, game.seed).terrain == Terrain::Water;
        let dying = matches!(game.state, State::Dying | State::Dead);
        let progress = if dying {
            (game.death_time / 0.75).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let splash = dying && matches!(game.reason, "FELL IN THE WATER" | "SWEPT AWAY");
        if game.state == State::Dying {
            self.death_effect(x, row, progress, splash);
        }
        if splash && progress > 0.4 {
            return;
        }
        let y = hop + if water { 0.23 } else { 0.02 } - if splash { progress * 1.6 } else { 0.0 };
        let color = if dying { 0xdba186 } else { 0x8dde67 };
        if !splash {
            let lane = Lane::new(row.round() as i32, game.seed);
            let on_log = lane.terrain == Terrain::Water && lane.supports(x, game.time);
            self.shadow(
                [x, row, if on_log { 0.215 } else { -0.015 }],
                [0.75, 0.58],
                hop.max(0.0),
            );
        }
        let (dx, dy) = (game.facing.0 as f32, game.facing.1 as f32);
        // Model coordinates: positive local row points toward the frog's face.
        let mut part = |[px, pr, py]: [f32; 3], [w, d, h]: [f32; 3], c| {
            self.block(
                [
                    x + px * dy + pr * dx,
                    row - px * dx + pr * dy,
                    y + py * (1.0 - progress * 0.8),
                ],
                [
                    (w * dy.abs() + d * dx.abs()) * (1.0 + progress * 0.3),
                    (d * dy.abs() + w * dx.abs()) * (1.0 + progress * 0.3),
                    h * (1.0 - progress * 0.8),
                ],
                c,
            );
        };
        part([0.0, 0.0, 0.12], [0.64, 0.57, 0.32], color);
        part([0.0, 0.12, 0.13], [0.46, 0.39, 0.16], 0xb7ef83);
        part([0.0, -0.06, 0.445], [0.26, 0.26, 0.02], 0x70c65b);
        for side in [-1.0, 1.0] {
            part([side * 0.30, -0.14, 0.0], [0.25, 0.37, 0.16], 0x5fb859);
            part([side * 0.28, 0.27, 0.02], [0.20, 0.24, 0.10], 0x76ce61);
            part([side * 0.20, 0.17, 0.39], [0.22, 0.24, 0.21], color);
            part([side * 0.20, 0.295, 0.43], [0.17, 0.03, 0.15], CREAM);
            part([side * 0.20, 0.315, 0.46], [0.085, 0.025, 0.09], INK);
            part([side * 0.27, 0.31, 0.30], [0.07, 0.025, 0.055], 0xf5b1a0);
        }
    }

    fn death_effect(&mut self, x: f32, row: f32, progress: f32, splash: bool) {
        self.opacity = (1.0 - progress) * 0.85;
        let color = if splash { 0xe3faff } else { 0xffe4a1 };
        for index in 0..8 {
            let angle = index as f32 * std::f32::consts::TAU / 8.0;
            let radius = 0.25 + progress * 1.2;
            let height = 0.12 + 4.0 * progress * (1.0 - progress) * if splash { 0.7 } else { 0.9 };
            let size = 0.12 * (1.0 - progress * 0.6);
            self.block(
                [x + angle.cos() * radius, row + angle.sin() * radius, height],
                [size, size, if splash { size * 1.8 } else { size }],
                color,
            );
        }
        if splash {
            let radius = 0.25 + progress * 1.1;
            for side in [-1.0, 1.0] {
                self.block(
                    [x + side * radius, row, 0.02],
                    [0.045, radius * 2.0, 0.012],
                    color,
                );
                self.block(
                    [x, row + side * radius, 0.02],
                    [radius * 2.0, 0.045, 0.012],
                    color,
                );
            }
        }
        self.opacity = 1.0;
    }

    fn system_bar_gradients(&mut self, safe: bwindow::LogicalInsets) {
        let unit = (self.width / 400.0).clamp(0.75, 1.5);
        let top_end = (safe.top + 12.0 * unit).min(self.height);
        self.vertical_gradient(0.0, top_end, [0.88, 0.0]);
        // Android samples the content beneath the gesture handle to choose its contrast.
        let bottom_height = (safe.bottom + 18.0 * unit)
            .max(32.0 * unit)
            .min(self.height * 0.2);
        self.vertical_gradient(self.height - bottom_height, self.height, [0.0, 0.82]);
    }

    fn vertical_gradient(&mut self, top: f32, bottom: f32, alpha: [f32; 2]) {
        let top = 1.0 - top / self.height * 2.0;
        let bottom = 1.0 - bottom / self.height * 2.0;
        let points = [
            [-1.0, top, 0.0],
            [1.0, top, 0.0],
            [1.0, bottom, 0.0],
            [-1.0, bottom, 0.0],
        ];
        for index in [0, 1, 2, 0, 2, 3] {
            self.opacity = alpha[usize::from(index >= 2)];
            self.vertex(points[index], 0x0d1b1b, 1.0);
        }
        self.opacity = 1.0;
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        // UI is drawn in submission order in front of all world geometry.
        // LessEqual lets later UI triangles overwrite earlier ones at the same depth.
        let z = 0.0;
        self.quad(
            [[x, y, z], [x + w, y, z], [x + w, y + h, z], [x, y + h, z]]
                .map(|[x, y, z]| [x / self.width * 2.0 - 1.0, 1.0 - y / self.height * 2.0, z]),
            color,
            1.0,
        );
    }

    fn text(&mut self, text: &str, x: f32, y: f32, size: f32, color: u32) {
        for (i, character) in text.chars().enumerate() {
            let rows = glyph(character);
            for (row, bits) in rows.into_iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        self.rect(
                            x + (i * 6 + col) as f32 * size,
                            y + row as f32 * size,
                            size,
                            size,
                            color,
                        );
                    }
                }
            }
        }
    }

    fn centered(&mut self, text: &str, y: f32, size: f32, color: u32) {
        self.text(
            text,
            (self.width - (text.chars().count() * 6 - 1) as f32 * size) / 2.0,
            y,
            size,
            color,
        );
    }

    /// A small raised button of the top bar
    fn button(&mut self, x: f32, y: f32, unit: f32) {
        self.rect(
            x + 3.0 * unit,
            y + 3.0 * unit,
            32.0 * unit,
            50.0 * unit,
            0x547c59,
        );
        self.rect(x, y, 32.0 * unit, 50.0 * unit, CREAM);
    }

    fn score_card(&mut self, x: f32, y: f32, width: f32, label: &str, value: i32, unit: f32) {
        let height = 50.0 * unit;
        self.rect(x + 3.0 * unit, y + 3.0 * unit, width, height, 0x547c59);
        self.rect(x, y, width, height, CREAM);
        let accent = if label == "BEST" { 0xe5b64f } else { 0x8dde67 };
        self.rect(x, y, width, 3.0 * unit, accent);
        self.text(label, x + 8.0 * unit, y + 8.0 * unit, 1.25 * unit, INK);
        let number = value.to_string();
        let size = (3.0 * unit).min((width - 16.0 * unit) / (number.len() * 6 - 1) as f32);
        self.text(&number, x + 8.0 * unit, y + 24.0 * unit, size, INK);
    }

    fn onboarding(&mut self, unit: f32) {
        let unit = unit.min(self.height / 340.0);
        let width = (self.width - 32.0).min(380.0);
        let height = 270.0 * unit;
        let x = (self.width - width) / 2.0;
        let y = (self.height - height) / 2.0;
        self.rect(x + 4.0, y + 4.0, width, height, 0x547c59);
        self.rect(x, y, width, height, CREAM);
        self.centered("FROGGER", y + 18.0 * unit, 3.5 * unit, INK);
        let instructions = [
            ("↑", "TAP OR PRESS ↑ / W"),
            ("← →", "SWIPE OR PRESS ← → / A D"),
            ("↓", "SWIPE DOWN OR PRESS ↓ / S"),
        ];
        for (index, (symbol, label)) in instructions.into_iter().enumerate() {
            let row = y + (66.0 + index as f32 * 46.0) * unit;
            self.centered(symbol, row, 2.0 * unit, 0x579950);
            self.centered(label, row + 21.0 * unit, 1.3 * unit, INK);
        }
        let action_y = y + 224.0 * unit;
        self.rect(
            x + 14.0 * unit,
            action_y,
            width - 28.0 * unit,
            32.0 * unit,
            0x8dde67,
        );
        self.centered(
            "TAP / SPACE TO START",
            action_y + 11.0 * unit,
            1.4 * unit,
            INK,
        );
    }

    fn hud(&mut self, game: &Game, top: f32, mobile: bool) {
        let unit = (self.width / 400.0).clamp(0.75, 1.5);
        let pad = 12.0 * unit;
        let y = top + pad;
        self.score_card(pad, y, 72.0 * unit, "HOPS", game.score, unit);
        self.score_card(
            self.width - pad - 148.0 * unit,
            y,
            72.0 * unit,
            "BEST",
            game.best,
            unit,
        );
        let pause_x = self.width - pad - 70.0 * unit;
        self.button(pause_x, y, unit);
        let mute_x = self.width - pad - 32.0 * unit;
        self.button(mute_x, y, unit);
        let speaker = if game.muted { "🔈🔇" } else { "🔈🔉" };
        self.text(
            speaker,
            mute_x + 5.0 * unit,
            y + 18.0 * unit,
            2.0 * unit,
            INK,
        );
        if game.state == State::Paused {
            self.text("▶", pause_x + 11.0 * unit, y + 18.0 * unit, 2.0 * unit, INK);
        } else {
            for offset in [9.0, 19.0] {
                self.rect(
                    pause_x + offset * unit,
                    y + 18.0 * unit,
                    4.0 * unit,
                    14.0 * unit,
                    INK,
                );
            }
        }

        if game.state == State::Playing && game.idle >= 5.0 {
            self.centered(
                &format!("MOVE FORWARD  {}", (10.0 - game.idle).ceil() as i32),
                top + 92.0 * unit,
                2.0 * unit,
                INK,
            );
        }
        if game.state == State::Ready {
            self.onboarding(unit);
        }
        if matches!(game.state, State::Paused | State::Dead) {
            let dead = game.state == State::Dead;
            let width = (self.width - 32.0).min(380.0);
            let height = if dead { 122.0 } else { 102.0 } * unit;
            let x = (self.width - width) / 2.0;
            let y = (self.height - height) / 2.0;
            self.rect(x + 4.0, y + 4.0, width, height, 0x547c59);
            self.rect(x, y, width, height, CREAM);
            self.centered(
                if dead { "GAME OVER" } else { "PAUSED" },
                y + 18.0 * unit,
                3.5 * unit,
                INK,
            );
            if dead {
                self.centered(game.reason, y + 54.0 * unit, 1.3 * unit, INK);
            }
            let action_y = y + height - 46.0 * unit;
            self.rect(
                x + 14.0 * unit,
                action_y,
                width - 28.0 * unit,
                32.0 * unit,
                0x8dde67,
            );
            self.centered(
                match (dead, mobile) {
                    (false, true) => "TAP TO RESUME",
                    (false, false) => "P TO RESUME",
                    (true, true) => "TAP TO RETRY",
                    (true, false) => "SPACE TO RETRY",
                },
                action_y + 11.0 * unit,
                1.4 * unit,
                INK,
            );
        }
    }
}

const fn glyph(character: char) -> [u8; 7] {
    match character {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '!' => [4, 4, 4, 4, 4, 0, 4],
        '?' => [14, 17, 1, 2, 4, 0, 4],
        '.' => [0, 0, 0, 0, 0, 0, 4],
        ',' => [0, 0, 0, 0, 0, 4, 8],
        '←' => [0, 4, 8, 31, 8, 4, 0],
        '→' => [0, 4, 2, 31, 2, 4, 0],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        '▶' => [16, 24, 28, 30, 28, 24, 16],
        '↑' => [4, 14, 21, 4, 4, 4, 4],
        '↓' => [4, 4, 4, 4, 21, 14, 4],
        // The speaker icon is a speaker glyph followed by sound waves or a cross
        '🔈' => [2, 6, 30, 30, 30, 6, 2],
        '🔉' => [2, 9, 5, 21, 5, 9, 2],
        '🔇' => [0, 17, 10, 4, 10, 17, 0],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backward_hops_stay_visible_in_mobile_landscape() {
        let mut game = Game::new(0, 42);
        game.camera = 4.0;
        game.score = 4;
        for (width, height) in [(900.0, 390.0), (640.0, 320.0), (390.0, 900.0)] {
            let scene = Scene::new(
                width,
                height,
                &game,
                bwindow::LogicalInsets {
                    top: 32.0,
                    bottom: 24.0,
                    ..Default::default()
                },
                true,
            );
            assert!(scene.project([game.x, game.row as f32 - 0.4, 0.0])[1] > -0.9);
        }
    }

    #[test]
    fn mobile_zoom_keeps_the_frog_visible_at_both_edges() {
        let mut game = Game::new(0, 42);
        for (width, height) in [(320.0, 480.0), (390.0, 900.0), (900.0, 390.0)] {
            let desktop = Scene::new(width, height, &game, Default::default(), false);
            for x in [-6.0, 0.0, 6.0] {
                game.x = x;
                let mobile = Scene::new(width, height, &game, Default::default(), true);
                assert!(mobile.scale >= desktop.scale * 1.4);
                assert!(mobile.project([x, 0.0, 0.0])[0].abs() < 0.9);
            }
        }
    }

    #[test]
    fn traffic_generation_boundaries_are_offscreen() {
        let game = Game::new(0, 42);
        let lane = Lane::new(100, game.seed);
        for (width, height, mobile) in [
            (640.0, 820.0, false),
            (4000.0, 320.0, false),
            (320.0, 4000.0, false),
            (390.0, 900.0, true),
        ] {
            let mut scene = Scene::new(width, height, &game, Default::default(), mobile);
            for row in [-5.0, 0.0, 20.0, 100.0] {
                let (left, right) = scene.visible_bounds(row, 2.0);
                for (x, side) in [(left, -1.0), (right, 1.0)] {
                    scene.vertices.clear();
                    scene.vehicle(x, row, &lane);
                    assert!(
                        scene
                            .vertices
                            .as_chunks::<7>()
                            .0
                            .iter()
                            .all(|v| v[0] * side > 1.0)
                    );
                }
                for length in [3.4, 4.3, 5.1] {
                    let (left, right) = scene.visible_bounds(row, length / 2.0 + 0.3);
                    for (x, side) in [(left, -1.0), (right, 1.0)] {
                        scene.vertices.clear();
                        scene.shadow([x, row, -0.015], [length, 0.76], 0.0);
                        scene.block([x, row, 0.02], [length, 0.76, 0.19], 0xb48250);
                        assert!(
                            scene
                                .vertices
                                .as_chunks::<7>()
                                .0
                                .iter()
                                .all(|v| v[0] * side > 1.0)
                        );
                    }
                }
                let (left, right) = scene.visible_bounds(row, 5.5);
                for (x, side) in [(left, -1.0), (right, 1.0)] {
                    for dx in [-5.1, 5.1] {
                        for dr in [-0.5, 0.5] {
                            assert!(scene.project([x + dx, row + dr, 1.2])[0] * side > 1.0);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn resized_scenes_keep_visible_geometry_inside_the_depth_range() {
        let game = Game::new(0, 42);
        for (width, height) in [(640.0, 820.0), (320.0, 4000.0), (4000.0, 320.0)] {
            let scene = Scene::new(width, height, &game, Default::default(), false);
            for vertex in scene.vertices.as_chunks::<7>().0 {
                assert!(vertex.iter().all(|value| value.is_finite()));
                if vertex[0].abs() <= 1.0 && vertex[1].abs() <= 1.0 {
                    assert!(
                        (0.0..=1.0).contains(&vertex[2]),
                        "visible vertex clipped at {width}x{height}: {vertex:?}"
                    );
                }
            }
            // Every sampled point needs opaque geometry behind it, even at the edges.
            for x in [-0.99, 0.0, 0.99] {
                for y in [-0.99, 0.0, 0.99] {
                    assert!(
                        scene.vertices.as_chunks::<21>().0.iter().any(|triangle| {
                            let points = [
                                [triangle[0], triangle[1]],
                                [triangle[7], triangle[8]],
                                [triangle[14], triangle[15]],
                            ];
                            let edges = std::array::from_fn::<_, 3, _>(|i| {
                                let [ax, ay] = points[i];
                                let [bx, by] = points[(i + 1) % 3];
                                (bx - ax) * (y - ay) - (by - ay) * (x - ax)
                            });
                            edges.iter().all(|edge| *edge > 0.0)
                                || edges.iter().all(|edge| *edge < 0.0)
                        }),
                        "uncovered point ({x}, {y}) at {width}x{height}"
                    );
                }
            }
        }
    }
}
