/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

use crate::traits::{DeviceTrait, HostTrait, StreamTrait};

cfg_select! {
    target_vendor = "apple" => {
        use crate::apple as backend;
        mod apple;
    }
    target_os = "android" => {
        use crate::android as backend;
        mod android;
    }
    target_os = "linux" => {
        use crate::linux as backend;
        mod blocking;
        mod linux;
    }
    windows => {
        use crate::windows as backend;
        mod blocking;
        mod windows;
    }
    _ => {
        compile_error!("Unsupported platform");
    }
}

// MARK: Errors
/// General categories of errors.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The device is busy.
    DeviceBusy,
    /// The device is not available.
    DeviceNotAvailable,
    /// Invalid input or argument.
    InvalidInput,
    /// The stream configuration is not supported.
    UnsupportedConfig,
    /// The operation is not supported.
    UnsupportedOperation,
    /// A backend specific error.
    BackendError,
}

impl Display for ErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DeviceBusy => {
                "The requested device is temporarily busy. Another application or stream may be using it."
            }
            Self::DeviceNotAvailable => {
                "The requested audio device is not available. It may have been disconnected."
            }
            Self::InvalidInput => "Invalid input or argument.",
            Self::UnsupportedConfig => {
                "The requested stream configuration is not supported by the device."
            }
            Self::UnsupportedOperation => "The requested operation is not supported.",
            Self::BackendError => "The audio backend returned an unclassified error.",
        })
    }
}

/// Error type for all operations.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Error {
    kind: ErrorKind,
    message: Option<Cow<'static, str>>,
}

impl Error {
    /// Creates a new error with the given kind and no message.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            message: None,
        }
    }

    /// Creates a new error with the given kind and a human-readable message.
    pub fn with_message(kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            message: Some(message.into()),
        }
    }

    /// Returns the error kind.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Returns the human-readable message, if any.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(message) => f.write_str(message),
            None => self.kind.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self::new(kind)
    }
}

// MARK: Stream config
/// Number of channels.
pub type ChannelCount = u16;

/// Number of samples per second of a single channel.
pub type SampleRate = u32;

/// Number of frames in a buffer.
pub type FrameCount = u32;

/// Format of the samples in a buffer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum SampleFormat {
    /// `f32` with a valid range of `-1.0..=1.0`.
    F32,
}

/// A sample type that can be used for a stream.
pub trait SizedSample: Copy + Send + 'static {
    /// The corresponding sample format.
    const FORMAT: SampleFormat;

    #[doc(hidden)]
    fn from_f32_slice(data: &[f32]) -> &[Self];

    #[doc(hidden)]
    fn from_f32_slice_mut(data: &mut [f32]) -> &mut [Self];
}

impl SizedSample for f32 {
    const FORMAT: SampleFormat = SampleFormat::F32;

    fn from_f32_slice(data: &[f32]) -> &[Self] {
        data
    }

    fn from_f32_slice_mut(data: &mut [f32]) -> &mut [Self] {
        data
    }
}

/// The buffer size of a stream.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BufferSize {
    /// Let the backend pick a buffer size.
    #[default]
    Default,
    /// A fixed number of frames per buffer.
    Fixed(FrameCount),
}

/// The configuration of a stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamConfig {
    /// Number of interleaved channels.
    pub channels: ChannelCount,
    /// Sample rate in Hz.
    pub sample_rate: SampleRate,
    /// Frames per buffer.
    pub buffer_size: BufferSize,
}

impl StreamConfig {
    /// Frames per buffer, 20 ms when the backend can pick
    pub(crate) const fn buffer_frames(&self) -> usize {
        match self.buffer_size {
            BufferSize::Default => self.sample_rate as usize / 50,
            BufferSize::Fixed(frames) => frames as usize,
        }
    }
}

/// A stream configuration supported by a device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupportedStreamConfig {
    channels: ChannelCount,
    sample_rate: SampleRate,
    sample_format: SampleFormat,
}

impl SupportedStreamConfig {
    /// Returns the number of channels.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn channels(&self) -> ChannelCount {
        self.channels
    }

    /// Returns the sample rate.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    /// Returns the sample format.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }

    /// Returns a stream config with the default buffer size.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn config(&self) -> StreamConfig {
        StreamConfig {
            channels: self.channels,
            sample_rate: self.sample_rate,
            buffer_size: BufferSize::Default,
        }
    }
}

impl From<SupportedStreamConfig> for StreamConfig {
    fn from(config: SupportedStreamConfig) -> Self {
        config.config()
    }
}

/// Information passed to the input data callback.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InputCallbackInfo {
    _private: (),
}

/// Information passed to the output data callback.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OutputCallbackInfo {
    _private: (),
}

/// The direction of a device or stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Direction {
    Input,
    Output,
}

pub(crate) type InputCallback = Box<dyn FnMut(&[f32]) + Send>;
pub(crate) type OutputCallback = Box<dyn FnMut(&mut [f32]) + Send>;
pub(crate) type ErrorCallback = Box<dyn FnMut(Error) + Send>;

/// The data callback of a stream, its direction is the direction of the stream.
pub(crate) enum Callback {
    Input(InputCallback),
    Output(OutputCallback),
}

impl Callback {
    pub(crate) const fn direction(&self) -> Direction {
        match self {
            Self::Input(_) => Direction::Input,
            Self::Output(_) => Direction::Output,
        }
    }
}

// MARK: Traits
/// The traits of hosts, devices and streams.
pub mod traits {
    use std::time::Duration;

    use crate::{
        Error, InputCallbackInfo, OutputCallbackInfo, SizedSample, StreamConfig,
        SupportedStreamConfig,
    };

    /// An audio host that provides devices.
    pub trait HostTrait {
        /// The device type of this host.
        type Device: DeviceTrait;

        /// Returns the default input device, if any.
        fn default_input_device(&self) -> Option<Self::Device>;

        /// Returns the default output device, if any.
        fn default_output_device(&self) -> Option<Self::Device>;
    }

    /// An audio device.
    pub trait DeviceTrait {
        /// The stream type of this device.
        type Stream: StreamTrait;

        /// Returns the default input stream config.
        fn default_input_config(&self) -> Result<SupportedStreamConfig, Error>;

        /// Returns the default output stream config.
        fn default_output_config(&self) -> Result<SupportedStreamConfig, Error>;

        /// Builds a paused input stream, call [`StreamTrait::play`] to start it.
        fn build_input_stream<T, D, E>(
            &self,
            config: StreamConfig,
            data_callback: D,
            error_callback: E,
            timeout: Option<Duration>,
        ) -> Result<Self::Stream, Error>
        where
            T: SizedSample,
            D: FnMut(&[T], &InputCallbackInfo) + Send + 'static,
            E: FnMut(Error) + Send + 'static;

        /// Builds a paused output stream, call [`StreamTrait::play`] to start it.
        fn build_output_stream<T, D, E>(
            &self,
            config: StreamConfig,
            data_callback: D,
            error_callback: E,
            timeout: Option<Duration>,
        ) -> Result<Self::Stream, Error>
        where
            T: SizedSample,
            D: FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static,
            E: FnMut(Error) + Send + 'static;
    }

    /// An audio stream, it is closed when dropped.
    pub trait StreamTrait {
        /// Starts or resumes the stream.
        fn play(&self) -> Result<(), Error>;

        /// Pauses the stream.
        fn pause(&self) -> Result<(), Error>;
    }
}

// MARK: Host
/// Returns the default host of the platform.
#[allow(clippy::missing_const_for_fn)] // Upstream API is not const
pub fn default_host() -> Host {
    Host { _private: () }
}

/// The default audio host of the platform.
pub struct Host {
    _private: (),
}

impl HostTrait for Host {
    type Device = Device;

    fn default_input_device(&self) -> Option<Device> {
        backend::default_device(Direction::Input).map(|device| Device {
            device,
            direction: Direction::Input,
        })
    }

    fn default_output_device(&self) -> Option<Device> {
        backend::default_device(Direction::Output).map(|device| Device {
            device,
            direction: Direction::Output,
        })
    }
}

// MARK: Device
/// An audio device.
pub struct Device {
    device: backend::Device,
    direction: Direction,
}

impl Device {
    fn default_config(
        &self,
        direction: Direction,
        channels: ChannelCount,
    ) -> Result<SupportedStreamConfig, Error> {
        if self.direction != direction {
            return Err(Error::new(ErrorKind::UnsupportedOperation));
        }
        Ok(SupportedStreamConfig {
            channels,
            sample_rate: self.device.default_sample_rate()?,
            sample_format: SampleFormat::F32,
        })
    }

    fn check_config(&self, direction: Direction, config: &StreamConfig) -> Result<(), Error> {
        if self.direction != direction {
            return Err(Error::new(ErrorKind::UnsupportedOperation));
        }
        if config.channels == 0 || config.sample_rate == 0 || config.buffer_frames() == 0 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        Ok(())
    }
}

impl DeviceTrait for Device {
    type Stream = Stream;

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        self.default_config(Direction::Input, 1)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        self.default_config(Direction::Output, 2)
    }

    fn build_input_stream<T, D, E>(
        &self,
        config: StreamConfig,
        mut data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Stream, Error>
    where
        T: SizedSample,
        D: FnMut(&[T], &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        self.check_config(Direction::Input, &config)?;
        let info = InputCallbackInfo { _private: () };
        self.device
            .build_stream(
                &config,
                Callback::Input(Box::new(move |data| {
                    data_callback(T::from_f32_slice(data), &info);
                })),
                Box::new(error_callback),
            )
            .map(Stream)
    }

    fn build_output_stream<T, D, E>(
        &self,
        config: StreamConfig,
        mut data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Stream, Error>
    where
        T: SizedSample,
        D: FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        self.check_config(Direction::Output, &config)?;
        let info = OutputCallbackInfo { _private: () };
        self.device
            .build_stream(
                &config,
                Callback::Output(Box::new(move |data| {
                    data_callback(T::from_f32_slice_mut(data), &info);
                })),
                Box::new(error_callback),
            )
            .map(Stream)
    }
}

// MARK: Stream
/// An audio stream, it is closed when dropped.
pub struct Stream(backend::Stream);

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), Error> {
        self.0.play()
    }

    fn pause(&self) -> Result<(), Error> {
        self.0.pause()
    }
}
