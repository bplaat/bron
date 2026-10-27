# cpal

A minimal replacement for the [`cpal`](https://crates.io/crates/cpal) crate that provides the
audio input and output streams used in this workspace.

## Supported API

- Get the default host and its default input and output devices
- Read the default input config, this is always mono `f32` at the device sample rate
- Read the default output config, this is always stereo `f32` at the device sample rate
- Build input and output streams with `f32` samples for any channel count and sample rate, the
  platform converts the audio of the device
- Play, pause and drop a stream

Device enumeration, device names, other sample formats, and callback timestamps are not
implemented. The `timeout` of `build_input_stream` and `build_output_stream` is ignored, and the
buffer size is a hint that some backends don't follow.

## Platforms

| Platform | Output                                    | Input                | Additional requirements                                                                     |
| -------- | ----------------------------------------- | -------------------- | ------------------------------------------------------------------------------------------- |
| Windows  | WASAPI shared mode                        | WASAPI               | None                                                                                        |
| macOS    | CoreAudio AudioUnit default output        | CoreAudio AudioQueue | Input in bundled apps needs `NSMicrophoneUsageDescription` and an `audio-input` entitlement |
| iOS      | CoreAudio AudioUnit RemoteIO              | CoreAudio AudioQueue | Input needs `NSMicrophoneUsageDescription`                                                  |
| Android  | AAudio                                    | AAudio               | Input needs the `RECORD_AUDIO` permission                                                   |
| Linux    | PulseAudio, or ALSA when it's unavailable | Same as output       | The system libpulse or libasound library                                                    |

On macOS and iOS the error callback is never called, an input queue records silence when microphone
access is denied. On iOS a stream activates the shared audio session: output only streams use the
ambient category that mixes with other apps, input streams switch to the play and record category.

### Linux

PulseAudio is used through its asynchronous API, PipeWire desktops provide it with pipewire-pulse.
Its mainloop thread calls the stream callbacks. When no PulseAudio server is running, the ALSA
`default` device is used from a stream thread. Both libraries are loaded at runtime, so binaries
also start without them. Development headers are not required. Ubuntu / Debian:

```sh
sudo apt install libpulse0 libasound2
```

## Example

```rs
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn play_tone() -> Result<(), cpal::Error> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(cpal::ErrorKind::DeviceNotAvailable)?;
    let config = device.default_output_config()?.config();
    let channels = config.channels as usize;
    let step = 440.0 * std::f32::consts::TAU / config.sample_rate as f32;
    let mut phase = 0.0f32;
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for frame in data.chunks_mut(channels) {
                frame.fill(phase.sin() * 0.2);
                phase = (phase + step) % std::f32::consts::TAU;
            }
        },
        |err| eprintln!("{err}"),
        None,
    )?;
    stream.play()?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    Ok(())
}
```

See the [examples](examples) for recording the microphone to a WAV file and playing a WAV file.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
