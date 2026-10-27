/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Plays random sfxr presets over each other, then bends a looping sound

use std::thread::sleep;
use std::time::Duration;

use baudio::{Audio, Play, Sound};
use sfxr::Sample;

/// A sfxr preset constructor
type Preset = fn(Option<u64>) -> Sample;

fn main() {
    let audio = Audio::new().expect("Can't open audio output");

    // Each preset plays from another side, the sounds overlap
    let presets: [(&str, Preset); 7] = [
        ("pickup", Sample::pickup),
        ("laser", Sample::laser),
        ("explosion", Sample::explosion),
        ("powerup", Sample::powerup),
        ("hit", Sample::hit),
        ("jump", Sample::jump),
        ("blip", Sample::blip),
    ];
    for (index, (name, preset)) in presets.iter().enumerate() {
        let pan = index as f32 / (presets.len() - 1) as f32 * 2.0 - 1.0;
        println!("{name} at pan {pan:.1}");
        audio.play_with(
            &Sound::from_sfxr(&preset(None)),
            Play {
                pan,
                ..Play::default()
            },
        );
        sleep(Duration::from_millis(250));
    }
    sleep(Duration::from_millis(500));

    println!("looping siren");
    let mut siren = Sample::new();
    siren.wave_type = sfxr::WaveType::Triangle;
    siren.env_attack = 0.0;
    siren.env_sustain = 0.3;
    siren.env_decay = 0.0;
    let voice = audio.play_with(
        &Sound::from_sfxr(&siren),
        Play {
            volume: 0.6,
            looped: true,
            ..Play::default()
        },
    );
    for step in 0..40 {
        let phase = step as f32 / 40.0 * std::f32::consts::TAU;
        voice.set_pitch(1.0 + phase.sin() * 0.3);
        voice.set_pan(phase.sin());
        sleep(Duration::from_millis(50));
    }
    voice.stop();
    sleep(Duration::from_millis(100));
}
