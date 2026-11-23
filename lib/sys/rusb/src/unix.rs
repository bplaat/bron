/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! libusb-1.0 backend. All native pointers are owned by the RAII types below.

#![allow(unsafe_code)]

use std::ffi::{c_int, c_uchar};
use std::ptr::{self, NonNull};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{DeviceDescriptor, Error};

mod sys {
    use std::ffi::{c_int, c_uchar};

    #[repr(C)]
    pub(super) struct LibusbContext {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub(super) struct LibusbDevice {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub(super) struct LibusbDeviceHandle {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub(super) struct LibusbDeviceDescriptor {
        pub(super) length: u8,
        pub(super) descriptor_type: u8,
        pub(super) usb_version: u16,
        pub(super) device_class: u8,
        pub(super) device_sub_class: u8,
        pub(super) device_protocol: u8,
        pub(super) max_packet_size_0: u8,
        pub(super) vendor_id: u16,
        pub(super) product_id: u16,
        pub(super) device_version: u16,
        pub(super) manufacturer_index: u8,
        pub(super) product_index: u8,
        pub(super) serial_number_index: u8,
        pub(super) num_configurations: u8,
    }

    #[cfg_attr(
        target_os = "linux",
        link(name = "libusb-1.0.so.0", kind = "dylib", modifiers = "+verbatim")
    )]
    #[cfg_attr(not(target_os = "linux"), link(name = "usb-1.0"))]
    unsafe extern "C" {
        pub(super) fn libusb_init(context: *mut *mut LibusbContext) -> c_int;
        pub(super) fn libusb_exit(context: *mut LibusbContext);
        pub(super) fn libusb_get_device_list(
            context: *mut LibusbContext,
            list: *mut *mut *mut LibusbDevice,
        ) -> isize;
        pub(super) fn libusb_free_device_list(list: *mut *mut LibusbDevice, unref: c_int);
        pub(super) fn libusb_ref_device(device: *mut LibusbDevice) -> *mut LibusbDevice;
        pub(super) fn libusb_unref_device(device: *mut LibusbDevice);
        pub(super) fn libusb_get_device_descriptor(
            device: *mut LibusbDevice,
            descriptor: *mut LibusbDeviceDescriptor,
        ) -> c_int;
        pub(super) fn libusb_open(
            device: *mut LibusbDevice,
            handle: *mut *mut LibusbDeviceHandle,
        ) -> c_int;
        pub(super) fn libusb_close(handle: *mut LibusbDeviceHandle);
        pub(super) fn libusb_set_configuration(
            handle: *mut LibusbDeviceHandle,
            configuration: c_int,
        ) -> c_int;
        pub(super) fn libusb_claim_interface(
            handle: *mut LibusbDeviceHandle,
            interface: c_int,
        ) -> c_int;
        pub(super) fn libusb_release_interface(
            handle: *mut LibusbDeviceHandle,
            interface: c_int,
        ) -> c_int;
        pub(super) fn libusb_control_transfer(
            handle: *mut LibusbDeviceHandle,
            request_type: c_uchar,
            request: c_uchar,
            value: u16,
            index: u16,
            data: *mut c_uchar,
            length: u16,
            timeout: u32,
        ) -> c_int;
    }
}

pub(crate) struct Context {
    raw: NonNull<sys::LibusbContext>,
}

// SAFETY: libusb documents its contexts and synchronous API as thread-safe. The context is
// reference-counted by the safe facade, so libusb_exit cannot run while a device or handle uses it.
unsafe impl Send for Context {}
// SAFETY: the same libusb thread-safety guarantee permits shared context references.
unsafe impl Sync for Context {}

impl Context {
    pub(crate) fn new() -> Result<Self, Error> {
        let mut raw = ptr::null_mut();
        // SAFETY: libusb initializes `raw`; a successful call returns an owned context.
        check(unsafe { sys::libusb_init(&mut raw) })?;
        let raw = NonNull::new(raw).ok_or(Error::Other)?;
        Ok(Self { raw })
    }

    pub(crate) fn devices(&self) -> Result<Vec<Arc<Device>>, Error> {
        let mut list = ptr::null_mut();
        // SAFETY: the context is live and `list` is a valid output pointer.
        let count = unsafe { sys::libusb_get_device_list(self.raw.as_ptr(), &mut list) };
        if count < 0 {
            return Err(map_error(count as c_int));
        }
        let list = DeviceListGuard { raw: list };
        let devices = crate::collect_initialized(count as usize, |offset| {
            // SAFETY: libusb returned an array containing `count` device pointers.
            let raw = unsafe { *list.raw.add(offset) };
            // SAFETY: the list owns a reference; this adds the reference transferred to Device.
            let raw = unsafe { sys::libusb_ref_device(raw) };
            let raw = NonNull::new(raw).ok_or(Error::Other)?;
            Ok(Arc::new(Device { raw }))
        })?;
        Ok(devices)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: this is the single matching exit for the initialized context.
        unsafe { sys::libusb_exit(self.raw.as_ptr()) };
    }
}

struct DeviceListGuard {
    raw: *mut *mut sys::LibusbDevice,
}

impl Drop for DeviceListGuard {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: this is the list returned by libusb; `1` releases its device refs.
            unsafe { sys::libusb_free_device_list(self.raw, 1) };
        }
    }
}

pub(crate) struct Device {
    raw: NonNull<sys::LibusbDevice>,
}

// SAFETY: libusb devices are thread-safe reference-counted objects. Device owns one reference, and
// Rust ownership ensures its final unref cannot race with a safe method call on the same value.
unsafe impl Send for Device {}
// SAFETY: the same libusb thread-safety guarantee permits shared device references.
unsafe impl Sync for Device {}

impl Device {
    pub(crate) fn device_descriptor(&self) -> Result<DeviceDescriptor, Error> {
        let mut descriptor = std::mem::MaybeUninit::uninit();
        // SAFETY: `descriptor` is valid output storage and the device reference is live.
        check(unsafe {
            sys::libusb_get_device_descriptor(self.raw.as_ptr(), descriptor.as_mut_ptr())
        })?;
        // SAFETY: libusb initialized the complete descriptor after returning success.
        let descriptor = unsafe { descriptor.assume_init() };
        Ok(DeviceDescriptor::new(
            descriptor.vendor_id,
            descriptor.product_id,
        ))
    }

    pub(crate) fn open(&self) -> Result<Handle, Error> {
        let mut raw = ptr::null_mut();
        // SAFETY: the device is live and `raw` is valid output storage.
        check(unsafe { sys::libusb_open(self.raw.as_ptr(), &mut raw) })?;
        Ok(Handle {
            raw: NonNull::new(raw).ok_or(Error::Other)?,
            claimed: Mutex::new(Vec::new()),
        })
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: Device owns exactly one reference acquired by libusb_ref_device.
        unsafe { sys::libusb_unref_device(self.raw.as_ptr()) };
    }
}

pub(crate) struct Handle {
    raw: NonNull<sys::LibusbDeviceHandle>,
    claimed: Mutex<Vec<u8>>,
}

// SAFETY: libusb permits calls on device handles from multiple threads. The claimed-interface
// bookkeeping is synchronized, and Rust ownership prevents close from racing with a safe borrow.
unsafe impl Send for Handle {}
// SAFETY: the same guarantee permits shared access to a handle while Rust keeps it alive.
unsafe impl Sync for Handle {}

impl Handle {
    pub(crate) fn set_active_configuration(&self, configuration: u8) -> Result<(), Error> {
        // SAFETY: the handle remains live for this call.
        check(unsafe {
            sys::libusb_set_configuration(self.raw.as_ptr(), c_int::from(configuration))
        })
    }

    pub(crate) fn claim_interface(&self, interface: u8) -> Result<(), Error> {
        let mut claimed = self.claimed.lock().map_err(|_| Error::Other)?;
        if claimed.contains(&interface) {
            return Ok(());
        }
        // SAFETY: the handle remains live and libusb accepts the interface number as an int.
        check(unsafe { sys::libusb_claim_interface(self.raw.as_ptr(), c_int::from(interface)) })?;
        claimed.push(interface);
        Ok(())
    }

    pub(crate) fn write_control(
        &self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, Error> {
        let length = u16::try_from(data.len()).map_err(|_| Error::InvalidParam)?;
        // libusb's C API does not mutate OUT transfer buffers despite taking a mutable pointer.
        let data_ptr = data.as_ptr().cast_mut();
        // SAFETY: the handle and buffer remain live for the synchronous call.
        let result = unsafe {
            sys::libusb_control_transfer(
                self.raw.as_ptr(),
                request_type as c_uchar,
                request as c_uchar,
                value,
                index,
                data_ptr,
                length,
                timeout_millis(timeout),
            )
        };
        if result < 0 {
            Err(map_error(result))
        } else {
            Ok(result as usize)
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let claimed = self
            .claimed
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        crate::release_reverse(claimed, |interface| {
            // SAFETY: claimed interfaces are released before the live handle is closed.
            unsafe {
                sys::libusb_release_interface(self.raw.as_ptr(), c_int::from(interface));
            }
        });
        // SAFETY: this is the single close matching the successful libusb_open.
        unsafe { sys::libusb_close(self.raw.as_ptr()) };
    }
}

const fn timeout_millis(timeout: Duration) -> u32 {
    let millis = timeout.as_millis();
    if millis > u32::MAX as u128 {
        u32::MAX
    } else {
        millis as u32
    }
}

const fn map_error(code: c_int) -> Error {
    match code {
        -1 => Error::Io,
        -2 => Error::InvalidParam,
        -3 => Error::Access,
        -4 => Error::NoDevice,
        -5 => Error::NotFound,
        -6 => Error::Busy,
        -7 => Error::Timeout,
        -8 => Error::Overflow,
        -9 => Error::Pipe,
        -10 => Error::Interrupted,
        -11 => Error::NoMem,
        -12 => Error::NotSupported,
        _ => Error::Other,
    }
}

const fn check(code: c_int) -> Result<(), Error> {
    if code < 0 {
        Err(map_error(code))
    } else {
        Ok(())
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn native_resources_are_thread_safe() {
        assert_send_sync::<Context>();
        assert_send_sync::<Device>();
        assert_send_sync::<Handle>();
    }

    #[test]
    fn maps_every_libusb_error() {
        let expected = [
            Error::Io,
            Error::InvalidParam,
            Error::Access,
            Error::NoDevice,
            Error::NotFound,
            Error::Busy,
            Error::Timeout,
            Error::Overflow,
            Error::Pipe,
            Error::Interrupted,
            Error::NoMem,
            Error::NotSupported,
        ];
        for (offset, error) in expected.into_iter().enumerate() {
            assert_eq!(map_error(-((offset as c_int) + 1)), error);
        }
        assert_eq!(map_error(-99), Error::Other);
    }

    #[test]
    fn converts_and_saturates_timeouts() {
        assert_eq!(timeout_millis(Duration::ZERO), 0);
        assert_eq!(timeout_millis(Duration::from_micros(1_999)), 1);
        assert_eq!(timeout_millis(Duration::from_millis(500)), 500);
        assert_eq!(timeout_millis(Duration::MAX), u32::MAX);
    }

    #[test]
    fn descriptor_layout_matches_usb_spec() {
        assert_eq!(size_of::<sys::LibusbDeviceDescriptor>(), 18);
        let descriptor = sys::LibusbDeviceDescriptor {
            length: 18,
            descriptor_type: 1,
            usb_version: 0x0200,
            device_class: 0,
            device_sub_class: 0,
            device_protocol: 0,
            max_packet_size_0: 8,
            vendor_id: 0x1234,
            product_id: 0xabcd,
            device_version: 0x0100,
            manufacturer_index: 1,
            product_index: 2,
            serial_number_index: 3,
            num_configurations: 1,
        };
        assert_eq!(descriptor.vendor_id, 0x1234);
        assert_eq!(descriptor.product_id, 0xabcd);
    }
}
