/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Plays a PCM or float WAV file on the default output device

use std::sync::mpsc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

struct Wav {
    channels: usize,
    sample_rate: u32,
    /// Interleaved samples
    samples: Vec<f32>,
}

fn parse_wav(bytes: &[u8]) -> Result<Wav, String> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("Not a WAV file".to_string());
    }
    let mut format = None;
    let mut data = None;
    let mut offset = 12;
    while let Some(header) = bytes.get(offset..offset + 8) {
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let body = bytes
            .get(offset + 8..offset + 8 + size)
            .ok_or("Truncated WAV chunk")?;
        match &header[..4] {
            b"fmt " => format = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        // Chunks are padded to an even size
        offset += 8 + size + size % 2;
    }
    let format = format
        .filter(|format| format.len() >= 16)
        .ok_or("Missing fmt chunk")?;
    let data = data.ok_or("Missing data chunk")?;

    let u16_at = |index: usize| u16::from_le_bytes([format[index], format[index + 1]]);
    let channels = u16_at(2) as usize;
    let sample_rate = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
    let bits = u16_at(14);
    // An extensible format stores the real format tag at the start of its sub format GUID
    let tag = match u16_at(0) {
        0xfffe if format.len() >= 26 => u16_at(24),
        tag => tag,
    };
    let samples = match (tag, bits) {
        (1, 8) => data.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect(),
        (1, 16) => data
            .as_chunks()
            .0
            .iter()
            .map(|&b| i16::from_le_bytes(b) as f32 / 32_768.0)
            .collect(),
        (1, 24) => data
            .as_chunks()
            .0
            .iter()
            .map(|&[b0, b1, b2]| i32::from_le_bytes([0, b0, b1, b2]) as f32 / 2_147_483_648.0)
            .collect(),
        (1, 32) => data
            .as_chunks()
            .0
            .iter()
            .map(|&b| i32::from_le_bytes(b) as f32 / 2_147_483_648.0)
            .collect(),
        (3, 32) => data
            .as_chunks()
            .0
            .iter()
            .map(|&b| f32::from_le_bytes(b))
            .collect(),
        _ => return Err(format!("Unsupported WAV format {tag} with {bits} bits")),
    };
    if channels == 0 || sample_rate == 0 {
        return Err("Invalid WAV format".to_string());
    }
    Ok(Wav {
        channels,
        sample_rate,
        samples,
    })
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("Usage: cpal-play <file.wav>");
    let wav = parse_wav(&std::fs::read(&path).expect("Can't read file")).expect("Can't parse file");

    // The device converts the sample rate, the channels are mapped here
    let device = cpal::default_host()
        .default_output_device()
        .expect("No output device");
    let config = cpal::StreamConfig {
        sample_rate: wav.sample_rate,
        ..device
            .default_output_config()
            .expect("Can't read output config")
            .config()
    };
    let channels = config.channels as usize;

    let frames = wav.samples.len() / wav.channels;
    println!(
        "Playing {path}: {} channel(s) at {} Hz, {:.1} seconds",
        wav.channels,
        wav.sample_rate,
        frames as f32 / wav.sample_rate as f32
    );
    let (done_sender, done_receiver) = mpsc::sync_channel(1);
    let error_sender = done_sender.clone();
    let mut frame_index = 0;
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let source = wav
                        .samples
                        .get(frame_index * wav.channels..(frame_index + 1) * wav.channels);
                    for (channel, sample) in frame.iter_mut().enumerate() {
                        *sample = source.map_or(0.0, |source| source[channel % wav.channels]);
                    }
                    frame_index += 1;
                }
                if frame_index >= frames {
                    _ = done_sender.try_send(());
                }
            },
            move |err| {
                eprintln!("Stream error: {err}");
                _ = error_sender.try_send(());
            },
            None,
        )
        .expect("Can't build output stream");
    stream.play().expect("Can't start output stream");

    _ = done_receiver.recv();
    // Let the device play its last buffers
    std::thread::sleep(Duration::from_millis(200));
}
