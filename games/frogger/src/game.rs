// Copyright (c) 2026 Bastiaan van der Plaat
// SPDX-License-Identifier: MIT

const HOP: f32 = 0.16;
pub(super) const HALF_WIDTH: i32 = 6;
const PERIOD: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Terrain {
    Grass,
    Road,
    Water,
    Rail,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum State {
    Ready,
    Playing,
    Paused,
    Dying,
    Dead,
}

/// A sound the game asks to play, the pans go from -1.0 left to 1.0 right of the frog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Cue {
    Start,
    Pause,
    Resume,
    Hop,
    /// Landed on a log
    Log,
    /// The crossing bell of a rail lane while its lights flash
    Bell {
        pan: f32,
        volume: f32,
    },
    /// A train enters a nearby crossing
    Horn {
        pan: f32,
        volume: f32,
    },
    /// A countdown second, with the seconds left
    Tick(i32),
    /// The previous best score was beaten
    Best,
    Squash,
    Crash,
    Splash,
    Timeout,
    GameOver,
}

pub(super) struct Lane {
    pub row: i32,
    pub terrain: Terrain,
    pub speed: f32,
    pub length: f32,
    pub phase: f32,
}

pub(super) const fn hash(mut value: u32) -> u32 {
    value = value.wrapping_mul(747796405).wrapping_add(2891336453);
    value = ((value >> ((value >> 28) + 4)) ^ value).wrapping_mul(277803737);
    (value >> 22) ^ value
}

impl Lane {
    pub(super) fn new(row: i32, seed: u32) -> Self {
        let terrain = Self::terrain(row, seed);
        let value = hash((row as u32).wrapping_add(seed));
        let flow_row = if terrain == Terrain::Water {
            4 + (row - 4) / 8 * 8
        } else {
            row
        };
        let flow = hash((flow_row as u32).wrapping_add(seed));
        let difficulty = (flow_row.max(0) as f32 * 0.015).min(1.0);
        let direction = if flow & 8 == 0 { -1.0 } else { 1.0 };
        Self {
            row,
            terrain,
            speed: direction * (0.85 + (flow % 7) as f32 * 0.10 + difficulty * 1.25),
            length: if terrain == Terrain::Water {
                3.4 + ((value >> 8) % 4) as f32 * 0.3
            } else {
                if row < 18 || value & 16 == 0 {
                    1.5
                } else {
                    2.8
                }
            },
            // Independent row offsets and lengths break up formations without widening gaps.
            phase: (flow % 240) as f32 * 0.1
                + if terrain == Terrain::Water {
                    ((row - flow_row) % 3 - 1) as f32 * 0.9 + ((value >> 16) % 5) as f32 * 0.1 - 0.2
                } else {
                    0.0
                },
        }
    }

    fn terrain(row: i32, seed: u32) -> Terrain {
        if row < 4 {
            return Terrain::Grass;
        }
        // Each section groups one crossing between generous, continuous grass banks.
        // Its seed is independent of generation order, including when walking back.
        let section = (row - 4) / 8;
        let offset = (row - 4) % 8;
        let value = hash(seed.wrapping_add(section as u32).wrapping_add(0x9e3779b9));
        let stage = (section * 3 + 1) / 2;
        let terrain = if stage < 3 {
            Terrain::Road
        } else if section == 5 || stage > 8 && value.is_multiple_of(10) {
            Terrain::Rail
        } else if section == 2 || value % 10 < 3 {
            Terrain::Water
        } else {
            Terrain::Road
        };
        let start = 1 + (value % 2) as i32;
        let width = match terrain {
            Terrain::Rail => 1,
            Terrain::Water => (1 + stage / 4).min(3),
            _ => (1 + stage / 3 + ((value >> 8) % 2) as i32).min(5),
        };
        if offset >= start && offset < start + width {
            terrain
        } else {
            Terrain::Grass
        }
    }

    fn period(&self) -> f32 {
        match self.terrain {
            Terrain::Road => PERIOD + 8.0 * (1.0 - (self.row.max(0) as f32 * 0.015).min(1.0)),
            _ => PERIOD,
        }
    }

    pub(super) fn objects(&self, time: f32) -> [f32; 3] {
        let period = self.period();
        std::array::from_fn(|index| {
            (self.phase + index as f32 * period / 3.0 + time * self.speed + period / 2.0)
                .rem_euclid(period)
                - period / 2.0
        })
    }

    pub(super) fn objects_in(&self, time: f32, left: f32, right: f32) -> impl Iterator<Item = f32> {
        Self::repeated_positions(self.objects(time)[0], self.period() / 3.0, left, right)
    }

    fn repeated_positions(
        origin: f32,
        spacing: f32,
        left: f32,
        right: f32,
    ) -> impl Iterator<Item = f32> {
        let first = ((left - origin) / spacing).ceil() as i32;
        let last = ((right - origin) / spacing).floor() as i32;
        (first..=last).map(move |index| origin + index as f32 * spacing)
    }

    pub(super) fn train_phase(&self, time: f32) -> f32 {
        (time + self.phase).rem_euclid(8.0)
    }

    pub(super) fn train(&self, time: f32) -> Option<f32> {
        let phase = self.train_phase(time);
        (phase >= 5.0).then_some((-24.0 + (phase - 5.0) * 20.0) * self.speed.signum())
    }

    pub(super) fn trains_in(&self, time: f32, left: f32, right: f32) -> impl Iterator<Item = f32> {
        // Continue each scheduled train beyond the crossing until it leaves the view.
        let origin = (-24.0 + (self.train_phase(time) - 5.0) * 20.0) * self.speed.signum();
        Self::repeated_positions(origin, 8.0 * 20.0, left, right)
    }

    pub(super) fn supports(&self, x: f32, time: f32) -> bool {
        self.objects(time)
            .iter()
            .any(|center| (x - center).abs() <= self.length / 2.0 - 0.15)
    }

    pub(super) fn tree(&self, x: i32, seed: u32) -> bool {
        self.terrain == Terrain::Grass
            && self.row >= 4
            && x != 0
            && hash(
                seed.wrapping_add(self.row as u32)
                    .wrapping_mul(31)
                    .wrapping_add(x as u32),
            )
            .is_multiple_of(if self.row < 22 { 13 } else { 7 })
    }
}

pub(super) struct Game {
    pub state: State,
    pub x: f32,
    pub row: i32,
    pub score: i32,
    pub best: i32,
    pub time: f32,
    pub camera: f32,
    pub idle: f32,
    pub seed: u32,
    pub facing: (i32, i32),
    pub death_time: f32,
    pub reason: &'static str,
    /// Sounds to play, drained by the caller
    pub cues: Vec<Cue>,
    /// Whether the sound effects are muted, kept between games
    pub muted: bool,
    /// Whether the best score of earlier games was beaten this game
    celebrated: bool,
    from: (f32, f32),
    hop: f32,
    pending: Option<(i32, i32)>,
}

impl Game {
    pub(super) const fn new(best: i32, seed: u32) -> Self {
        Self {
            state: State::Ready,
            x: 0.0,
            row: 0,
            score: 0,
            best,
            time: 0.0,
            camera: 0.0,
            idle: 0.0,
            seed,
            reason: "",
            cues: Vec::new(),
            muted: false,
            celebrated: false,
            death_time: 0.0,
            facing: (0, 1),
            from: (0.0, 0.0),
            hop: 0.0,
            pending: None,
        }
    }

    pub(super) fn start(&mut self) {
        if self.state == State::Ready {
            self.state = State::Playing;
            self.cues.push(Cue::Start);
        }
    }

    pub(super) fn restart(&mut self) {
        let (cues, muted) = (std::mem::take(&mut self.cues), self.muted);
        *self = Self::new(self.best, hash(self.seed));
        self.cues = cues;
        self.muted = muted;
        self.state = State::Playing;
        self.cues.push(Cue::Start);
    }

    pub(super) fn toggle_pause(&mut self) {
        let (state, cue) = match self.state {
            State::Playing => (State::Paused, Cue::Pause),
            State::Paused => (State::Playing, Cue::Resume),
            _ => return,
        };
        self.state = state;
        self.cues.push(cue);
    }

    pub(super) fn move_by(&mut self, dx: i32, dy: i32) {
        if self.state != State::Playing {
            return;
        }
        if self.hop > 0.0 {
            self.pending = Some((dx, dy));
            return;
        }
        let x = self.x.round() + dx as f32;
        let row = self.row + dy;
        if x.abs() > HALF_WIDTH as f32
            || row < 0
            || (row as f32) < self.camera - 4.0
            || Lane::new(row, self.seed).tree(x as i32, self.seed)
        {
            return;
        }
        self.facing = (dx, dy);
        self.cues.push(Cue::Hop);
        self.from = (self.x, self.row as f32);
        self.x = x;
        self.row = row;
        self.hop = HOP;
    }

    pub(super) fn position(&self) -> (f32, f32, f32) {
        let t = 1.0 - self.hop / HOP;
        let smooth = t * t * (3.0 - 2.0 * t);
        (
            self.from.0 + (self.x - self.from.0) * smooth,
            self.from.1 + (self.row as f32 - self.from.1) * smooth,
            (t * std::f32::consts::PI).sin() * 0.5,
        )
    }

    fn die(&mut self, reason: &'static str, cue: Cue) {
        self.cues.push(cue);
        self.reason = reason;
        self.state = State::Dying;
        self.death_time = 0.0;
        self.pending = None;
    }

    pub(super) fn update(&mut self, dt: f32) {
        if self.state == State::Dying {
            let dt = dt.clamp(0.0, 0.25);
            self.time += dt;
            self.death_time = (self.death_time + dt).min(0.75);
            if self.death_time >= 0.75 {
                self.state = State::Dead;
                self.cues.push(Cue::GameOver);
            }
            return;
        }
        if self.state != State::Playing {
            return;
        }
        // Small physics steps avoid tunneling through traffic after slow frames.
        let mut remaining = dt.min(0.25);
        while remaining > 0.0 && self.state == State::Playing {
            let step = remaining.min(1.0 / 120.0);
            remaining -= step;
            self.step(step);
        }
    }

    fn step(&mut self, dt: f32) {
        self.time += dt;
        self.idle += dt;
        self.camera += (self.score as f32 - self.camera) * (dt * 5.0).min(1.0);
        let hopping = self.hop > 0.0;
        self.hop = (self.hop - dt).max(0.0);
        let lane = Lane::new(self.row, self.seed);
        if hopping
            && self.hop == 0.0
            && lane.terrain == Terrain::Water
            && lane.supports(self.x, self.time)
        {
            self.cues.push(Cue::Log);
        }
        self.crossing_cues(dt);
        let left = 10.0 - self.idle;
        if (5.0..10.0).contains(&self.idle) && left.ceil() < (left + dt).ceil() {
            self.cues.push(Cue::Tick(left.ceil() as i32));
        }
        if !hopping && lane.terrain == Terrain::Water {
            self.x += lane.speed * dt;
            self.from.0 = self.x;
        }
        // Road collisions use the visible position, including while hopping.
        let (x, row, _) = self.position();
        let road = Lane::new(row.round() as i32, self.seed);
        if road.terrain == Terrain::Road
            && road
                .objects(self.time)
                .iter()
                .any(|center| (center - x).abs() < road.length / 2.0 + 0.23)
        {
            self.die("HIT BY TRAFFIC", Cue::Squash);
        } else if road.terrain == Terrain::Rail
            && road
                .train(self.time)
                .is_some_and(|center| (center - x).abs() < 5.0 + 0.23)
        {
            self.die("HIT BY A TRAIN", Cue::Crash);
        } else if self.hop == 0.0
            && lane.terrain == Terrain::Water
            && !lane.supports(self.x, self.time)
        {
            self.die("FELL IN THE WATER", Cue::Splash);
        } else if self.x.abs() > HALF_WIDTH as f32 + 0.5 {
            self.die("SWEPT AWAY", Cue::Splash);
        } else if self.idle >= 10.0 {
            self.die("TIME RAN OUT", Cue::Timeout);
        }
        if self.hop == 0.0 && self.state == State::Playing {
            if self.row > self.score {
                if self.row > self.best && self.best > 0 && !self.celebrated {
                    self.celebrated = true;
                    self.cues.push(Cue::Best);
                }
                self.score = self.row;
                self.best = self.best.max(self.score);
                self.idle = 0.0;
            }
            if let Some((dx, dy)) = self.pending.take() {
                self.move_by(dx, dy);
            }
        }
    }
}

impl Game {
    /// Rings the bells of nearby crossings while their lights flash and sounds the horn of an
    /// arriving train, both from the side the train comes from.
    fn crossing_cues(&mut self, dt: f32) {
        for row in self.row - 2..=self.row + 6 {
            let lane = Lane::new(row, self.seed);
            if lane.terrain != Terrain::Rail {
                continue;
            }
            let before = lane.train_phase(self.time - dt);
            let after = lane.train_phase(self.time);
            let pan = -lane.speed.signum() * 0.7;
            let volume = 1.0 / (1.0 + (row - self.row).abs() as f32 * 0.4);
            if (3.5..5.0).contains(&after) && (after * 2.0).floor() > (before * 2.0).floor() {
                self.cues.push(Cue::Bell { pan, volume });
            }
            if before < 5.0 && after >= 5.0 {
                self.cues.push(Cue::Horn { pan, volume });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_traffic_stays_continuous_across_wraps() {
        for seed in 0..20 {
            for row in 4..100 {
                let lane = Lane::new(row, seed);
                if !matches!(lane.terrain, Terrain::Road | Terrain::Water | Terrain::Rail) {
                    continue;
                }
                for time in [0.0, 5.0, 7.999, 12.0, 31.999, 100.0] {
                    let before: Vec<_> = if lane.terrain == Terrain::Rail {
                        lane.trains_in(time, -200.0, 200.0).collect()
                    } else {
                        lane.objects_in(time, -200.0, 200.0).collect()
                    };
                    let after: Vec<_> = if lane.terrain == Terrain::Rail {
                        lane.trains_in(time + 0.01, -201.0, 201.0).collect()
                    } else {
                        lane.objects_in(time + 0.01, -201.0, 201.0).collect()
                    };
                    let speed = if lane.terrain == Terrain::Rail {
                        lane.speed.signum() * 20.0
                    } else {
                        lane.speed
                    };
                    for x in before {
                        assert!(
                            after
                                .iter()
                                .any(|next| (next - x - speed * 0.01).abs() < 0.001)
                        );
                    }
                    // Rendering and collision detection must agree inside the playable area.
                    if lane.terrain == Terrain::Rail {
                        if let Some(x) = lane.train(time) {
                            assert!(lane.trains_in(time, x - 0.01, x + 0.01).next().is_some());
                        }
                    } else {
                        for x in lane.objects(time) {
                            assert!(lane.objects_in(time, x - 0.01, x + 0.01).next().is_some());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn safe_start_and_regular_banks() {
        for seed in 0..100 {
            let mut consecutive = 0;
            for row in 0..200 {
                let lane = Lane::new(row, seed);
                if row < 4 {
                    assert_eq!(lane.terrain, Terrain::Grass);
                    for x in -HALF_WIDTH..=HALF_WIDTH {
                        assert!(!lane.tree(x, seed));
                    }
                }
                if row < 20 {
                    assert!(!matches!(lane.terrain, Terrain::Water | Terrain::Rail));
                }
                if row < 44 {
                    assert_ne!(lane.terrain, Terrain::Rail);
                }
                if lane.terrain == Terrain::Grass {
                    consecutive = 0;
                } else {
                    consecutive += 1;
                }
                assert!(
                    consecutive <= 5,
                    "too many hazards at row {row}, seed {seed}"
                );
                assert!(!lane.tree(0, seed));
                if row > 0 {
                    let previous = Lane::new(row - 1, seed).terrain;
                    assert!(
                        previous == Terrain::Grass
                            || lane.terrain == Terrain::Grass
                            || previous == lane.terrain
                    );
                }
            }
        }
    }

    #[test]
    fn crossings_get_busier_and_stay_deterministic() {
        for seed in 0..100 {
            let hazards = |range: std::ops::Range<i32>| {
                range
                    .filter(|&r| Lane::new(r, seed).terrain != Terrain::Grass)
                    .count()
            };
            assert!(
                hazards(4..20) < hazards(100..116),
                "difficulty did not increase for seed {seed}"
            );
            assert!((20..28).any(|r| Lane::new(r, seed).terrain == Terrain::Water));
            assert!((44..52).any(|r| Lane::new(r, seed).terrain == Terrain::Rail));
            for row in (0..160).rev() {
                let first = Lane::new(row, seed);
                let second = Lane::new(row, seed);
                assert_eq!(first.terrain, second.terrain);
                assert_eq!(first.objects(15.0), second.objects(15.0));
            }
        }
    }

    #[test]
    fn multi_row_rivers_have_playable_routes() {
        let mut widths = [false; 4];
        for seed in 0..100 {
            for row in 20..200 {
                if Lane::new(row, seed).terrain != Terrain::Water
                    || Lane::new(row - 1, seed).terrain == Terrain::Water
                {
                    continue;
                }
                let width = (row..)
                    .take_while(|&r| Lane::new(r, seed).terrain == Terrain::Water)
                    .count();
                if width < 2 {
                    continue;
                }
                widths[width] = true;
                for time in [0.0, 5.0, 12.0, 31.999, 100.0] {
                    let crossed = (0..=8).any(|wait| {
                        (-HALF_WIDTH..=HALF_WIDTH).any(|x| {
                            if Lane::new(row - 1, seed).tree(x, seed) {
                                return false;
                            }
                            let mut game = Game::new(0, seed);
                            game.state = State::Playing;
                            game.row = row - 1;
                            game.score = row - 1;
                            game.camera = (row - 1) as f32;
                            game.x = x as f32;
                            game.from = (game.x, game.row as f32);
                            game.time = time + wait as f32 * 0.5;
                            // Allow a short pause on each log before the next hop.
                            for _ in 0..=width {
                                game.move_by(0, 1);
                                game.update(HOP + 0.05);
                                if game.state != State::Playing {
                                    return false;
                                }
                            }
                            game.row == row + width as i32
                        })
                    });
                    assert!(
                        crossed,
                        "no river route at row {row}, seed {seed}, time {time}"
                    );
                }
            }
        }
        assert!(widths[2] && widths[3]);
    }

    #[test]
    fn score_only_counts_new_rows_and_pause_freezes() {
        let mut game = Game::new(8, 12);
        game.state = State::Playing;
        game.move_by(0, 1);
        game.update(0.2);
        assert_eq!(game.score, 1);
        game.move_by(0, -1);
        game.update(0.2);
        assert_eq!(game.score, 1);
        game.toggle_pause();
        let time = game.time;
        game.update(0.2);
        game.move_by(1, 0);
        assert_eq!(game.time, time);
        assert_eq!(game.x, 0.0);
        game.restart();
        assert_eq!(game.best, 8);
        assert_eq!(game.score, 0);
    }

    #[test]
    fn traffic_and_water_kill_but_logs_carry() {
        let mut game = Game::new(0, 42);
        game.state = State::Playing;
        let row = (2..100)
            .find(|&r| Lane::new(r, 42).terrain == Terrain::Road)
            .expect("road");
        let road = Lane::new(row, 42);
        game.row = row;
        game.x = road.objects(0.0)[1];
        game.from = (game.x, row as f32);
        game.update(0.01);
        assert!(game.state == State::Dying);
        game.restart();
        let row = (2..100)
            .find(|&r| Lane::new(r, game.seed).terrain == Terrain::Water)
            .expect("water");
        let water = Lane::new(row, game.seed);
        game.row = row;
        game.x = water
            .objects(0.0)
            .into_iter()
            .find(|x| x.abs() < 4.0)
            .expect("log");
        game.from = (game.x, row as f32);
        let x = game.x;
        game.update(0.1);
        assert!(game.state == State::Playing);
        assert!((game.x - x - water.speed * 0.1).abs() < 0.001);
        game.x += water.period() / 6.0;
        game.from.0 = game.x;
        game.update(0.01);
        assert!(game.state == State::Dying);
    }

    #[test]
    fn trains_warn_before_arriving_and_hit_during_hops() {
        let seed = 1234;
        let row = (44..52)
            .find(|&r| Lane::new(r, seed).terrain == Terrain::Rail)
            .expect("railway");
        let lane = Lane::new(row, seed);
        assert_eq!(lane.terrain, Terrain::Rail);
        let offset = 16.0 - lane.phase.rem_euclid(8.0);
        assert!(lane.train(offset + 4.0).is_none());
        assert!(lane.train_phase(offset + 4.0) >= 3.5);
        assert!(lane.train(offset + 6.2).expect("passing train").abs() < 0.001);
        let mut game = Game::new(0, seed);
        game.state = State::Playing;
        game.row = row;
        game.from = (0.0, row as f32);
        game.time = offset + 6.2;
        game.hop = HOP / 2.0;
        game.update(0.01);
        assert!(game.state == State::Dying);
        assert_eq!(game.reason, "HIT BY A TRAIN");
    }

    #[test]
    fn crossings_ring_before_the_horn_and_the_countdown_ticks() {
        let seed = 1234;
        let row = (44..52)
            .find(|&r| Lane::new(r, seed).terrain == Terrain::Rail)
            .expect("railway");
        let lane = Lane::new(row, seed);
        let mut game = Game::new(0, seed);
        game.state = State::Playing;
        game.row = row - 1;
        game.from = (0.0, (row - 1) as f32);
        game.time = 16.0 - lane.phase.rem_euclid(8.0) + 3.0;
        for _ in 0..200 {
            game.idle = 0.0;
            game.update(0.01);
        }
        let bells = game
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Bell { .. }))
            .count();
        assert_eq!(bells, 3);
        assert!(matches!(game.cues.last(), Some(Cue::Horn { volume, .. }) if *volume < 1.0));

        let mut game = Game::new(0, 42);
        game.state = State::Playing;
        for _ in 0..44 {
            game.update(0.25);
        }
        let ticks: Vec<_> = game
            .cues
            .iter()
            .filter(|cue| matches!(cue, Cue::Tick(_)))
            .collect();
        assert_eq!(
            ticks,
            [
                &Cue::Tick(5),
                &Cue::Tick(4),
                &Cue::Tick(3),
                &Cue::Tick(2),
                &Cue::Tick(1)
            ]
        );
        assert!(game.cues.ends_with(&[Cue::Timeout, Cue::GameOver]));
    }

    #[test]
    fn beating_the_best_score_is_celebrated_once() {
        let mut game = Game::new(1, 42);
        game.state = State::Playing;
        for _ in 0..3 {
            game.move_by(0, 1);
            game.update(0.2);
        }
        assert_eq!(game.score, 3);
        assert_eq!(game.cues.iter().filter(|cue| **cue == Cue::Best).count(), 1);
    }

    #[test]
    fn death_animation_finishes_before_retry() {
        let mut game = Game::new(16, 42);
        game.state = State::Playing;
        game.move_by(1, 0);
        game.die("FELL IN THE WATER", Cue::Splash);
        let position = game.position();
        game.move_by(0, 1);
        game.update(0.25);
        assert!(game.state == State::Dying);
        assert_eq!(game.position(), position);
        assert_eq!(game.death_time, 0.25);
        game.update(0.25);
        assert!(game.state == State::Dying);
        game.update(0.25);
        assert!(game.state == State::Dead);
        assert_eq!(game.cues, [Cue::Hop, Cue::Splash, Cue::GameOver]);
        game.restart();
        assert_eq!(game.cues.last(), Some(&Cue::Start));
        assert_eq!(game.death_time, 0.0);
        assert_eq!(game.best, 16);
    }

    #[test]
    fn frog_faces_moves_and_buffers_one_hop() {
        let mut game = Game::new(0, 42);
        game.state = State::Playing;
        game.move_by(1, 0);
        assert_eq!(game.facing, (1, 0));
        game.move_by(0, 1);
        game.update(0.18);
        assert_eq!(game.facing, (0, 1));
        game.update(0.18);
        assert_eq!(game.row, 1);
        assert_eq!(game.x, 1.0);
        assert_eq!(game.score, 1);
    }

    #[test]
    fn bounds_trees_and_idle_timeout() {
        let mut game = Game::new(0, 42);
        game.state = State::Playing;
        game.move_by(0, -1);
        assert_eq!(game.row, 0);
        game.x = HALF_WIDTH as f32;
        game.move_by(1, 0);
        assert_eq!(game.x, HALF_WIDTH as f32);
        let (row, x) = (2..100)
            .flat_map(|r| (-6..6).map(move |x| (r, x)))
            .find(|&(r, x)| Lane::new(r, game.seed).tree(x, game.seed))
            .expect("tree");
        game.row = row - 1;
        game.x = x as f32;
        game.move_by(0, 1);
        assert_eq!(game.row, row - 1);
        game.restart();
        for _ in 0..45 {
            game.update(0.25);
        }
        assert!(game.state == State::Dead);
    }
}
