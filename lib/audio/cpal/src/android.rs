/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! AAudio backend, the stream converts the format, rate and channels.

#![allow(unsafe_code)]

use std::ffi::{CStr, c_void};
use std::ptr;
use std::sync::Mutex;

use crate::{Callback, Direction, Error, ErrorCallback, ErrorKind, SampleRate, StreamConfig};

mod sys {
    use std::ffi::{c_char, c_void};

    pub(super) type DataCallback = unsafe extern "C" fn(
        stream: *mut c_void,
        user_data: *mut c_void,
        audio_data: *mut c_void,
        frame_count: i32,
    ) -> i32;
    pub(super) type ErrorCallback =
        unsafe extern "C" fn(stream: *mut c_void, user_data: *mut c_void, error: i32);

    pub(super) const AAUDIO_DIRECTION_OUTPUT: i32 = 0;
    pub(super) const AAUDIO_DIRECTION_INPUT: i32 = 1;
    pub(super) const AAUDIO_FORMAT_PCM_FLOAT: i32 = 2;
    pub(super) const AAUDIO_PERFORMANCE_MODE_LOW_LATENCY: i32 = 12;
    pub(super) const AAUDIO_CALLBACK_RESULT_CONTINUE: i32 = 0;
    pub(super) const AAUDIO_ERROR_DISCONNECTED: i32 = -899;
    pub(super) const AAUDIO_ERROR_ILLEGAL_ARGUMENT: i32 = -898;
    pub(super) const AAUDIO_ERROR_UNAVAILABLE: i32 = -889;
    pub(super) const AAUDIO_ERROR_NO_FREE_HANDLES: i32 = -888;
    pub(super) const AAUDIO_ERROR_INVALID_FORMAT: i32 = -883;
    pub(super) const AAUDIO_ERROR_NO_SERVICE: i32 = -881;
    pub(super) const AAUDIO_ERROR_INVALID_RATE: i32 = -880;

    #[link(name = "aaudio")]
    unsafe extern "C" {
        pub(super) fn AAudio_createStreamBuilder(builder: *mut *mut c_void) -> i32;
        pub(super) fn AAudio_convertResultToText(result: i32) -> *const c_char;
        pub(super) fn AAudioStreamBuilder_setDirection(builder: *mut c_void, direction: i32);
        pub(super) fn AAudioStreamBuilder_setFormat(builder: *mut c_void, format: i32);
        pub(super) fn AAudioStreamBuilder_setChannelCount(builder: *mut c_void, channels: i32);
        pub(super) fn AAudioStreamBuilder_setSampleRate(builder: *mut c_void, rate: i32);
        pub(super) fn AAudioStreamBuilder_setPerformanceMode(builder: *mut c_void, mode: i32);
        pub(super) fn AAudioStreamBuilder_setDataCallback(
            builder: *mut c_void,
            callback: DataCallback,
            user_data: *mut c_void,
        );
        pub(super) fn AAudioStreamBuilder_setErrorCallback(
            builder: *mut c_void,
            callback: ErrorCallback,
            user_data: *mut c_void,
        );
        pub(super) fn AAudioStreamBuilder_openStream(
            builder: *mut c_void,
            stream: *mut *mut c_void,
        ) -> i32;
        pub(super) fn AAudioStreamBuilder_delete(builder: *mut c_void) -> i32;
        pub(super) fn AAudioStream_getSampleRate(stream: *mut c_void) -> i32;
        pub(super) fn AAudioStream_getChannelCount(stream: *mut c_void) -> i32;
        pub(super) fn AAudioStream_requestStart(stream: *mut c_void) -> i32;
        pub(super) fn AAudioStream_requestStop(stream: *mut c_void) -> i32;
        pub(super) fn AAudioStream_close(stream: *mut c_void) -> i32;
    }
}

fn error(result: i32) -> Error {
    // SAFETY: AAudio_convertResultToText returns a static NUL-terminated string for any result.
    let message = unsafe { CStr::from_ptr(sys::AAudio_convertResultToText(result)) }
        .to_string_lossy()
        .into_owned();
    let kind = match result {
        sys::AAUDIO_ERROR_NO_FREE_HANDLES => ErrorKind::DeviceBusy,
        sys::AAUDIO_ERROR_DISCONNECTED
        | sys::AAUDIO_ERROR_UNAVAILABLE
        | sys::AAUDIO_ERROR_NO_SERVICE => ErrorKind::DeviceNotAvailable,
        sys::AAUDIO_ERROR_ILLEGAL_ARGUMENT
        | sys::AAUDIO_ERROR_INVALID_FORMAT
        | sys::AAUDIO_ERROR_INVALID_RATE => ErrorKind::UnsupportedConfig,
        _ => ErrorKind::BackendError,
    };
    Error::with_message(kind, message)
}

fn check(result: i32) -> Result<(), Error> {
    if result < 0 {
        Err(error(result))
    } else {
        Ok(())
    }
}

/// A stream builder, it is deleted when dropped
struct Builder(*mut c_void);

impl Builder {
    fn new(direction: Direction) -> Result<Self, Error> {
        let mut builder = ptr::null_mut();
        // SAFETY: `builder` is a valid output pointer.
        check(unsafe { sys::AAudio_createStreamBuilder(&mut builder) })?;
        let builder = Self(builder);
        // SAFETY: the builder is live.
        unsafe {
            sys::AAudioStreamBuilder_setDirection(
                builder.0,
                match direction {
                    Direction::Input => sys::AAUDIO_DIRECTION_INPUT,
                    Direction::Output => sys::AAUDIO_DIRECTION_OUTPUT,
                },
            );
            sys::AAudioStreamBuilder_setPerformanceMode(
                builder.0,
                sys::AAUDIO_PERFORMANCE_MODE_LOW_LATENCY,
            );
        }
        Ok(builder)
    }

    fn open(&self) -> Result<*mut c_void, Error> {
        let mut stream = ptr::null_mut();
        // SAFETY: the builder is live and `stream` is a valid output pointer.
        check(unsafe { sys::AAudioStreamBuilder_openStream(self.0, &mut stream) })?;
        Ok(stream)
    }
}

impl Drop for Builder {
    fn drop(&mut self) {
        // SAFETY: the builder was created by AAudio_createStreamBuilder and is deleted once.
        unsafe { sys::AAudioStreamBuilder_delete(self.0) };
    }
}

// MARK: Device
pub(crate) struct Device(Direction);

pub(crate) const fn default_device(direction: Direction) -> Option<Device> {
    Some(Device(direction))
}

impl Device {
    /// The native sample rate, which streams can use without conversion
    pub(crate) fn default_sample_rate(&self) -> Result<SampleRate, Error> {
        let stream = Builder::new(self.0)?.open()?;
        // SAFETY: the stream is open and closed once after reading its rate.
        unsafe {
            let rate = sys::AAudioStream_getSampleRate(stream);
            sys::AAudioStream_close(stream);
            Ok(rate as SampleRate)
        }
    }

    pub(crate) fn build_stream(
        &self,
        config: &StreamConfig,
        callback: Callback,
        error: ErrorCallback,
    ) -> Result<Stream, Error> {
        let builder = Builder::new(callback.direction())?;
        let callbacks = Box::into_raw(Box::new(Callbacks {
            data: callback,
            error: Mutex::new(error),
            channels: config.channels as usize,
        }));
        let mut stream = Stream {
            stream: ptr::null_mut(),
            callbacks,
        };
        // SAFETY: the builder is live and the callbacks stay alive until the stream is closed.
        unsafe {
            sys::AAudioStreamBuilder_setFormat(builder.0, sys::AAUDIO_FORMAT_PCM_FLOAT);
            sys::AAudioStreamBuilder_setChannelCount(builder.0, config.channels as i32);
            sys::AAudioStreamBuilder_setSampleRate(builder.0, config.sample_rate as i32);
            sys::AAudioStreamBuilder_setDataCallback(builder.0, data_callback, callbacks.cast());
            sys::AAudioStreamBuilder_setErrorCallback(builder.0, error_callback, callbacks.cast());
        }
        stream.stream = builder.open()?;
        // SAFETY: the stream is open.
        if unsafe { sys::AAudioStream_getChannelCount(stream.stream) } != config.channels as i32 {
            return Err(Error::new(ErrorKind::UnsupportedConfig));
        }
        Ok(stream)
    }
}

// MARK: Stream
struct Callbacks {
    data: Callback,
    error: Mutex<ErrorCallback>,
    channels: usize,
}

unsafe extern "C" fn data_callback(
    _stream: *mut c_void,
    user_data: *mut c_void,
    audio_data: *mut c_void,
    frame_count: i32,
) -> i32 {
    // SAFETY: data callbacks run one at a time and only use the data callback, the error callback
    // is only borrowed by the error thread. The audio data holds `frame_count` interleaved frames.
    unsafe {
        let callbacks = user_data.cast::<Callbacks>();
        let length = frame_count as usize * (*callbacks).channels;
        match &mut (*callbacks).data {
            Callback::Input(data) => {
                data(std::slice::from_raw_parts(audio_data.cast(), length));
            }
            Callback::Output(data) => {
                data(std::slice::from_raw_parts_mut(audio_data.cast(), length));
            }
        }
    }
    sys::AAUDIO_CALLBACK_RESULT_CONTINUE
}

unsafe extern "C" fn error_callback(_stream: *mut c_void, user_data: *mut c_void, result: i32) {
    // SAFETY: the error callback is only used behind its mutex.
    let callbacks = unsafe { &(*user_data.cast::<Callbacks>()).error };
    if let Ok(mut callback) = callbacks.lock() {
        callback(error(result));
    }
}

pub(crate) struct Stream {
    stream: *mut c_void,
    callbacks: *mut Callbacks,
}

// SAFETY: AAudio stream functions are thread-safe and the callbacks are only used by the stream.
unsafe impl Send for Stream {}
// SAFETY: the same AAudio thread-safety permits shared references.
unsafe impl Sync for Stream {}

impl Stream {
    pub(crate) fn play(&self) -> Result<(), Error> {
        // SAFETY: the stream is open.
        check(unsafe { sys::AAudioStream_requestStart(self.stream) })
    }

    /// Stops the stream, input streams can't be paused
    pub(crate) fn pause(&self) -> Result<(), Error> {
        // SAFETY: the stream is open.
        check(unsafe { sys::AAudioStream_requestStop(self.stream) })
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: closing the stream stops its callbacks, so they can be freed after.
        unsafe {
            if !self.stream.is_null() {
                sys::AAudioStream_requestStop(self.stream);
                sys::AAudioStream_close(self.stream);
            }
            drop(Box::from_raw(self.callbacks));
        }
    }
}
