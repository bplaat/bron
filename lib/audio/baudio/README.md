# baudio

A small game audio mixer on top of [cpal](../cpal). It plays many sounds at once with their own
volume, stereo position and pitch, and renders [sfxr](../sfxr) sound effects.

## Features

- `Audio` opens the default output device, the mixer runs in its audio callback
- `Sound` holds shared mono samples, `Sound::from_sfxr` renders a sfxr sample once
- `Play` sets the volume, pan, pitch and looping of a played sound
- `Voice` changes or stops a playing sound, with short ramps that avoid clicks
- At most 32 voices play at once, the oldest voice makes room for a new one
- A soft limiter keeps many loud sounds together from clipping

The audio callback only receives commands over a channel, it never waits for the game. Sounds are
resampled to the device with linear interpolation, so they can have any sample rate.

## Example

```rs
use baudio::{Audio, Play, Sound};

let audio = Audio::new()?;
let jump = Sound::from_sfxr(&sfxr::Sample::jump(Some(7)));
audio.play(&jump);
let voice = audio.play_with(
    &jump,
    Play {
        volume: 0.5,
        pan: -0.8,
        pitch: 1.2,
        ..Play::default()
    },
);
voice.stop();
```

Run the example that plays all sfxr presets over each other:

```sh
cargo run -p baudio --example baudio-sfxr
```

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
