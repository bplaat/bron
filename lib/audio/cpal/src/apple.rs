/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! CoreAudio backend for macOS and iOS: an output AudioUnit plays and an AudioQueue records, both
//! convert the format, rate and channels.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;

pub(crate) use self::platform::{Device, default_device};
use crate::{
    Callback, Error, ErrorCallback, ErrorKind, InputCallback, OutputCallback, StreamConfig,
};

mod sys {
    use std::ffi::c_void;

    pub(super) type OsStatus = i32;
    pub(super) type AudioQueueRef = *mut c_void;
    pub(super) type AudioComponent = *mut c_void;
    pub(super) type AudioUnit = *mut c_void;
    pub(super) type AudioQueueInputCallback = unsafe extern "C" fn(
        user_data: *mut c_void,
        queue: AudioQueueRef,
        buffer: *mut AudioQueueBuffer,
        start_time: *const c_void,
        packet_count: u32,
        packet_descriptions: *const c_void,
    );
    pub(super) type AuRenderCallback = unsafe extern "C" fn(
        user_data: *mut c_void,
        action_flags: *mut u32,
        time_stamp: *const c_void,
        bus_number: u32,
        frame_count: u32,
        data: *mut AudioBufferList,
    ) -> OsStatus;

    pub(super) const fn four_cc(code: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*code)
    }

    pub(super) const K_AUDIO_FORMAT_LINEAR_PCM: u32 = four_cc(b"lpcm");
    pub(super) const K_AUDIO_FORMAT_FLAG_IS_FLOAT: u32 = 1 << 0;
    pub(super) const K_AUDIO_FORMAT_FLAG_IS_PACKED: u32 = 1 << 3;
    pub(super) const K_AUDIO_UNIT_TYPE_OUTPUT: u32 = four_cc(b"auou");
    #[cfg(target_os = "macos")]
    pub(super) const K_AUDIO_UNIT_SUB_TYPE_OUTPUT: u32 = four_cc(b"def ");
    #[cfg(not(target_os = "macos"))]
    pub(super) const K_AUDIO_UNIT_SUB_TYPE_OUTPUT: u32 = four_cc(b"rioc");
    pub(super) const K_AUDIO_UNIT_MANUFACTURER_APPLE: u32 = four_cc(b"appl");
    pub(super) const K_AUDIO_UNIT_PROPERTY_STREAM_FORMAT: u32 = 8;
    pub(super) const K_AUDIO_UNIT_PROPERTY_SET_RENDER_CALLBACK: u32 = 23;
    pub(super) const K_AUDIO_UNIT_SCOPE_INPUT: u32 = 1;

    #[repr(C)]
    pub(super) struct AudioStreamBasicDescription {
        pub(super) sample_rate: f64,
        pub(super) format_id: u32,
        pub(super) format_flags: u32,
        pub(super) bytes_per_packet: u32,
        pub(super) frames_per_packet: u32,
        pub(super) bytes_per_frame: u32,
        pub(super) channels_per_frame: u32,
        pub(super) bits_per_channel: u32,
        pub(super) reserved: u32,
    }

    #[repr(C)]
    pub(super) struct AudioQueueBuffer {
        pub(super) audio_data_bytes_capacity: u32,
        pub(super) audio_data: *mut c_void,
        pub(super) audio_data_byte_size: u32,
        pub(super) user_data: *mut c_void,
        pub(super) packet_description_capacity: u32,
        pub(super) packet_descriptions: *mut c_void,
        pub(super) packet_description_count: u32,
    }

    #[repr(C)]
    pub(super) struct AudioComponentDescription {
        pub(super) component_type: u32,
        pub(super) component_sub_type: u32,
        pub(super) component_manufacturer: u32,
        pub(super) component_flags: u32,
        pub(super) component_flags_mask: u32,
    }

    #[repr(C)]
    pub(super) struct AuRenderCallbackStruct {
        pub(super) input_proc: AuRenderCallback,
        pub(super) input_proc_ref_con: *mut c_void,
    }

    #[repr(C)]
    pub(super) struct AudioBuffer {
        pub(super) number_channels: u32,
        pub(super) data_byte_size: u32,
        pub(super) data: *mut c_void,
    }

    #[repr(C)]
    pub(super) struct AudioBufferList {
        pub(super) number_buffers: u32,
        pub(super) buffers: [AudioBuffer; 1],
    }

    #[link(name = "AudioToolbox", kind = "framework")]
    unsafe extern "C" {
        pub(super) fn AudioQueueNewInput(
            format: *const AudioStreamBasicDescription,
            callback: AudioQueueInputCallback,
            user_data: *mut c_void,
            run_loop: *const c_void,
            run_loop_mode: *const c_void,
            flags: u32,
            queue: *mut AudioQueueRef,
        ) -> OsStatus;
        pub(super) fn AudioQueueAllocateBuffer(
            queue: AudioQueueRef,
            byte_size: u32,
            buffer: *mut *mut AudioQueueBuffer,
        ) -> OsStatus;
        pub(super) fn AudioQueueEnqueueBuffer(
            queue: AudioQueueRef,
            buffer: *mut AudioQueueBuffer,
            packet_count: u32,
            packet_descriptions: *const c_void,
        ) -> OsStatus;
        pub(super) fn AudioQueueStart(queue: AudioQueueRef, start_time: *const c_void) -> OsStatus;
        pub(super) fn AudioQueuePause(queue: AudioQueueRef) -> OsStatus;
        pub(super) fn AudioQueueDispose(queue: AudioQueueRef, immediate: u8) -> OsStatus;

        pub(super) fn AudioComponentFindNext(
            component: AudioComponent,
            description: *const AudioComponentDescription,
        ) -> AudioComponent;
        pub(super) fn AudioComponentInstanceNew(
            component: AudioComponent,
            unit: *mut AudioUnit,
        ) -> OsStatus;
        pub(super) fn AudioComponentInstanceDispose(unit: AudioUnit) -> OsStatus;
        pub(super) fn AudioUnitSetProperty(
            unit: AudioUnit,
            id: u32,
            scope: u32,
            element: u32,
            data: *const c_void,
            data_size: u32,
        ) -> OsStatus;
        pub(super) fn AudioUnitInitialize(unit: AudioUnit) -> OsStatus;
        pub(super) fn AudioUnitUninitialize(unit: AudioUnit) -> OsStatus;
        pub(super) fn AudioOutputUnitStart(unit: AudioUnit) -> OsStatus;
        pub(super) fn AudioOutputUnitStop(unit: AudioUnit) -> OsStatus;
    }
}

/// Number of buffers queued at the input audio queue
const BUFFER_COUNT: usize = 3;

fn check(status: sys::OsStatus) -> Result<(), Error> {
    if status == 0 {
        Ok(())
    } else {
        Err(Error::with_message(
            ErrorKind::BackendError,
            format!("CoreAudio error {status}"),
        ))
    }
}

// MARK: macOS
/// The macOS hardware abstraction layer knows the default devices and their sample rates
#[cfg(target_os = "macos")]
mod platform {
    use std::ptr;

    use super::check;
    use crate::{Direction, Error, SampleRate, StreamConfig};

    mod sys {
        use std::ffi::c_void;

        use super::super::sys::{OsStatus, four_cc};

        pub(super) type AudioObjectId = u32;

        pub(super) const K_AUDIO_OBJECT_SYSTEM_OBJECT: AudioObjectId = 1;
        pub(super) const K_AUDIO_OBJECT_UNKNOWN: AudioObjectId = 0;
        pub(super) const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_INPUT_DEVICE: u32 = four_cc(b"dIn ");
        pub(super) const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE: u32 = four_cc(b"dOut");
        pub(super) const K_AUDIO_DEVICE_PROPERTY_NOMINAL_SAMPLE_RATE: u32 = four_cc(b"nsrt");
        pub(super) const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = four_cc(b"glob");
        pub(super) const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;

        #[repr(C)]
        pub(super) struct AudioObjectPropertyAddress {
            pub(super) selector: u32,
            pub(super) scope: u32,
            pub(super) element: u32,
        }

        #[link(name = "CoreAudio", kind = "framework")]
        unsafe extern "C" {
            pub(super) fn AudioObjectGetPropertyData(
                object: AudioObjectId,
                address: *const AudioObjectPropertyAddress,
                qualifier_size: u32,
                qualifier: *const c_void,
                data_size: *mut u32,
                data: *mut c_void,
            ) -> OsStatus;
        }
    }

    /// Reads a fixed size property of a CoreAudio object
    fn property<T: Default>(object: sys::AudioObjectId, selector: u32) -> Result<T, Error> {
        let address = sys::AudioObjectPropertyAddress {
            selector,
            scope: sys::K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            element: sys::K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let mut value = T::default();
        let mut size = size_of::<T>() as u32;
        // SAFETY: the address is valid and the value has room for `size` bytes.
        check(unsafe {
            sys::AudioObjectGetPropertyData(
                object,
                &address,
                0,
                ptr::null(),
                &mut size,
                (&raw mut value).cast(),
            )
        })?;
        Ok(value)
    }

    pub(crate) struct Device(sys::AudioObjectId);

    pub(crate) fn default_device(direction: Direction) -> Option<Device> {
        let selector = match direction {
            Direction::Input => sys::K_AUDIO_HARDWARE_PROPERTY_DEFAULT_INPUT_DEVICE,
            Direction::Output => sys::K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE,
        };
        property(sys::K_AUDIO_OBJECT_SYSTEM_OBJECT, selector)
            .ok()
            .filter(|&id| id != sys::K_AUDIO_OBJECT_UNKNOWN)
            .map(Device)
    }

    impl Device {
        pub(crate) fn default_sample_rate(&self) -> Result<SampleRate, Error> {
            property::<f64>(self.0, sys::K_AUDIO_DEVICE_PROPERTY_NOMINAL_SAMPLE_RATE)
                .map(|rate| rate as SampleRate)
        }
    }

    /// macOS has no audio session
    pub(super) const fn activate_session(
        _direction: Direction,
        _config: &StreamConfig,
    ) -> Result<(), Error> {
        Ok(())
    }
}

// MARK: iOS
/// The iOS audio session knows the sample rate and must be configured and activated
#[cfg(not(target_os = "macos"))]
mod platform {
    use std::ffi::c_void;
    use std::ptr;

    use objc2::runtime::{AnyObject as Object, Bool};
    use objc2::{class, msg_send};

    use crate::{Direction, Error, ErrorKind, SampleRate, StreamConfig};

    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {
        static AVAudioSessionCategoryAmbient: *mut Object;
        static AVAudioSessionCategoryPlayAndRecord: *mut Object;
    }

    const OPTION_MIX_WITH_OTHERS: usize = 0x1;
    const OPTION_DEFAULT_TO_SPEAKER: usize = 0x8;

    fn session() -> *mut Object {
        // SAFETY: the shared audio session always exists.
        unsafe { msg_send![class!(AVAudioSession), sharedInstance] }
    }

    fn session_error(message: &'static str) -> Error {
        Error::with_message(ErrorKind::BackendError, message)
    }

    pub(crate) struct Device;

    pub(crate) fn default_device(direction: Direction) -> Option<Device> {
        match direction {
            // SAFETY: the session is a valid AVAudioSession.
            Direction::Input => unsafe { msg_send![session(), isInputAvailable] },
            Direction::Output => Bool::YES,
        }
        .as_bool()
        .then_some(Device)
    }

    impl Device {
        pub(crate) fn default_sample_rate(&self) -> Result<SampleRate, Error> {
            // SAFETY: the session is a valid AVAudioSession.
            let rate: f64 = unsafe { msg_send![session(), sampleRate] };
            Ok(rate as SampleRate)
        }
    }

    /// Plays mixed with other apps, recording switches to play and record which keeps playing
    pub(super) fn activate_session(
        direction: Direction,
        config: &StreamConfig,
    ) -> Result<(), Error> {
        let session = session();
        let error = ptr::null_mut::<c_void>();
        // SAFETY: the session and the category constants are valid objects.
        unsafe {
            let category: *mut Object = msg_send![session, category];
            let recording: Bool =
                msg_send![category, isEqualToString: AVAudioSessionCategoryPlayAndRecord];
            if !recording.as_bool() {
                let (category, options) = match direction {
                    Direction::Input => (
                        AVAudioSessionCategoryPlayAndRecord,
                        OPTION_MIX_WITH_OTHERS | OPTION_DEFAULT_TO_SPEAKER,
                    ),
                    Direction::Output => (AVAudioSessionCategoryAmbient, OPTION_MIX_WITH_OTHERS),
                };
                let set: Bool =
                    msg_send![session, setCategory: category, withOptions: options, error: error];
                if !set.as_bool() {
                    return Err(session_error("Can't set the audio session category"));
                }
            }

            // The buffer duration is only a preference, the session may pick another one
            let duration = config.buffer_frames() as f64 / config.sample_rate as f64;
            let _: Bool = msg_send![session, setPreferredIOBufferDuration: duration, error: error];
            let active: Bool = msg_send![session, setActive: Bool::YES, error: error];
            if !active.as_bool() {
                return Err(session_error("Can't activate the audio session"));
            }
        }
        Ok(())
    }
}

// MARK: Streams
const fn format(config: &StreamConfig) -> sys::AudioStreamBasicDescription {
    let channels = config.channels as u32;
    sys::AudioStreamBasicDescription {
        sample_rate: config.sample_rate as f64,
        format_id: sys::K_AUDIO_FORMAT_LINEAR_PCM,
        format_flags: sys::K_AUDIO_FORMAT_FLAG_IS_FLOAT | sys::K_AUDIO_FORMAT_FLAG_IS_PACKED,
        bytes_per_packet: channels * size_of::<f32>() as u32,
        frames_per_packet: 1,
        bytes_per_frame: channels * size_of::<f32>() as u32,
        channels_per_frame: channels,
        bits_per_channel: 32,
        reserved: 0,
    }
}

impl Device {
    /// Errors are reported when building, CoreAudio doesn't report errors of running streams
    pub(crate) fn build_stream(
        &self,
        config: &StreamConfig,
        callback: Callback,
        _error: ErrorCallback,
    ) -> Result<Stream, Error> {
        platform::activate_session(callback.direction(), config)?;
        match callback {
            Callback::Input(data) => Queue::new(config, data).map(Stream::Queue),
            Callback::Output(data) => Unit::new(config, data).map(Stream::Unit),
        }
    }
}

pub(crate) enum Stream {
    Queue(Queue),
    Unit(Unit),
}

impl Stream {
    pub(crate) fn play(&self) -> Result<(), Error> {
        match self {
            // SAFETY: the queue is live.
            Self::Queue(queue) => check(unsafe { sys::AudioQueueStart(queue.queue, ptr::null()) }),
            // SAFETY: the unit is initialized.
            Self::Unit(unit) => check(unsafe { sys::AudioOutputUnitStart(unit.unit) }),
        }
    }

    pub(crate) fn pause(&self) -> Result<(), Error> {
        match self {
            // SAFETY: the queue is live.
            Self::Queue(queue) => check(unsafe { sys::AudioQueuePause(queue.queue) }),
            // SAFETY: the unit is initialized.
            Self::Unit(unit) => check(unsafe { sys::AudioOutputUnitStop(unit.unit) }),
        }
    }
}

// MARK: Input queue
/// An input audio queue, it records from the default input device
pub(crate) struct Queue {
    queue: sys::AudioQueueRef,
    data: *mut InputCallback,
}

// SAFETY: audio queue functions are thread-safe and the callback is only used by the queue.
unsafe impl Send for Queue {}
// SAFETY: the same audio queue thread-safety permits shared references.
unsafe impl Sync for Queue {}

impl Queue {
    fn new(config: &StreamConfig, data: InputCallback) -> Result<Self, Error> {
        let data = Box::into_raw(Box::new(data));
        let mut queue = ptr::null_mut();
        // SAFETY: without a run loop the callback runs on the queue's own thread, `data` stays
        // alive until the queue is disposed.
        if let Err(err) = check(unsafe {
            sys::AudioQueueNewInput(
                &format(config),
                input_callback,
                data.cast(),
                ptr::null(),
                ptr::null(),
                0,
                &mut queue,
            )
        }) {
            // SAFETY: the queue wasn't created, so nothing else owns `data`.
            drop(unsafe { Box::from_raw(data) });
            return Err(err);
        }
        let stream = Self { queue, data };

        let byte_size =
            (config.buffer_frames() * config.channels as usize * size_of::<f32>()) as u32;
        for _ in 0..BUFFER_COUNT {
            let mut buffer = ptr::null_mut();
            // SAFETY: the queue is live and owns the allocated buffer.
            check(unsafe { sys::AudioQueueAllocateBuffer(queue, byte_size, &mut buffer) })?;
            // SAFETY: the buffer belongs to this queue.
            check(unsafe { sys::AudioQueueEnqueueBuffer(queue, buffer, 0, ptr::null()) })?;
        }
        Ok(stream)
    }
}

unsafe extern "C" fn input_callback(
    user_data: *mut c_void,
    queue: sys::AudioQueueRef,
    buffer: *mut sys::AudioQueueBuffer,
    _start_time: *const c_void,
    _packet_count: u32,
    _packet_descriptions: *const c_void,
) {
    // SAFETY: the queue passes the callback and a filled buffer of packed f32 frames, callbacks
    // run one at a time on the queue's thread.
    unsafe {
        let data = &mut *user_data.cast::<InputCallback>();
        let buffer_ref = &*buffer;
        let samples = std::slice::from_raw_parts(
            buffer_ref.audio_data.cast::<f32>(),
            buffer_ref.audio_data_byte_size as usize / size_of::<f32>(),
        );
        if !samples.is_empty() {
            data(samples);
        }
        // Fails while the queue is disposed, then the buffer is freed with it
        sys::AudioQueueEnqueueBuffer(queue, buffer, 0, ptr::null());
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        // SAFETY: an immediate dispose waits for running callbacks, so `data` can be freed after.
        unsafe {
            sys::AudioQueueDispose(self.queue, 1);
            drop(Box::from_raw(self.data));
        }
    }
}

// MARK: Output unit
/// An output audio unit, it plays to the default output device and follows its changes
pub(crate) struct Unit {
    unit: sys::AudioUnit,
    data: *mut OutputCallback,
}

// SAFETY: audio unit functions are thread-safe and the callback is only used by the unit.
unsafe impl Send for Unit {}
// SAFETY: the same audio unit thread-safety permits shared references.
unsafe impl Sync for Unit {}

impl Unit {
    fn new(config: &StreamConfig, data: OutputCallback) -> Result<Self, Error> {
        let description = sys::AudioComponentDescription {
            component_type: sys::K_AUDIO_UNIT_TYPE_OUTPUT,
            component_sub_type: sys::K_AUDIO_UNIT_SUB_TYPE_OUTPUT,
            component_manufacturer: sys::K_AUDIO_UNIT_MANUFACTURER_APPLE,
            component_flags: 0,
            component_flags_mask: 0,
        };
        // SAFETY: the description is valid, a null component searches from the start.
        let component = unsafe { sys::AudioComponentFindNext(ptr::null_mut(), &description) };
        if component.is_null() {
            return Err(Error::new(ErrorKind::DeviceNotAvailable));
        }
        let mut unit = ptr::null_mut();
        // SAFETY: the component is valid and `unit` is a valid output pointer.
        check(unsafe { sys::AudioComponentInstanceNew(component, &mut unit) })?;
        let stream = Self {
            unit,
            data: Box::into_raw(Box::new(data)),
        };

        let format = format(config);
        let callback = sys::AuRenderCallbackStruct {
            input_proc: render_callback,
            input_proc_ref_con: stream.data.cast(),
        };
        // SAFETY: the unit is live and the properties point to values of the given sizes, the
        // callback data stays alive until the unit is disposed.
        unsafe {
            check(sys::AudioUnitSetProperty(
                unit,
                sys::K_AUDIO_UNIT_PROPERTY_STREAM_FORMAT,
                sys::K_AUDIO_UNIT_SCOPE_INPUT,
                0,
                (&raw const format).cast(),
                size_of_val(&format) as u32,
            ))?;
            check(sys::AudioUnitSetProperty(
                unit,
                sys::K_AUDIO_UNIT_PROPERTY_SET_RENDER_CALLBACK,
                sys::K_AUDIO_UNIT_SCOPE_INPUT,
                0,
                (&raw const callback).cast(),
                size_of_val(&callback) as u32,
            ))?;
            check(sys::AudioUnitInitialize(unit))?;
        }
        Ok(stream)
    }
}

unsafe extern "C" fn render_callback(
    user_data: *mut c_void,
    _action_flags: *mut u32,
    _time_stamp: *const c_void,
    _bus_number: u32,
    _frame_count: u32,
    data: *mut sys::AudioBufferList,
) -> sys::OsStatus {
    // SAFETY: the unit passes the callback and one interleaved buffer of packed f32 frames,
    // render callbacks run one at a time on the unit's thread.
    unsafe {
        let callback = &mut *user_data.cast::<OutputCallback>();
        let buffer = &(*data).buffers[0];
        let samples = std::slice::from_raw_parts_mut(
            buffer.data.cast::<f32>(),
            buffer.data_byte_size as usize / size_of::<f32>(),
        );
        callback(samples);
    }
    0
}

impl Drop for Unit {
    fn drop(&mut self) {
        // SAFETY: disposing the unit stops its render callbacks, so `data` can be freed after.
        unsafe {
            sys::AudioOutputUnitStop(self.unit);
            sys::AudioUnitUninitialize(self.unit);
            sys::AudioComponentInstanceDispose(self.unit);
            drop(Box::from_raw(self.data));
        }
    }
}
