/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use baudio::{Audio, Sound};

pub(crate) struct Sounds {
    audio: Audio,
    pub(crate) start: Sound,
    pub(crate) bounce: Sound,
    pub(crate) game_over: Sound,
}

impl Sounds {
    pub(crate) fn new() -> Option<Self> {
        let audio = Audio::new().ok()?;
        // The bundled Qt cues are canonical 44-byte-header PCM WAVs, mono at 22,050 Hz.
        let wav = |bytes: &[u8]| {
            assert_eq!(&bytes[36..40], b"data");
            let samples = bytes[44..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|sample| f32::from(i16::from_le_bytes(*sample)) / 32768.0)
                .collect::<Vec<_>>();
            Sound::new(22_050, samples)
        };
        Some(Self {
            audio,
            start: wav(include_bytes!("../assets/start.wav")),
            bounce: wav(include_bytes!("../assets/border.wav")),
            game_over: wav(include_bytes!("../assets/gameover.wav")),
        })
    }

    pub(crate) fn play(&self, sound: &Sound) {
        self.audio.play(sound);
    }
}
