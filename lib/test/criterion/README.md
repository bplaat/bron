# criterion

A minimal replacement for the [criterion](https://crates.io/crates/criterion) crate. It is a small,
std-only benchmark runner that provides the Criterion-style APIs used by the benchmarks in this
workspace and has no dependencies.

Supported APIs: `Criterion`, `BenchmarkGroup`, `BenchmarkId`, `Throughput`,
`Bencher::iter`, `iter_batched`, `iter_batched_ref`, `iter_custom`,
`criterion_group!` (including the `name = ...; config = ...; targets = ...;` form),
and `criterion_main!`. Results show the median and the 10th and 90th percentile
sample times.

Run a benchmark with Cargo:

```sh
cargo bench -p sha1 --bench sha1 --features disable-simd
cargo bench -p sha1 --bench sha1 --features disable-software
```

Optional arguments after `--`:

- A substring filters benchmark names, for example `-- digest/1048576`.
- `--sample-size 20` sets the number of timed samples (default: 15, minimum: 10).
- `--measurement-time 2` sets the total target measurement time in seconds
  (default: 3).
- `--warm-up-time 1` sets the minimum warm-up time in seconds (default: 0.1).
- `--test` runs every selected case once; `--list` prints case names.
- `--save-baseline PATH` writes median times as tab-separated data.
- `--baseline PATH` compares each median with a saved baseline.

Baseline comparisons compare medians; there are no plots, reports, or significance tests.

The SHA crates select SIMD at runtime: `disable-simd` benchmarks the software path and
`disable-software` the SIMD path. Enabling both restores runtime selection.
