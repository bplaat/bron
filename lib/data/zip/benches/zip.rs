/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]

use std::hint::black_box;
use std::io::Cursor;

use crc::{CRC_32_ISO_HDLC, Crc};
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use zip::ZipArchive;

static CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

fn archive(data: &[u8], compressed: bool) -> Vec<u8> {
    let name = b"payload.bin";
    let payload = if compressed {
        miniz_oxide::deflate::compress_to_vec(data, 6)
    } else {
        data.to_vec()
    };
    let method = if compressed { 8u16 } else { 0 };
    let checksum = CRC32.checksum(data);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PK\x03\x04");
    bytes.extend_from_slice(&20u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&method.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&checksum.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&payload);

    let directory_offset = bytes.len() as u32;
    bytes.extend_from_slice(b"PK\x01\x02");
    bytes.extend_from_slice(&20u16.to_le_bytes());
    bytes.extend_from_slice(&20u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&method.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&checksum.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(name);

    let directory_size = bytes.len() as u32 - directory_offset;
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&directory_size.to_le_bytes());
    bytes.extend_from_slice(&directory_offset.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes
}

fn open_and_extract(c: &mut Criterion) {
    let cases = [1024, 1024 * 1024]
        .into_iter()
        .flat_map(|size| {
            let data = (0..size).map(|i| (i * 37) as u8).collect::<Vec<_>>();
            [("stored", false), ("deflated", true)]
                .into_iter()
                .map(move |(name, compressed)| (name, size, archive(&data, compressed)))
        })
        .collect::<Vec<_>>();

    let mut open = c.benchmark_group("zip_open");
    for (name, size, bytes) in &cases {
        let mut check = ZipArchive::new(Cursor::new(bytes)).expect("valid ZIP archive");
        assert_eq!(
            check.by_index(0).expect("valid ZIP entry").name(),
            "payload.bin"
        );
        open.throughput(Throughput::Bytes(bytes.len() as u64));
        open.bench_with_input(BenchmarkId::new(*name, size), bytes, |b, bytes| {
            b.iter(|| {
                black_box(
                    ZipArchive::new(Cursor::new(black_box(bytes))).expect("valid ZIP archive"),
                )
            });
        });
    }
    open.finish();

    let mut extract = c.benchmark_group("zip_extract");
    for (name, size, bytes) in &cases {
        extract.throughput(Throughput::Bytes(*size as u64));
        extract.bench_with_input(BenchmarkId::new(*name, size), bytes, |b, bytes| {
            b.iter_batched_ref(
                || ZipArchive::new(Cursor::new(black_box(bytes))).expect("valid ZIP archive"),
                |archive| black_box(archive.by_index(0).expect("valid ZIP entry")),
                BatchSize::LargeInput,
            );
        });
    }
    extract.finish();
}

criterion_group!(benches, open_and_extract);
criterion_main!(benches);
