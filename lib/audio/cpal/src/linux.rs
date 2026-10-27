/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! PulseAudio backend, which also serves PipeWire through pipewire-pulse, with an ALSA fallback.
//! Both libraries are loaded at runtime and convert the format, rate and channels. PulseAudio
//! calls back from its own mainloop thread, ALSA blocks on a stream thread.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_int, c_uint, c_ulong, c_void};
use std::ptr::{self, NonNull};
use std::sync::OnceLock;

use crate::blocking::{self, Transfer};
use crate::{Callback, Direction, Error, ErrorCallback, ErrorKind, SampleRate, StreamConfig};

mod sys {
    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};

    const RTLD_NOW: c_int = 2;

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(library: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    /// Declares a library whose functions are loaded at runtime, it stays loaded once found
    macro_rules! library {
        ($name:ident, $file:literal, {
            $(fn $function:ident($($arg:ident: $type:ty),* $(,)?) $(-> $ret:ty)?;)*
        }) => {
            #[allow(non_snake_case)]
            pub(super) struct $name {
                $(pub(super) $function: unsafe extern "C" fn($($arg: $type),*) $(-> $ret)?,)*
            }

            impl $name {
                pub(super) fn load() -> Option<Self> {
                    // SAFETY: the library name is NUL-terminated.
                    let library = unsafe { dlopen($file.as_ptr(), RTLD_NOW) };
                    if library.is_null() {
                        return None;
                    }
                    Some(Self {
                        $($function: {
                            // SAFETY: the symbol name is NUL-terminated and the library is open.
                            let symbol = unsafe {
                                dlsym(library, concat!(stringify!($function), "\0").as_ptr().cast())
                            };
                            if symbol.is_null() {
                                return None;
                            }
                            // SAFETY: the symbol is a function with the declared C signature.
                            unsafe {
                                std::mem::transmute::<
                                    *mut c_void,
                                    unsafe extern "C" fn($($arg: $type),*) $(-> $ret)?,
                                >(symbol)
                            }
                        },)*
                    })
                }
            }
        };
    }

    // MARK: PulseAudio
    pub(super) const PA_CONTEXT_NOAUTOSPAWN: c_int = 1;
    pub(super) const PA_CONTEXT_READY: c_int = 4;
    pub(super) const PA_CONTEXT_FAILED: c_int = 5;
    pub(super) const PA_CONTEXT_TERMINATED: c_int = 6;
    pub(super) const PA_STREAM_READY: c_int = 2;
    pub(super) const PA_STREAM_FAILED: c_int = 3;
    pub(super) const PA_STREAM_TERMINATED: c_int = 4;
    pub(super) const PA_STREAM_START_CORKED: c_int = 0x0001;
    pub(super) const PA_STREAM_ADJUST_LATENCY: c_int = 0x2000;
    pub(super) const PA_OPERATION_RUNNING: c_int = 0;
    pub(super) const PA_SEEK_RELATIVE: c_int = 0;
    pub(super) const PA_SAMPLE_FLOAT32LE: c_int = 5;
    pub(super) const PA_ERR_INVALID: c_int = 3;
    pub(super) const PA_ERR_NOENTITY: c_int = 5;
    pub(super) const PA_ERR_CONNECTIONREFUSED: c_int = 6;
    pub(super) const PA_ERR_BUSY: c_int = 10;
    pub(super) const PA_ERR_NOTSUPPORTED: c_int = 19;

    pub(super) type NotifyCallback =
        unsafe extern "C" fn(object: *mut c_void, user_data: *mut c_void);
    pub(super) type RequestCallback =
        unsafe extern "C" fn(stream: *mut c_void, bytes: usize, user_data: *mut c_void);
    pub(super) type SuccessCallback =
        unsafe extern "C" fn(stream: *mut c_void, success: c_int, user_data: *mut c_void);
    pub(super) type ServerInfoCallback = unsafe extern "C" fn(
        context: *mut c_void,
        info: *const PaServerInfo,
        user_data: *mut c_void,
    );

    #[repr(C)]
    pub(super) struct PaSampleSpec {
        pub(super) format: c_int,
        pub(super) rate: u32,
        pub(super) channels: u8,
    }

    #[repr(C)]
    pub(super) struct PaBufferAttr {
        pub(super) maxlength: u32,
        pub(super) tlength: u32,
        pub(super) prebuf: u32,
        pub(super) minreq: u32,
        pub(super) fragsize: u32,
    }

    /// The start of the server info, the remaining fields are not used
    #[repr(C)]
    pub(super) struct PaServerInfo {
        pub(super) user_name: *const c_char,
        pub(super) host_name: *const c_char,
        pub(super) server_version: *const c_char,
        pub(super) server_name: *const c_char,
        pub(super) sample_spec: PaSampleSpec,
    }

    library!(Pulse, c"libpulse.so.0", {
        fn pa_threaded_mainloop_new() -> *mut c_void;
        fn pa_threaded_mainloop_free(mainloop: *mut c_void);
        fn pa_threaded_mainloop_start(mainloop: *mut c_void) -> c_int;
        fn pa_threaded_mainloop_stop(mainloop: *mut c_void);
        fn pa_threaded_mainloop_lock(mainloop: *mut c_void);
        fn pa_threaded_mainloop_unlock(mainloop: *mut c_void);
        fn pa_threaded_mainloop_wait(mainloop: *mut c_void);
        fn pa_threaded_mainloop_signal(mainloop: *mut c_void, wait_for_accept: c_int);
        fn pa_threaded_mainloop_get_api(mainloop: *mut c_void) -> *mut c_void;
        fn pa_context_new(api: *mut c_void, name: *const c_char) -> *mut c_void;
        fn pa_context_set_state_callback(
            context: *mut c_void,
            callback: Option<NotifyCallback>,
            user_data: *mut c_void,
        );
        fn pa_context_connect(
            context: *mut c_void,
            server: *const c_char,
            flags: c_int,
            api: *const c_void,
        ) -> c_int;
        fn pa_context_get_state(context: *mut c_void) -> c_int;
        fn pa_context_errno(context: *mut c_void) -> c_int;
        fn pa_context_disconnect(context: *mut c_void);
        fn pa_context_unref(context: *mut c_void);
        fn pa_context_get_server_info(
            context: *mut c_void,
            callback: ServerInfoCallback,
            user_data: *mut c_void,
        ) -> *mut c_void;
        fn pa_operation_get_state(operation: *mut c_void) -> c_int;
        fn pa_operation_unref(operation: *mut c_void);
        fn pa_stream_new(
            context: *mut c_void,
            name: *const c_char,
            spec: *const PaSampleSpec,
            map: *const c_void,
        ) -> *mut c_void;
        fn pa_stream_set_state_callback(
            stream: *mut c_void,
            callback: Option<NotifyCallback>,
            user_data: *mut c_void,
        );
        fn pa_stream_set_write_callback(
            stream: *mut c_void,
            callback: Option<RequestCallback>,
            user_data: *mut c_void,
        );
        fn pa_stream_set_read_callback(
            stream: *mut c_void,
            callback: Option<RequestCallback>,
            user_data: *mut c_void,
        );
        fn pa_stream_connect_playback(
            stream: *mut c_void,
            device: *const c_char,
            attr: *const PaBufferAttr,
            flags: c_int,
            volume: *const c_void,
            sync_stream: *mut c_void,
        ) -> c_int;
        fn pa_stream_connect_record(
            stream: *mut c_void,
            device: *const c_char,
            attr: *const PaBufferAttr,
            flags: c_int,
        ) -> c_int;
        fn pa_stream_get_state(stream: *mut c_void) -> c_int;
        fn pa_stream_get_context(stream: *mut c_void) -> *mut c_void;
        fn pa_stream_begin_write(
            stream: *mut c_void,
            data: *mut *mut c_void,
            bytes: *mut usize,
        ) -> c_int;
        fn pa_stream_write(
            stream: *mut c_void,
            data: *const c_void,
            bytes: usize,
            free: *const c_void,
            offset: i64,
            seek: c_int,
        ) -> c_int;
        fn pa_stream_peek(
            stream: *mut c_void,
            data: *mut *const c_void,
            bytes: *mut usize,
        ) -> c_int;
        fn pa_stream_drop(stream: *mut c_void) -> c_int;
        fn pa_stream_cancel_write(stream: *mut c_void) -> c_int;
        fn pa_stream_cork(
            stream: *mut c_void,
            cork: c_int,
            callback: SuccessCallback,
            user_data: *mut c_void,
        ) -> *mut c_void;
        fn pa_stream_disconnect(stream: *mut c_void) -> c_int;
        fn pa_stream_unref(stream: *mut c_void);
        fn pa_strerror(error: c_int) -> *const c_char;
    });

    // MARK: ALSA
    pub(super) const SND_PCM_STREAM_PLAYBACK: c_int = 0;
    pub(super) const SND_PCM_STREAM_CAPTURE: c_int = 1;
    pub(super) const SND_PCM_FORMAT_FLOAT_LE: c_int = 14;
    pub(super) const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;

    library!(Alsa, c"libasound.so.2", {
        fn snd_pcm_open(
            pcm: *mut *mut c_void,
            name: *const c_char,
            stream: c_int,
            mode: c_int,
        ) -> c_int;
        fn snd_pcm_close(pcm: *mut c_void) -> c_int;
        fn snd_pcm_set_params(
            pcm: *mut c_void,
            format: c_int,
            access: c_int,
            channels: c_uint,
            rate: c_uint,
            soft_resample: c_int,
            latency: c_uint,
        ) -> c_int;
        fn snd_pcm_prepare(pcm: *mut c_void) -> c_int;
        fn snd_pcm_drop(pcm: *mut c_void) -> c_int;
        fn snd_pcm_readi(pcm: *mut c_void, buffer: *mut c_void, size: c_ulong) -> c_long;
        fn snd_pcm_writei(pcm: *mut c_void, buffer: *const c_void, size: c_ulong) -> c_long;
        fn snd_pcm_recover(pcm: *mut c_void, err: c_int, silent: c_int) -> c_int;
        fn snd_strerror(errnum: c_int) -> *const c_char;
    });
}

/// Sample rate requested from ALSA, which converts any rate
const ALSA_SAMPLE_RATE: SampleRate = 48_000;

fn pulse() -> Option<&'static sys::Pulse> {
    static PULSE: OnceLock<Option<sys::Pulse>> = OnceLock::new();
    PULSE.get_or_init(sys::Pulse::load).as_ref()
}

fn alsa() -> Option<&'static sys::Alsa> {
    static ALSA: OnceLock<Option<sys::Alsa>> = OnceLock::new();
    ALSA.get_or_init(sys::Alsa::load).as_ref()
}

// MARK: Device
pub(crate) struct Device;

pub(crate) fn default_device(_direction: Direction) -> Option<Device> {
    (pulse().is_some() || alsa().is_some()).then_some(Device)
}

pub(crate) enum Stream {
    Pulse(PulseStream),
    Alsa(blocking::Stream),
}

impl Stream {
    pub(crate) fn play(&self) -> Result<(), Error> {
        match self {
            Self::Pulse(stream) => stream.cork(false),
            Self::Alsa(stream) => stream.play(),
        }
    }

    pub(crate) fn pause(&self) -> Result<(), Error> {
        match self {
            Self::Pulse(stream) => stream.cork(true),
            Self::Alsa(stream) => stream.pause(),
        }
    }
}

impl Device {
    /// The sample rate of the PulseAudio server, or the rate requested from ALSA
    pub(crate) fn default_sample_rate(&self) -> Result<SampleRate, Error> {
        Ok(pulse()
            .and_then(|lib| Connection::new(lib).ok())
            .and_then(|connection| connection.server_sample_rate().ok())
            .unwrap_or(ALSA_SAMPLE_RATE))
    }

    /// Connects to PulseAudio and falls back to ALSA when there is no server
    pub(crate) fn build_stream(
        &self,
        config: &StreamConfig,
        callback: Callback,
        error: ErrorCallback,
    ) -> Result<Stream, Error> {
        let connection = pulse().map(Connection::new);
        match connection {
            Some(Ok(connection)) => {
                PulseStream::new(connection, config, callback, error).map(Stream::Pulse)
            }
            Some(Err(err)) if alsa().is_none() => Err(err),
            _ => {
                let alsa = alsa().ok_or_else(|| {
                    Error::with_message(
                        ErrorKind::DeviceNotAvailable,
                        "Neither libpulse.so.0 nor libasound.so.2 can be loaded",
                    )
                })?;
                let pcm = AlsaPcm::open(alsa, callback, config)?;
                blocking::Stream::spawn(move || Ok(pcm), error).map(Stream::Alsa)
            }
        }
    }
}

// MARK: PulseAudio
fn pulse_error(lib: &sys::Pulse, error: c_int) -> Error {
    // SAFETY: pa_strerror returns a static NUL-terminated string for any error code.
    let message = unsafe { CStr::from_ptr((lib.pa_strerror)(error)) }
        .to_string_lossy()
        .into_owned();
    let kind = match error {
        sys::PA_ERR_BUSY => ErrorKind::DeviceBusy,
        sys::PA_ERR_NOENTITY | sys::PA_ERR_CONNECTIONREFUSED => ErrorKind::DeviceNotAvailable,
        sys::PA_ERR_INVALID | sys::PA_ERR_NOTSUPPORTED => ErrorKind::UnsupportedConfig,
        _ => ErrorKind::BackendError,
    };
    Error::with_message(kind, message)
}

/// Wakes the threads that wait on the mainloop when an operation succeeded or failed
unsafe extern "C" fn success_callback(
    _stream: *mut c_void,
    _success: c_int,
    mainloop: *mut c_void,
) {
    // SAFETY: the user data is the mainloop.
    unsafe { signal_callback(ptr::null_mut(), mainloop) };
}

/// Wakes the threads that wait on the mainloop, the user data is the mainloop
unsafe extern "C" fn signal_callback(_object: *mut c_void, mainloop: *mut c_void) {
    if let Some(lib) = pulse() {
        // SAFETY: the mainloop outlives every object whose callback signals it.
        unsafe { (lib.pa_threaded_mainloop_signal)(mainloop, 0) };
    }
}

/// A threaded mainloop with a connected context, the mainloop thread runs all callbacks
struct Connection {
    lib: &'static sys::Pulse,
    mainloop: *mut c_void,
    context: *mut c_void,
}

/// Holds the mainloop lock until dropped
struct Lock<'a>(&'a Connection);

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        // SAFETY: the lock is held by this guard.
        unsafe { (self.0.lib.pa_threaded_mainloop_unlock)(self.0.mainloop) };
    }
}

impl Connection {
    fn new(lib: &'static sys::Pulse) -> Result<Self, Error> {
        // The application name is shown in the volume mixer
        let name = std::env::current_exe()
            .ok()
            .and_then(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
            .and_then(|name| CString::new(name).ok())
            .unwrap_or_else(|| c"cpal".to_owned());

        // SAFETY: the mainloop and context are checked, owned by the connection and freed on drop.
        unsafe {
            let mainloop = (lib.pa_threaded_mainloop_new)();
            if mainloop.is_null() {
                return Err(Error::new(ErrorKind::BackendError));
            }
            let connection = Self {
                lib,
                mainloop,
                context: (lib.pa_context_new)(
                    (lib.pa_threaded_mainloop_get_api)(mainloop),
                    name.as_ptr(),
                ),
            };
            if connection.context.is_null() {
                return Err(Error::new(ErrorKind::BackendError));
            }
            (lib.pa_context_set_state_callback)(
                connection.context,
                Some(signal_callback),
                mainloop,
            );
            // Don't start a server, so ALSA is used when none is running
            if (lib.pa_context_connect)(
                connection.context,
                ptr::null(),
                sys::PA_CONTEXT_NOAUTOSPAWN,
                ptr::null(),
            ) < 0
            {
                return Err(connection.context_error());
            }

            let lock = connection.lock();
            if (lib.pa_threaded_mainloop_start)(mainloop) < 0 {
                return Err(Error::new(ErrorKind::BackendError));
            }
            lock.wait_until(|| match (lib.pa_context_get_state)(connection.context) {
                sys::PA_CONTEXT_READY => Ok(true),
                sys::PA_CONTEXT_FAILED | sys::PA_CONTEXT_TERMINATED => {
                    Err(connection.context_error())
                }
                _ => Ok(false),
            })?;
            drop(lock);
            Ok(connection)
        }
    }

    fn lock(&self) -> Lock<'_> {
        // SAFETY: the mainloop is live, the lock is recursive and released by the guard.
        unsafe { (self.lib.pa_threaded_mainloop_lock)(self.mainloop) };
        Lock(self)
    }

    fn context_error(&self) -> Error {
        // SAFETY: the context is live.
        pulse_error(self.lib, unsafe {
            (self.lib.pa_context_errno)(self.context)
        })
    }

    fn server_sample_rate(&self) -> Result<SampleRate, Error> {
        struct Info {
            mainloop: *mut c_void,
            rate: SampleRate,
        }

        unsafe extern "C" fn info_callback(
            _context: *mut c_void,
            info: *const sys::PaServerInfo,
            user_data: *mut c_void,
        ) {
            // SAFETY: the user data is the info of the waiting thread, the server info is valid
            // or null.
            unsafe {
                let result = &mut *user_data.cast::<Info>();
                if let Some(info) = info.as_ref() {
                    result.rate = info.sample_spec.rate;
                }
                signal_callback(ptr::null_mut(), result.mainloop);
            }
        }

        let mut info = Info {
            mainloop: self.mainloop,
            rate: 0,
        };
        let lock = self.lock();
        // SAFETY: the context is ready and `info` outlives the operation, which is awaited.
        let operation = unsafe {
            (self.lib.pa_context_get_server_info)(
                self.context,
                info_callback,
                (&raw mut info).cast(),
            )
        };
        lock.wait_operation(operation)?;
        if info.rate == 0 {
            return Err(self.context_error());
        }
        Ok(info.rate)
    }
}

impl Lock<'_> {
    /// Waits for mainloop signals until `done` returns true or fails
    fn wait_until(&self, mut done: impl FnMut() -> Result<bool, Error>) -> Result<(), Error> {
        while !done()? {
            // SAFETY: the lock is held, waiting releases it until a callback signals.
            unsafe { (self.0.lib.pa_threaded_mainloop_wait)(self.0.mainloop) };
        }
        Ok(())
    }

    /// Waits for an operation to complete and releases it
    fn wait_operation(&self, operation: *mut c_void) -> Result<(), Error> {
        let lib = self.0.lib;
        if operation.is_null() {
            return Err(self.0.context_error());
        }
        // The operation callback signals when it is done, a failed context signals as well
        let result = self.wait_until(|| {
            // SAFETY: the operation is live until it is released and the context is live.
            let (done, ready) = unsafe {
                (
                    (lib.pa_operation_get_state)(operation) != sys::PA_OPERATION_RUNNING,
                    (lib.pa_context_get_state)(self.0.context) == sys::PA_CONTEXT_READY,
                )
            };
            if done || ready {
                Ok(done)
            } else {
                Err(self.0.context_error())
            }
        });
        // SAFETY: the operation is released once.
        unsafe { (lib.pa_operation_unref)(operation) };
        result
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: stopping the mainloop ends all callbacks, then the context and mainloop are
        // freed once.
        unsafe {
            (self.lib.pa_threaded_mainloop_stop)(self.mainloop);
            if !self.context.is_null() {
                (self.lib.pa_context_disconnect)(self.context);
                (self.lib.pa_context_unref)(self.context);
            }
            (self.lib.pa_threaded_mainloop_free)(self.mainloop);
        }
    }
}

/// The state used by the stream callbacks, they all run on the mainloop thread
struct Callbacks {
    lib: &'static sys::Pulse,
    mainloop: *mut c_void,
    data: Callback,
    error: ErrorCallback,
    frame_bytes: usize,
    /// Failures are only reported to the error callback after the stream was built
    built: bool,
}

pub(crate) struct PulseStream {
    stream: *mut c_void,
    callbacks: *mut Callbacks,
    /// Dropped after the stream is released
    connection: Connection,
}

// SAFETY: the stream is only used with the mainloop lock held and its callbacks run on the
// mainloop thread.
unsafe impl Send for PulseStream {}
// SAFETY: the same mainloop locking permits shared references.
unsafe impl Sync for PulseStream {}

impl PulseStream {
    fn new(
        connection: Connection,
        config: &StreamConfig,
        callback: Callback,
        error: ErrorCallback,
    ) -> Result<Self, Error> {
        let lib = connection.lib;
        let spec = sys::PaSampleSpec {
            format: sys::PA_SAMPLE_FLOAT32LE,
            rate: config.sample_rate,
            channels: u8::try_from(config.channels)
                .map_err(|_| Error::new(ErrorKind::UnsupportedConfig))?,
        };
        let frame_bytes = config.channels as usize * size_of::<f32>();
        let buffer_bytes = (config.buffer_frames() * frame_bytes) as u32;
        let direction = callback.direction();

        let lock = connection.lock();
        let name = match direction {
            Direction::Input => c"Record",
            Direction::Output => c"Playback",
        };
        // SAFETY: the context is ready and the lock is held.
        let stream =
            unsafe { (lib.pa_stream_new)(connection.context, name.as_ptr(), &spec, ptr::null()) };
        if stream.is_null() {
            return Err(connection.context_error());
        }
        let callbacks = Box::into_raw(Box::new(Callbacks {
            lib,
            mainloop: connection.mainloop,
            data: callback,
            error,
            frame_bytes,
            built: false,
        }));
        drop(lock);
        let stream = Self {
            stream,
            callbacks,
            connection,
        };

        let lock = stream.connection.lock();
        // SAFETY: the stream is unconnected, the lock is held and the callbacks stay alive until
        // they are removed on drop.
        unsafe {
            (lib.pa_stream_set_state_callback)(
                stream.stream,
                Some(stream_state_callback),
                callbacks.cast(),
            );
            let flags = sys::PA_STREAM_START_CORKED | sys::PA_STREAM_ADJUST_LATENCY;
            let result = match direction {
                Direction::Input => {
                    (lib.pa_stream_set_read_callback)(
                        stream.stream,
                        Some(read_callback),
                        callbacks.cast(),
                    );
                    let attr = sys::PaBufferAttr {
                        maxlength: u32::MAX,
                        tlength: u32::MAX,
                        prebuf: u32::MAX,
                        minreq: u32::MAX,
                        fragsize: buffer_bytes,
                    };
                    (lib.pa_stream_connect_record)(stream.stream, ptr::null(), &attr, flags)
                }
                Direction::Output => {
                    (lib.pa_stream_set_write_callback)(
                        stream.stream,
                        Some(write_callback),
                        callbacks.cast(),
                    );
                    let attr = sys::PaBufferAttr {
                        maxlength: u32::MAX,
                        tlength: buffer_bytes * 2,
                        prebuf: u32::MAX,
                        minreq: buffer_bytes,
                        fragsize: u32::MAX,
                    };
                    (lib.pa_stream_connect_playback)(
                        stream.stream,
                        ptr::null(),
                        &attr,
                        flags,
                        ptr::null(),
                        ptr::null_mut(),
                    )
                }
            };
            if result < 0 {
                return Err(stream.connection.context_error());
            }
            lock.wait_until(|| match (lib.pa_stream_get_state)(stream.stream) {
                sys::PA_STREAM_READY => Ok(true),
                sys::PA_STREAM_FAILED | sys::PA_STREAM_TERMINATED => {
                    Err(stream.connection.context_error())
                }
                _ => Ok(false),
            })?;
            (*callbacks).built = true;
        }
        drop(lock);
        Ok(stream)
    }

    /// Corks to pause the stream, or uncorks to play it
    fn cork(&self, cork: bool) -> Result<(), Error> {
        let lock = self.connection.lock();
        // SAFETY: the stream is connected and the lock is held.
        let operation = unsafe {
            (self.connection.lib.pa_stream_cork)(
                self.stream,
                cork as c_int,
                success_callback,
                self.connection.mainloop,
            )
        };
        lock.wait_operation(operation)
    }
}

impl Drop for PulseStream {
    fn drop(&mut self) {
        let lib = self.connection.lib;
        let lock = self.connection.lock();
        // SAFETY: the callbacks are removed with the lock held, so they don't run anymore when
        // their state is freed.
        unsafe {
            (lib.pa_stream_set_state_callback)(self.stream, None, ptr::null_mut());
            (lib.pa_stream_set_read_callback)(self.stream, None, ptr::null_mut());
            (lib.pa_stream_set_write_callback)(self.stream, None, ptr::null_mut());
            (lib.pa_stream_disconnect)(self.stream);
            (lib.pa_stream_unref)(self.stream);
            drop(lock);
            drop(Box::from_raw(self.callbacks));
        }
    }
}

unsafe extern "C" fn stream_state_callback(stream: *mut c_void, user_data: *mut c_void) {
    // SAFETY: the user data is the stream callbacks, which only run on the mainloop thread.
    unsafe {
        let callbacks = &mut *user_data.cast::<Callbacks>();
        let lib = callbacks.lib;
        if callbacks.built && (lib.pa_stream_get_state)(stream) == sys::PA_STREAM_FAILED {
            let context = (lib.pa_stream_get_context)(stream);
            (callbacks.error)(pulse_error(lib, (lib.pa_context_errno)(context)));
        }
        (lib.pa_threaded_mainloop_signal)(callbacks.mainloop, 0);
    }
}

/// Fills the requested bytes in place in the stream's write buffer
unsafe extern "C" fn write_callback(stream: *mut c_void, bytes: usize, user_data: *mut c_void) {
    // SAFETY: the user data is the stream callbacks, which only run on the mainloop thread. The
    // write buffer holds `size` bytes until it is written.
    unsafe {
        let callbacks = &mut *user_data.cast::<Callbacks>();
        let lib = callbacks.lib;
        let Callback::Output(data) = &mut callbacks.data else {
            return;
        };
        let mut buffer = ptr::null_mut();
        let mut size = bytes;
        if (lib.pa_stream_begin_write)(stream, &mut buffer, &mut size) < 0 || buffer.is_null() {
            return;
        }
        size -= size % callbacks.frame_bytes;
        if size == 0 {
            (lib.pa_stream_cancel_write)(stream);
            return;
        }
        data(std::slice::from_raw_parts_mut(
            buffer.cast::<f32>(),
            size / size_of::<f32>(),
        ));
        (lib.pa_stream_write)(stream, buffer, size, ptr::null(), 0, sys::PA_SEEK_RELATIVE);
    }
}

/// Passes all recorded fragments, holes in the recording are skipped
unsafe extern "C" fn read_callback(stream: *mut c_void, _bytes: usize, user_data: *mut c_void) {
    // SAFETY: the user data is the stream callbacks, which only run on the mainloop thread. A
    // peeked fragment holds `size` bytes until it is dropped.
    unsafe {
        let callbacks = &mut *user_data.cast::<Callbacks>();
        let lib = callbacks.lib;
        let Callback::Input(data) = &mut callbacks.data else {
            return;
        };
        loop {
            let mut buffer = ptr::null();
            let mut size = 0;
            if (lib.pa_stream_peek)(stream, &mut buffer, &mut size) < 0 || size == 0 {
                return;
            }
            if !buffer.is_null() {
                data(std::slice::from_raw_parts(
                    buffer.cast::<f32>(),
                    size / size_of::<f32>(),
                ));
            }
            (lib.pa_stream_drop)(stream);
        }
    }
}

// MARK: ALSA
const ALSA_DEVICE_NAME: &CStr = c"default";

const ENOENT: c_int = 2;
const EBUSY: c_int = 16;
const ENODEV: c_int = 19;
const EINVAL: c_int = 22;

fn alsa_check(lib: &sys::Alsa, result: c_int) -> Result<(), Error> {
    if result >= 0 {
        return Ok(());
    }
    // SAFETY: snd_strerror returns a static NUL-terminated string for any error code.
    let message = unsafe { CStr::from_ptr((lib.snd_strerror)(result)) }
        .to_string_lossy()
        .into_owned();
    let kind = match -result {
        EBUSY => ErrorKind::DeviceBusy,
        ENOENT | ENODEV => ErrorKind::DeviceNotAvailable,
        EINVAL => ErrorKind::UnsupportedConfig,
        _ => ErrorKind::BackendError,
    };
    Err(Error::with_message(kind, message))
}

/// A blocking ALSA PCM, it reads or writes a buffer per transfer on the stream thread
struct AlsaPcm {
    lib: &'static sys::Alsa,
    pcm: NonNull<c_void>,
    channels: usize,
    callback: Callback,
    buffer: Vec<f32>,
}

// SAFETY: the PCM handle is only used by one thread at a time.
unsafe impl Send for AlsaPcm {}

impl AlsaPcm {
    fn open(
        lib: &'static sys::Alsa,
        callback: Callback,
        config: &StreamConfig,
    ) -> Result<Self, Error> {
        let stream = match callback.direction() {
            Direction::Input => sys::SND_PCM_STREAM_CAPTURE,
            Direction::Output => sys::SND_PCM_STREAM_PLAYBACK,
        };
        let mut raw = ptr::null_mut();
        // SAFETY: the name is NUL-terminated and `raw` is a valid output pointer.
        alsa_check(lib, unsafe {
            (lib.snd_pcm_open)(&mut raw, ALSA_DEVICE_NAME.as_ptr(), stream, 0)
        })?;
        let pcm = Self {
            lib,
            pcm: NonNull::new(raw).ok_or_else(|| Error::new(ErrorKind::DeviceNotAvailable))?,
            channels: config.channels as usize,
            callback,
            buffer: vec![0.0; config.buffer_frames() * config.channels as usize],
        };

        let latency = config.buffer_frames() as u64 * 1_000_000 / config.sample_rate as u64;
        // SAFETY: the handle is open and all parameters are plain values.
        alsa_check(lib, unsafe {
            (lib.snd_pcm_set_params)(
                pcm.pcm.as_ptr(),
                sys::SND_PCM_FORMAT_FLOAT_LE,
                sys::SND_PCM_ACCESS_RW_INTERLEAVED,
                config.channels as c_uint,
                config.sample_rate,
                1,
                latency as c_uint,
            )
        })?;
        Ok(pcm)
    }

    /// Recovers from underruns, overruns and suspends, other errors end the stream
    fn recover(&self, err: c_int) -> Result<(), Error> {
        // SAFETY: the handle is open and `err` is the error of the last call.
        alsa_check(self.lib, unsafe {
            (self.lib.snd_pcm_recover)(self.pcm.as_ptr(), err, 1)
        })
    }
}

impl Drop for AlsaPcm {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by snd_pcm_open and is closed once.
        unsafe { (self.lib.snd_pcm_close)(self.pcm.as_ptr()) };
    }
}

impl Transfer for AlsaPcm {
    fn start(&mut self) -> Result<(), Error> {
        // SAFETY: the handle is open.
        alsa_check(self.lib, unsafe {
            (self.lib.snd_pcm_prepare)(self.pcm.as_ptr())
        })
    }

    fn stop(&mut self) -> Result<(), Error> {
        // SAFETY: the handle is open.
        alsa_check(self.lib, unsafe {
            (self.lib.snd_pcm_drop)(self.pcm.as_ptr())
        })
    }

    fn transfer(&mut self) -> Result<(), Error> {
        let frames = self.buffer.len() / self.channels;
        match &mut self.callback {
            Callback::Input(data) => {
                // SAFETY: the buffer holds `frames` interleaved frames of the configured format.
                let read = unsafe {
                    (self.lib.snd_pcm_readi)(
                        self.pcm.as_ptr(),
                        self.buffer.as_mut_ptr().cast(),
                        frames as c_ulong,
                    )
                };
                if read < 0 {
                    return self.recover(read as c_int);
                }
                if read > 0 {
                    data(&self.buffer[..read as usize * self.channels]);
                }
                Ok(())
            }
            Callback::Output(data) => {
                data(&mut self.buffer);
                let mut offset = 0;
                while offset < frames {
                    // SAFETY: the buffer holds the remaining interleaved frames of the format.
                    let written = unsafe {
                        (self.lib.snd_pcm_writei)(
                            self.pcm.as_ptr(),
                            self.buffer[offset * self.channels..].as_ptr().cast(),
                            (frames - offset) as c_ulong,
                        )
                    };
                    if written < 0 {
                        self.recover(written as c_int)?;
                    } else {
                        offset += written as usize;
                    }
                }
                Ok(())
            }
        }
    }
}
