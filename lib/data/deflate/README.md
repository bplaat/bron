# Deflate

Compress and decompress raw Deflate and zlib streams. The compressor ranges from
quick greedy matching to an optimal parser in the style of libdeflate and Zopfli,
which finds the cheapest sequence of literals and matches for each block.

## Getting started

```rs
let data = b"hello hello hello";
let compressed = deflate::zlib_compress(data, deflate::Level::Default);
let restored = deflate::zlib_decompress(&compressed, data.len()).expect("valid zlib");
assert_eq!(restored, data);
```

Use `compress` and `decompress` for raw Deflate streams, as in gzip and ZIP files.

## Levels

| Level     | Method                                                                                   |
| --------- | ---------------------------------------------------------------------------------------- |
| `Fast`    | Greedy matching on hash chains of 4 byte sequences, with Huffman block splitting.        |
| `Default` | Like `Fast` with lazy matching and deeper chains.                                        |
| `Best`    | Binary tree match finding and optimal parsing with a refined cost model, then splitting. |
| `Crunch`  | Like `Best` with deeper match finding, more cost model passes, and repeated splitting.   |

`Fast` and `Default` also check the latest earlier occurrence of each 3 byte sequence,
which keeps short matches without walking chains full of them.

Total PNG sizes and single-threaded times when compressing the filtered scanlines of
20 images from this repository (screenshots, icons, pixel art, a photo and vector
art), compared with other compressors:

| Compressor            | Size   | Time   |
| --------------------- | ------ | ------ |
| miniz_oxide level 6   | 100.0% | 0.16 s |
| libdeflate level 6    | 99.3%  | 0.08 s |
| libdeflate level 12   | 90.0%  | 4.4 s  |
| Zopfli, 15 iterations | 89.2%  | 141 s  |
| `Fast`                | 101.0% | 0.11 s |
| `Default`             | 97.4%  | 0.16 s |
| `Best`                | 89.8%  | 4.8 s  |
| `Crunch`              | 89.1%  | 32 s   |

Compression processes the input in segments, so memory use stays bounded for large
inputs.

## Decompression

Decompression takes an output size limit and allocates fallibly, returning an error
instead of aborting when the limit is exceeded or memory runs out. A stream must use
all of its input, and zlib streams must have a correct Adler-32 checksum, except with
`zlib_decompress_unchecked` for formats that already checksum the stream.

## Tests and benchmarks

Run `cargo test -p deflate` for tests or `cargo bench -p deflate --bench deflate` for
benchmarks.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
