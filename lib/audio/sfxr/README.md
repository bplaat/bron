# sfxr

A minimal replacement for the [`sfxr`](https://crates.io/crates/sfxr) crate, a reimplementation of
DrPetter's sfxr sound effect generator. It generates retro game sound effects from a handful of
parameters, the same parameters as the [sfxr.me](https://sfxr.me/) web editor.

## Supported API

- `Sample` with all sound parameters and `Sample::new`
- Random presets `pickup`, `laser`, `explosion`, `powerup`, `hit`, `jump` and `blip`, and
  `mutate`, they take an optional seed
- `Generator` with `new`, `generate`, `reset` and its `sample` and `volume` fields
- `WaveType` with `Square`, `Triangle`, `Sine` and `Noise`

Without the default `std` feature the crate is `no_std` and only has `Sample`, `WaveType` and
`SAMPLE_RATE`, so `no_std` crates can describe their sounds and render them elsewhere.

Differences with the original crate:

- Without a seed the presets and `mutate` use a random seed from `getrandom`, the original crate
  uses seed zero
- `SAMPLE_RATE` and `Generator::finished` tell the sample rate and when a sound has ended
- `freq_dramp` and `pha_ramp` accept the documented `-1.0..=1.0` range, empty envelope stages
  don't produce a click

## Example

```rs
use sfxr::{Generator, Sample};

let mut generator = Generator::new(Sample::jump(Some(7)));
let mut buffer = vec![0.0; sfxr::SAMPLE_RATE as usize];
generator.generate(&mut buffer);
```

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
