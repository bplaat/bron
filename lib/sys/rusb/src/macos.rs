/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native IOUSBHost backend. Objective-C and IOKit calls are confined here.

#![allow(unsafe_code)]

use std::ffi::{c_char, c_void};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::time::Duration;

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2::{Encode, Encoding, class, msg_send};

use crate::{DeviceDescriptor, Error};

type IoObject = u32;
type IoIterator = IoObject;
type IoService = IoObject;
type KernReturn = i32;
type CfTypeRef = *const c_void;
type CfMutableDictionaryRef = *mut c_void;
type CfStringRef = *const c_void;

const K_IO_MAIN_PORT_DEFAULT: u32 = 0;
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_CF_NUMBER_SINT32_TYPE: i32 = 3;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> CfMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        main_port: u32,
        matching: CfMutableDictionaryRef,
        existing: *mut IoIterator,
    ) -> KernReturn;
    fn IOIteratorNext(iterator: IoIterator) -> IoObject;
    fn IOObjectRelease(object: IoObject) -> KernReturn;
    fn IORegistryEntryCreateCFProperty(
        entry: IoService,
        key: CfStringRef,
        allocator: *const c_void,
        options: u32,
    ) -> CfTypeRef;
}

#[link(name = "IOUSBHost", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFAllocatorDefault: *const c_void;
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        string: *const c_char,
        encoding: u32,
    ) -> CfStringRef;
    fn CFNumberGetValue(number: CfTypeRef, number_type: i32, value: *mut c_void) -> bool;
    fn CFRelease(object: CfTypeRef);
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct DeviceRequest {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
}

// SAFETY: this exactly matches Apple's packed IOUSBDeviceRequest declaration.
unsafe impl Encode for DeviceRequest {
    const ENCODING: Encoding = Encoding::Struct(
        "IOUSBDeviceRequest",
        &[
            u8::ENCODING,
            u8::ENCODING,
            u16::ENCODING,
            u16::ENCODING,
            u16::ENCODING,
        ],
    );
}

pub(crate) struct Context;

impl Context {
    pub(crate) const fn new() -> Result<Self, Error> {
        Ok(Self)
    }

    pub(crate) fn devices(&self) -> Result<Vec<Arc<Device>>, Error> {
        autoreleasepool(|_| {
            // SAFETY: the class name is a static, null-terminated C string.
            let matching = unsafe { IOServiceMatching(c"IOUSBHostDevice".as_ptr()) };
            if matching.is_null() {
                return Err(Error::NoMem);
            }
            let mut iterator = 0;
            // SAFETY: IOKit consumes `matching` and initializes the iterator on success.
            check_ioreturn(unsafe {
                IOServiceGetMatchingServices(K_IO_MAIN_PORT_DEFAULT, matching, &mut iterator)
            })?;
            let iterator = IoObjectGuard(iterator);
            let mut devices = Vec::new();
            loop {
                // SAFETY: `iterator` is live for the duration of enumeration.
                let service = unsafe { IOIteratorNext(iterator.0) };
                if service == 0 {
                    break;
                }
                devices.push(Arc::new(Device { service }));
            }
            Ok(devices)
        })
    }
}

struct IoObjectGuard(IoObject);

impl Drop for IoObjectGuard {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: this guard owns the IOKit object reference.
            unsafe { IOObjectRelease(self.0) };
        }
    }
}

pub(crate) struct Device {
    service: IoService,
}

impl Device {
    pub(crate) fn device_descriptor(&self) -> Result<DeviceDescriptor, Error> {
        autoreleasepool(|_| {
            Ok(DeviceDescriptor::new(
                registry_u16(self.service, c"idVendor")?,
                registry_u16(self.service, c"idProduct")?,
            ))
        })
    }

    pub(crate) fn open(&self) -> Result<Handle, Error> {
        autoreleasepool(|_| {
            let mut error: *mut AnyObject = ptr::null_mut();
            // SAFETY: the named Objective-C class is provided by IOUSBHost.
            let allocated: *mut AnyObject = unsafe { msg_send![class!(IOUSBHostDevice), alloc] };
            if allocated.is_null() {
                return Err(Error::NoMem);
            }
            // SAFETY: the service is a live IOUSBHostDevice service. Nil queue and handler request
            // IOUSBHost defaults, and `error` is valid output storage.
            let object: *mut AnyObject = unsafe {
                msg_send![allocated, initWithIOService:self.service, options:0usize, queue:ptr::null_mut::<AnyObject>(), error:(&mut error as *mut *mut AnyObject).cast::<c_void>(), interestHandler:ptr::null_mut::<AnyObject>()]
            };
            // SAFETY: a successful init returns a +1 Objective-C object.
            let object =
                unsafe { Retained::from_raw(object) }.ok_or_else(|| error_from_nserror(error))?;
            Ok(Handle { object })
        })
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: Device owns the reference returned by IOIteratorNext.
        unsafe { IOObjectRelease(self.service) };
    }
}

pub(crate) struct Handle {
    object: Retained<AnyObject>,
}

impl Handle {
    pub(crate) fn set_active_configuration(&self, configuration: u8) -> Result<(), Error> {
        autoreleasepool(|_| {
            let mut error: *mut AnyObject = ptr::null_mut();
            // SAFETY: this is a live IOUSBHostDevice and arguments match the SDK declaration.
            let success: bool = unsafe {
                msg_send![&self.object, configureWithValue:usize::from(configuration), matchInterfaces:true, error:(&mut error as *mut *mut AnyObject).cast::<c_void>()]
            };
            bool_result(success, error)
        })
    }

    pub(crate) const fn claim_interface(&self, interface: u8) -> Result<(), Error> {
        validate_interface(interface)
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
        autoreleasepool(|_| {
            // SAFETY: NSMutableData copies `data`, whose pointer and length form a valid slice.
            let buffer: *mut AnyObject = unsafe {
                msg_send![class!(NSMutableData), dataWithBytes:data.as_ptr().cast::<c_void>(), length:data.len()]
            };
            if buffer.is_null() {
                return Err(Error::NoMem);
            }
            let request = DeviceRequest {
                request_type,
                request,
                value,
                index,
                length,
            };
            let mut transferred = 0usize;
            let mut error: *mut AnyObject = ptr::null_mut();
            // SAFETY: the request has Apple's packed ABI; the mutable data object and output
            // pointers remain live until this synchronous method returns.
            let success: bool = unsafe {
                msg_send![&self.object, sendDeviceRequest:request, data:buffer, bytesTransferred:(&mut transferred as *mut usize).cast::<c_void>(), completionTimeout:timeout.as_secs_f64(), error:(&mut error as *mut *mut AnyObject).cast::<c_void>()]
            };
            bool_result(success, error)?;
            Ok(transferred)
        })
    }
}

// This minimal backend has proven behavior only for interface zero through the device's exclusive
// default endpoint. Other interfaces require IOUSBHostInterface support.
const fn validate_interface(interface: u8) -> Result<(), Error> {
    if interface == 0 {
        Ok(())
    } else {
        Err(Error::NotSupported)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        autoreleasepool(|_| {
            // SAFETY: IOUSBHost documents destroy as idempotent and requires it before release.
            unsafe {
                let _: () = msg_send![&self.object, destroy];
            }
        });
    }
}

fn registry_u16(service: IoService, key: &std::ffi::CStr) -> Result<u16, Error> {
    // SAFETY: the allocator and UTF-8 key are valid CoreFoundation inputs.
    let key = unsafe {
        CFStringCreateWithCString(kCFAllocatorDefault, key.as_ptr(), K_CF_STRING_ENCODING_UTF8)
    };
    let key = NonNull::new(key.cast_mut()).ok_or(Error::NoMem)?;
    // SAFETY: the service is live and `key` is a CFString.
    let value =
        unsafe { IORegistryEntryCreateCFProperty(service, key.as_ptr(), kCFAllocatorDefault, 0) };
    // SAFETY: the created key is no longer needed after the property lookup.
    unsafe { CFRelease(key.as_ptr()) };
    let value = NonNull::new(value.cast_mut()).ok_or(Error::NotFound)?;
    let mut number = 0i32;
    // SAFETY: USB ID properties are CFNumbers and `number` is valid output storage.
    let success = unsafe {
        CFNumberGetValue(
            value.as_ptr(),
            K_CF_NUMBER_SINT32_TYPE,
            (&mut number as *mut i32).cast(),
        )
    };
    // SAFETY: IORegistryEntryCreateCFProperty returned an owned CF object.
    unsafe { CFRelease(value.as_ptr()) };
    if success {
        u16::try_from(number).map_err(|_| Error::Other)
    } else {
        Err(Error::Other)
    }
}

fn bool_result(success: bool, error: *mut AnyObject) -> Result<(), Error> {
    if success {
        Ok(())
    } else {
        Err(error_from_nserror(error))
    }
}

fn error_from_nserror(error: *mut AnyObject) -> Error {
    if error.is_null() {
        return Error::Other;
    }
    // SAFETY: the pointer is a live NSError supplied by the immediately preceding call.
    let code: isize = unsafe { msg_send![error, code] };
    map_ioreturn(code as i32)
}

const fn check_ioreturn(code: KernReturn) -> Result<(), Error> {
    if code == 0 {
        Ok(())
    } else {
        Err(map_ioreturn(code))
    }
}

const fn map_ioreturn(code: KernReturn) -> Error {
    match code as u32 {
        0xe000_02bd => Error::NoMem,
        0xe000_02c0 | 0xe000_02d7 | 0xe000_02d9 => Error::NoDevice,
        0xe000_02c1 | 0xe000_02e2 => Error::Access,
        0xe000_02c2 => Error::InvalidParam,
        0xe000_02c5 | 0xe000_02d5 => Error::Busy,
        0xe000_02c7 | 0xe000_02e6 => Error::NotSupported,
        0xe000_02ca => Error::Io,
        0xe000_02d6 | 0xe000_02ed => Error::Timeout,
        0xe000_02e1 | 0xe000_02e8 => Error::Overflow,
        0xe000_02eb => Error::Interrupted,
        0xe000_02f0 => Error::NotFound,
        _ => Error::Other,
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_request_has_packed_usb_layout() {
        assert_eq!(size_of::<DeviceRequest>(), 8);
        assert_eq!(align_of::<DeviceRequest>(), 1);
    }

    #[test]
    fn maps_ioreturn_errors() {
        assert_eq!(map_ioreturn(0xe000_02c0u32 as i32), Error::NoDevice);
        assert_eq!(map_ioreturn(0xe000_02c1u32 as i32), Error::Access);
        assert_eq!(map_ioreturn(0xe000_02c5u32 as i32), Error::Busy);
        assert_eq!(map_ioreturn(0xe000_02d6u32 as i32), Error::Timeout);
        assert_eq!(map_ioreturn(0xe000_02c7u32 as i32), Error::NotSupported);
        assert_eq!(map_ioreturn(-1), Error::Other);
    }

    #[test]
    fn maps_nserror_codes() {
        autoreleasepool(|_| {
            // SAFETY: the string literal is NUL-terminated and valid for the duration of the call.
            let domain: *mut AnyObject =
                unsafe { msg_send![class!(NSString), stringWithUTF8String:c"IOKit".as_ptr()] };
            // SAFETY: NSError's constructor accepts this domain, code, and nil userInfo.
            let error: *mut AnyObject = unsafe {
                msg_send![class!(NSError), errorWithDomain:domain, code:0xe000_02d6u32 as i32 as isize, userInfo:ptr::null_mut::<AnyObject>()]
            };
            assert_eq!(error_from_nserror(error), Error::Timeout);
        });
    }

    #[test]
    fn supports_only_the_proven_interface() {
        assert_eq!(validate_interface(0), Ok(()));
        assert_eq!(validate_interface(1), Err(Error::NotSupported));
    }
}
