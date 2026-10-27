/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! WASAPI shared mode backend, the audio engine converts the format, rate and channels.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

use crate::blocking::{self, Transfer};
use crate::{
    Callback, Direction, Error, ErrorCallback, ErrorKind, InputCallback, OutputCallback,
    SampleRate, StreamConfig,
};

#[allow(non_snake_case)]
mod sys {
    use std::ffi::c_void;

    pub(super) type HResult = i32;
    pub(super) type Handle = *mut c_void;
    type Method<Args> = unsafe extern "system" fn(*mut c_void, Args) -> HResult;

    #[repr(C)]
    pub(super) struct Guid(u32, u16, u16, [u8; 8]);

    pub(super) const CLSID_MM_DEVICE_ENUMERATOR: Guid = Guid(
        0xbcde0395,
        0xe52f,
        0x467c,
        [0x8e, 0x3d, 0xc4, 0x57, 0x92, 0x91, 0x69, 0x2e],
    );
    pub(super) const IID_IMM_DEVICE_ENUMERATOR: Guid = Guid(
        0xa95664d2,
        0x9614,
        0x4f35,
        [0xa7, 0x46, 0xde, 0x8d, 0xb6, 0x36, 0x17, 0xe6],
    );
    pub(super) const IID_IAUDIO_CLIENT: Guid = Guid(
        0x1cb9ad4c,
        0xdbfa,
        0x4c32,
        [0xb1, 0x78, 0xc2, 0xf5, 0x68, 0xa7, 0x03, 0xb2],
    );
    pub(super) const IID_IAUDIO_RENDER_CLIENT: Guid = Guid(
        0xf294acfc,
        0x3146,
        0x4483,
        [0xa7, 0xbf, 0xad, 0xdc, 0xa7, 0xc2, 0x60, 0xe2],
    );
    pub(super) const IID_IAUDIO_CAPTURE_CLIENT: Guid = Guid(
        0xc8adbd64,
        0xe71e,
        0x48a0,
        [0xa4, 0xde, 0x18, 0x5c, 0x39, 0x5c, 0xd3, 0x17],
    );
    pub(super) const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: Guid = Guid(
        0x00000003,
        0x0000,
        0x0010,
        [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71],
    );

    pub(super) const COINIT_MULTITHREADED: u32 = 0;
    pub(super) const CLSCTX_ALL: u32 = 0x17;
    pub(super) const E_RENDER: i32 = 0;
    pub(super) const E_CAPTURE: i32 = 1;
    pub(super) const E_CONSOLE: i32 = 0;
    pub(super) const AUDCLNT_SHAREMODE_SHARED: i32 = 0;
    pub(super) const AUDCLNT_STREAMFLAGS_EVENTCALLBACK: u32 = 0x0004_0000;
    pub(super) const AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x0800_0000;
    pub(super) const AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x8000_0000;
    pub(super) const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x2;
    pub(super) const WAVE_FORMAT_EXTENSIBLE: u16 = 0xfffe;
    pub(super) const SPEAKER_FRONT_LEFT_RIGHT: u32 = 0x3;
    pub(super) const SPEAKER_FRONT_CENTER: u32 = 0x4;
    pub(super) const E_NOTFOUND: HResult = 0x8007_0490_u32 as HResult;
    pub(super) const AUDCLNT_E_DEVICE_INVALIDATED: HResult = 0x8889_0004_u32 as HResult;
    pub(super) const AUDCLNT_E_UNSUPPORTED_FORMAT: HResult = 0x8889_0008_u32 as HResult;
    pub(super) const AUDCLNT_E_DEVICE_IN_USE: HResult = 0x8889_000a_u32 as HResult;
    pub(super) const WAIT_OBJECT_0: u32 = 0;

    #[repr(C, packed(1))]
    pub(super) struct WaveFormatEx {
        pub(super) format_tag: u16,
        pub(super) channels: u16,
        pub(super) samples_per_sec: u32,
        pub(super) avg_bytes_per_sec: u32,
        pub(super) block_align: u16,
        pub(super) bits_per_sample: u16,
        pub(super) size: u16,
    }

    #[repr(C, packed(1))]
    pub(super) struct WaveFormatExtensible {
        pub(super) format: WaveFormatEx,
        pub(super) valid_bits_per_sample: u16,
        pub(super) channel_mask: u32,
        pub(super) sub_format: Guid,
    }

    // The vtables are truncated after the last used method, unused methods are placeholders
    #[repr(C)]
    pub(super) struct IUnknownVtbl {
        pub(super) QueryInterface: usize,
        pub(super) AddRef: usize,
        pub(super) Release: unsafe extern "system" fn(*mut c_void) -> u32,
    }

    #[repr(C)]
    pub(super) struct IMMDeviceEnumeratorVtbl {
        pub(super) unknown: IUnknownVtbl,
        pub(super) EnumAudioEndpoints: usize,
        pub(super) GetDefaultAudioEndpoint:
            unsafe extern "system" fn(*mut c_void, i32, i32, *mut *mut c_void) -> HResult,
    }

    #[repr(C)]
    pub(super) struct IMMDeviceVtbl {
        pub(super) unknown: IUnknownVtbl,
        pub(super) Activate: unsafe extern "system" fn(
            *mut c_void,
            *const Guid,
            u32,
            *const c_void,
            *mut *mut c_void,
        ) -> HResult,
    }

    #[repr(C)]
    pub(super) struct IAudioClientVtbl {
        pub(super) unknown: IUnknownVtbl,
        pub(super) Initialize: unsafe extern "system" fn(
            *mut c_void,
            i32,
            u32,
            i64,
            i64,
            *const WaveFormatExtensible,
            *const Guid,
        ) -> HResult,
        pub(super) GetBufferSize: Method<*mut u32>,
        pub(super) GetStreamLatency: usize,
        pub(super) GetCurrentPadding: Method<*mut u32>,
        pub(super) IsFormatSupported: usize,
        pub(super) GetMixFormat: Method<*mut *mut WaveFormatEx>,
        pub(super) GetDevicePeriod: usize,
        pub(super) Start: unsafe extern "system" fn(*mut c_void) -> HResult,
        pub(super) Stop: unsafe extern "system" fn(*mut c_void) -> HResult,
        pub(super) Reset: unsafe extern "system" fn(*mut c_void) -> HResult,
        pub(super) SetEventHandle: Method<Handle>,
        pub(super) GetService:
            unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HResult,
    }

    #[repr(C)]
    pub(super) struct IAudioRenderClientVtbl {
        pub(super) unknown: IUnknownVtbl,
        pub(super) GetBuffer: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u8) -> HResult,
        pub(super) ReleaseBuffer: unsafe extern "system" fn(*mut c_void, u32, u32) -> HResult,
    }

    #[repr(C)]
    pub(super) struct IAudioCaptureClientVtbl {
        pub(super) unknown: IUnknownVtbl,
        pub(super) GetBuffer: unsafe extern "system" fn(
            *mut c_void,
            *mut *mut u8,
            *mut u32,
            *mut u32,
            *mut u64,
            *mut u64,
        ) -> HResult,
        pub(super) ReleaseBuffer: Method<u32>,
        pub(super) GetNextPacketSize: Method<*mut u32>,
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        pub(super) fn CoInitializeEx(reserved: *const c_void, flags: u32) -> HResult;
        pub(super) fn CoUninitialize();
        pub(super) fn CoCreateInstance(
            clsid: *const Guid,
            outer: *mut c_void,
            context: u32,
            iid: *const Guid,
            object: *mut *mut c_void,
        ) -> HResult;
        pub(super) fn CoTaskMemFree(pv: *mut c_void);
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub(super) fn CreateEventW(
            attributes: *const c_void,
            manual_reset: i32,
            initial_state: i32,
            name: *const u16,
        ) -> Handle;
        pub(super) fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        pub(super) fn CloseHandle(handle: Handle) -> i32;
    }
}

const WAIT_MILLISECONDS: u32 = 100;

fn check(result: sys::HResult) -> Result<(), Error> {
    if result >= 0 {
        return Ok(());
    }
    let kind = match result {
        sys::AUDCLNT_E_DEVICE_IN_USE => ErrorKind::DeviceBusy,
        sys::E_NOTFOUND | sys::AUDCLNT_E_DEVICE_INVALIDATED => ErrorKind::DeviceNotAvailable,
        sys::AUDCLNT_E_UNSUPPORTED_FORMAT => ErrorKind::UnsupportedConfig,
        _ => ErrorKind::BackendError,
    };
    Err(Error::with_message(
        kind,
        format!("WASAPI error {:#010x}", result as u32),
    ))
}

// MARK: COM
/// Initializes COM on the current thread until dropped
struct ComGuard(bool);

impl ComGuard {
    fn new() -> Self {
        // SAFETY: COM may already be initialized, then it isn't uninitialized on drop.
        Self(unsafe { sys::CoInitializeEx(ptr::null(), sys::COINIT_MULTITHREADED) } >= 0)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: COM was initialized by this guard on this thread.
            unsafe { sys::CoUninitialize() };
        }
    }
}

/// An owned COM object with vtable `V`
struct Com<V>(NonNull<c_void>, PhantomData<V>);

impl<V> Com<V> {
    /// Takes the object that `create` writes to its output pointer
    fn new(create: impl FnOnce(*mut *mut c_void) -> sys::HResult) -> Result<Self, Error> {
        let mut object = ptr::null_mut();
        check(create(&mut object))?;
        NonNull::new(object)
            .map(|object| Self(object, PhantomData))
            .ok_or_else(|| Error::new(ErrorKind::BackendError))
    }

    const fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    fn vtable(&self) -> &V {
        // SAFETY: a COM object starts with a pointer to its vtable, which outlives the object.
        unsafe { &**self.0.as_ptr().cast::<*const V>() }
    }
}

impl<V> Drop for Com<V> {
    fn drop(&mut self) {
        // SAFETY: every vtable starts with the IUnknown methods and the object is released once.
        unsafe {
            let unknown = &**self.0.as_ptr().cast::<*const sys::IUnknownVtbl>();
            (unknown.Release)(self.as_ptr());
        }
    }
}

impl Com<sys::IAudioClientVtbl> {
    /// Gets a service of an initialized audio client
    fn service<V>(&self, iid: &sys::Guid) -> Result<Com<V>, Error> {
        // SAFETY: the client is initialized and the service is written to the pointer.
        Com::new(|object| unsafe { (self.vtable().GetService)(self.as_ptr(), iid, object) })
    }
}

/// Activates an audio client on the default endpoint of the direction, COM must be initialized
fn audio_client(direction: Direction) -> Result<Com<sys::IAudioClientVtbl>, Error> {
    let flow = match direction {
        Direction::Input => sys::E_CAPTURE,
        Direction::Output => sys::E_RENDER,
    };
    // SAFETY: the GUIDs are valid and each object is written to its output pointer.
    unsafe {
        let enumerator = Com::<sys::IMMDeviceEnumeratorVtbl>::new(|object| {
            sys::CoCreateInstance(
                &sys::CLSID_MM_DEVICE_ENUMERATOR,
                ptr::null_mut(),
                sys::CLSCTX_ALL,
                &sys::IID_IMM_DEVICE_ENUMERATOR,
                object,
            )
        })?;
        let device = Com::<sys::IMMDeviceVtbl>::new(|object| {
            (enumerator.vtable().GetDefaultAudioEndpoint)(
                enumerator.as_ptr(),
                flow,
                sys::E_CONSOLE,
                object,
            )
        })?;
        Com::new(|object| {
            (device.vtable().Activate)(
                device.as_ptr(),
                &sys::IID_IAUDIO_CLIENT,
                sys::CLSCTX_ALL,
                ptr::null(),
                object,
            )
        })
    }
}

// MARK: Device
pub(crate) struct Device(Direction);

pub(crate) fn default_device(direction: Direction) -> Option<Device> {
    let _com = ComGuard::new();
    audio_client(direction).ok().map(|_| Device(direction))
}

pub(crate) type Stream = blocking::Stream;

impl Device {
    /// The sample rate of the shared mode mix format
    pub(crate) fn default_sample_rate(&self) -> Result<SampleRate, Error> {
        let _com = ComGuard::new();
        let client = audio_client(self.0)?;
        let mut format = ptr::null_mut();
        // SAFETY: the client is live, the returned format is freed with CoTaskMemFree.
        unsafe {
            check((client.vtable().GetMixFormat)(client.as_ptr(), &mut format))?;
            let rate = (*format).samples_per_sec;
            sys::CoTaskMemFree(format.cast());
            Ok(rate)
        }
    }

    pub(crate) fn build_stream(
        &self,
        config: &StreamConfig,
        callback: Callback,
        error: ErrorCallback,
    ) -> Result<Stream, Error> {
        let config = *config;
        // COM objects are created and used on the stream thread only
        Stream::spawn(move || Wasapi::open(&config, callback), error)
    }
}

// MARK: Stream
enum Service {
    Capture(Com<sys::IAudioCaptureClientVtbl>, InputCallback, Vec<f32>),
    Render(Com<sys::IAudioRenderClientVtbl>, OutputCallback),
}

struct Wasapi {
    service: Service,
    client: Com<sys::IAudioClientVtbl>,
    event: sys::Handle,
    buffer_frames: u32,
    channels: usize,
    /// Dropped last, after all COM objects are released
    _com: ComGuard,
}

impl Wasapi {
    fn open(config: &StreamConfig, callback: Callback) -> Result<Self, Error> {
        let com = ComGuard::new();
        let client = audio_client(callback.direction())?;

        let block_align = config.channels * size_of::<f32>() as u16;
        let format = sys::WaveFormatExtensible {
            format: sys::WaveFormatEx {
                format_tag: sys::WAVE_FORMAT_EXTENSIBLE,
                channels: config.channels,
                samples_per_sec: config.sample_rate,
                avg_bytes_per_sec: config.sample_rate * block_align as u32,
                block_align,
                bits_per_sample: 32,
                size: (size_of::<sys::WaveFormatExtensible>() - size_of::<sys::WaveFormatEx>())
                    as u16,
            },
            valid_bits_per_sample: 32,
            channel_mask: match config.channels {
                1 => sys::SPEAKER_FRONT_CENTER,
                2 => sys::SPEAKER_FRONT_LEFT_RIGHT,
                _ => 0,
            },
            sub_format: sys::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        };
        // Buffer duration in 100 nanosecond units
        let duration = config.buffer_frames() as i64 * 10_000_000 / config.sample_rate as i64;

        let client_vtable = client.vtable();
        let mut buffer_frames = 0;
        // SAFETY: the client is live and all pointers are valid for the calls, the event outlives
        // the client because it is closed on drop after stopping the client.
        let event = unsafe {
            check((client_vtable.Initialize)(
                client.as_ptr(),
                sys::AUDCLNT_SHAREMODE_SHARED,
                sys::AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | sys::AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | sys::AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                duration,
                0,
                &format,
                ptr::null(),
            ))?;
            check((client_vtable.GetBufferSize)(
                client.as_ptr(),
                &mut buffer_frames,
            ))?;
            let event = sys::CreateEventW(ptr::null(), 0, 0, ptr::null());
            if event.is_null() {
                return Err(Error::new(ErrorKind::BackendError));
            }
            if let Err(err) = check((client_vtable.SetEventHandle)(client.as_ptr(), event)) {
                sys::CloseHandle(event);
                return Err(err);
            }
            event
        };

        let service = match callback {
            Callback::Input(data) => client
                .service(&sys::IID_IAUDIO_CAPTURE_CLIENT)
                .map(|capture| Service::Capture(capture, data, Vec::new())),
            Callback::Output(data) => client
                .service(&sys::IID_IAUDIO_RENDER_CLIENT)
                .map(|render| Service::Render(render, data)),
        };
        let service = match service {
            Ok(service) => service,
            Err(err) => {
                // SAFETY: the event was created above and the client is not started.
                unsafe { sys::CloseHandle(event) };
                return Err(err);
            }
        };
        Ok(Self {
            service,
            client,
            event,
            buffer_frames,
            channels: config.channels as usize,
            _com: com,
        })
    }

    /// Plays the frames the client has room for
    fn render(
        client: &Com<sys::IAudioClientVtbl>,
        render: &Com<sys::IAudioRenderClientVtbl>,
        data: &mut OutputCallback,
        buffer_frames: u32,
        channels: usize,
    ) -> Result<(), Error> {
        let mut padding = 0;
        // SAFETY: the client is live.
        check(unsafe { (client.vtable().GetCurrentPadding)(client.as_ptr(), &mut padding) })?;
        let frames = buffer_frames - padding;
        if frames == 0 {
            return Ok(());
        }
        let mut buffer = ptr::null_mut();
        // SAFETY: the render client returns a buffer of `frames` frames of the stream format,
        // which is ours until it is released.
        unsafe {
            check((render.vtable().GetBuffer)(
                render.as_ptr(),
                frames,
                &mut buffer,
            ))?;
            data(std::slice::from_raw_parts_mut(
                buffer.cast::<f32>(),
                frames as usize * channels,
            ));
            check((render.vtable().ReleaseBuffer)(render.as_ptr(), frames, 0))
        }
    }

    /// Reads all recorded packets
    fn capture(
        capture: &Com<sys::IAudioCaptureClientVtbl>,
        data: &mut InputCallback,
        samples: &mut Vec<f32>,
        channels: usize,
    ) -> Result<(), Error> {
        let vtable = capture.vtable();
        loop {
            let mut packet_frames = 0;
            // SAFETY: the capture client is live.
            check(unsafe { (vtable.GetNextPacketSize)(capture.as_ptr(), &mut packet_frames) })?;
            if packet_frames == 0 {
                return Ok(());
            }

            let mut buffer = ptr::null_mut();
            let mut frames = 0;
            let mut flags = 0;
            samples.clear();
            // SAFETY: the capture client returns a buffer of `frames` frames of the stream format,
            // which is copied before it is released.
            unsafe {
                check((vtable.GetBuffer)(
                    capture.as_ptr(),
                    &mut buffer,
                    &mut frames,
                    &mut flags,
                    ptr::null_mut(),
                    ptr::null_mut(),
                ))?;
                let length = frames as usize * channels;
                if flags & sys::AUDCLNT_BUFFERFLAGS_SILENT == 0 {
                    samples.extend_from_slice(std::slice::from_raw_parts(
                        buffer.cast::<f32>(),
                        length,
                    ));
                } else {
                    samples.resize(length, 0.0);
                }
                check((vtable.ReleaseBuffer)(capture.as_ptr(), frames))?;
            }
            if !samples.is_empty() {
                data(samples);
            }
        }
    }
}

impl Drop for Wasapi {
    fn drop(&mut self) {
        // SAFETY: the client is stopped before its event is closed.
        unsafe {
            (self.client.vtable().Stop)(self.client.as_ptr());
            sys::CloseHandle(self.event);
        }
    }
}

impl Transfer for Wasapi {
    fn start(&mut self) -> Result<(), Error> {
        // SAFETY: the client is initialized.
        check(unsafe { (self.client.vtable().Start)(self.client.as_ptr()) })
    }

    /// Stops and discards the buffered audio
    fn stop(&mut self) -> Result<(), Error> {
        let vtable = self.client.vtable();
        // SAFETY: the client is initialized.
        unsafe {
            check((vtable.Stop)(self.client.as_ptr()))?;
            check((vtable.Reset)(self.client.as_ptr()))
        }
    }

    fn transfer(&mut self) -> Result<(), Error> {
        // SAFETY: the event is open, a timeout lets the stream thread see state changes.
        if unsafe { sys::WaitForSingleObject(self.event, WAIT_MILLISECONDS) } != sys::WAIT_OBJECT_0
        {
            return Ok(());
        }
        match &mut self.service {
            Service::Capture(capture, data, samples) => {
                Self::capture(capture, data, samples, self.channels)
            }
            Service::Render(render, data) => Self::render(
                &self.client,
                render,
                data,
                self.buffer_frames,
                self.channels,
            ),
        }
    }
}
