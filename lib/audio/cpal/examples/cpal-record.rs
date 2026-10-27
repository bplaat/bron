/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Records the default input device to a 16-bit WAV file until Ctrl+C is pressed

use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::sync::mpsc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn write_header(
    file: &mut impl Write,
    channels: u16,
    sample_rate: u32,
    data_size: u32,
) -> io::Result<()> {
    let block_align = channels * size_of::<i16>() as u16;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_size).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&(sample_rate * block_align as u32).to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "recording.wav".to_string());

    let device = cpal::default_host()
        .default_input_device()
        .expect("No input device");
    let config = device
        .default_input_config()
        .expect("Can't read input config")
        .config();
    let (sender, receiver) = mpsc::channel::<Vec<f32>>();
    let stream = device
        .build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| _ = sender.send(data.to_vec()),
            |err| eprintln!("Stream error: {err}"),
            None,
        )
        .expect("Can't build input stream");
    stream.play().expect("Can't start input stream");

    let mut file = File::create(&path).expect("Can't create file");
    write_header(&mut file, config.channels, config.sample_rate, 0).expect("Can't write file");
    println!(
        "Recording {} channel(s) at {} Hz to {path}, press Ctrl+C to stop",
        config.channels, config.sample_rate
    );

    // The header is updated after every buffer, so the file stays valid when Ctrl+C ends the process
    let mut data_size = 0;
    for samples in receiver {
        let bytes = samples
            .iter()
            .flat_map(|sample| ((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())
            .collect::<Vec<_>>();
        data_size += bytes.len() as u32;
        file.write_all(&bytes)
            .and_then(|()| file.seek(SeekFrom::Start(0)))
            .and_then(|_| write_header(&mut file, config.channels, config.sample_rate, data_size))
            .and_then(|()| file.seek(SeekFrom::End(0)))
            .expect("Can't write file");
    }
}
