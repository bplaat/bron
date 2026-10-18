/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

cfg_select! {
    target_os = "macos" => {
        use crate::macos as backend;
        mod macos;
    }
    windows => {
        use crate::windows as backend;
        mod windows;
    }
    any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ) => {
        use crate::unix as backend;
        mod unix;
    }
    _ => {
        compile_error!("Unsupported platform");
    }
}

/// Errors returned by USB operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Input/output error.
    Io,
    /// Invalid parameter.
    InvalidParam,
    /// Access was denied.
    Access,
    /// The device or resource is busy.
    Busy,
    /// The operation timed out.
    Timeout,
    /// An integer or buffer overflow occurred.
    Overflow,
    /// A pipe stalled.
    Pipe,
    /// The operation was interrupted.
    Interrupted,
    /// Memory allocation failed.
    NoMem,
    /// The operation is not supported.
    NotSupported,
    /// The USB device is no longer present.
    NoDevice,
    /// The requested item was not found.
    NotFound,
    /// Another error occurred.
    Other,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Io => "input/output error",
            Self::InvalidParam => "invalid parameter",
            Self::Access => "access denied",
            Self::Busy => "resource busy",
            Self::Timeout => "operation timed out",
            Self::Overflow => "overflow",
            Self::Pipe => "pipe error",
            Self::Interrupted => "operation interrupted",
            Self::NoMem => "out of memory",
            Self::NotSupported => "operation not supported",
            Self::NoDevice => "device not present",
            Self::NotFound => "not found",
            Self::Other => "other USB error",
        })
    }
}

impl std::error::Error for Error {}

/// USB transfer direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// Host to device.
    Out,
    /// Device to host.
    In,
}

/// USB request type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestType {
    /// Standard USB request.
    Standard,
    /// Class-specific request.
    Class,
    /// Vendor-specific request.
    Vendor,
    /// Reserved request type.
    Reserved,
}

/// USB request recipient.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recipient {
    /// Device recipient.
    Device,
    /// Interface recipient.
    Interface,
    /// Endpoint recipient.
    Endpoint,
    /// Other recipient.
    Other,
}

/// Builds a USB control request type byte.
pub const fn request_type(
    direction: Direction,
    request_type: RequestType,
    recipient: Recipient,
) -> u8 {
    let direction = match direction {
        Direction::Out => 0x00,
        Direction::In => 0x80,
    };
    let request_type = match request_type {
        RequestType::Standard => 0x00,
        RequestType::Class => 0x20,
        RequestType::Vendor => 0x40,
        RequestType::Reserved => 0x60,
    };
    let recipient = match recipient {
        Recipient::Device => 0x00,
        Recipient::Interface => 0x01,
        Recipient::Endpoint => 0x02,
        Recipient::Other => 0x03,
    };
    direction | request_type | recipient
}

/// A USB context.
#[derive(Clone)]
pub struct Context {
    inner: Arc<backend::Context>,
}

impl Context {
    /// Creates a USB context.
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            inner: Arc::new(backend::Context::new()?),
        })
    }
}

/// Operations implemented by USB contexts.
pub trait UsbContext: Sized {
    /// Returns the currently present USB devices.
    fn devices(&self) -> Result<DeviceList<Self>, Error>;
}

impl UsbContext for Context {
    fn devices(&self) -> Result<DeviceList<Self>, Error> {
        let devices = self
            .inner
            .devices()?
            .into_iter()
            .map(|inner| Device {
                inner,
                context: Arc::clone(&self.inner),
                marker: PhantomData,
            })
            .collect();
        Ok(DeviceList { devices })
    }
}

/// A snapshot of USB devices.
pub struct DeviceList<T: UsbContext> {
    devices: Vec<Device<T>>,
}

impl<T: UsbContext> DeviceList<T> {
    /// Iterates over the devices in this snapshot.
    pub fn iter(&self) -> impl Iterator<Item = Device<T>> + '_ {
        self.devices.iter().cloned()
    }
}

/// A USB device.
pub struct Device<T: UsbContext> {
    inner: Arc<backend::Device>,
    context: Arc<backend::Context>,
    marker: PhantomData<T>,
}

impl<T: UsbContext> Clone for Device<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            context: Arc::clone(&self.context),
            marker: PhantomData,
        }
    }
}

impl<T: UsbContext> Device<T> {
    /// Reads the device descriptor.
    pub fn device_descriptor(&self) -> Result<DeviceDescriptor, Error> {
        self.inner.device_descriptor()
    }

    /// Opens the device.
    pub fn open(&self) -> Result<DeviceHandle<T>, Error> {
        Ok(DeviceHandle {
            inner: self.inner.open()?,
            _context: Arc::clone(&self.context),
            marker: PhantomData,
        })
    }
}

/// The fields from a USB device descriptor exposed by this minimal API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceDescriptor {
    vendor_id: u16,
    product_id: u16,
}

impl DeviceDescriptor {
    pub(crate) const fn new(vendor_id: u16, product_id: u16) -> Self {
        Self {
            vendor_id,
            product_id,
        }
    }

    /// Returns the vendor identifier.
    pub const fn vendor_id(&self) -> u16 {
        self.vendor_id
    }

    /// Returns the product identifier.
    pub const fn product_id(&self) -> u16 {
        self.product_id
    }
}

/// An open USB device handle.
pub struct DeviceHandle<T: UsbContext> {
    inner: backend::Handle,
    _context: Arc<backend::Context>,
    marker: PhantomData<T>,
}

impl<T: UsbContext> DeviceHandle<T> {
    /// Selects the active configuration.
    #[allow(clippy::missing_const_for_fn)]
    pub fn set_active_configuration(&self, configuration: u8) -> Result<(), Error> {
        self.inner.set_active_configuration(configuration)
    }

    /// Claims an interface for this handle.
    #[allow(clippy::missing_const_for_fn)]
    pub fn claim_interface(&self, interface: u8) -> Result<(), Error> {
        self.inner.claim_interface(interface)
    }

    /// Performs a host-to-device control transfer.
    pub fn write_control(
        &self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, Error> {
        validate_write_request_type(request_type)?;
        self.inner
            .write_control(request_type, request, value, index, data, timeout)
    }
}

const fn validate_write_request_type(request_type: u8) -> Result<(), Error> {
    if request_type & 0x80 == 0 {
        Ok(())
    } else {
        Err(Error::InvalidParam)
    }
}

#[cfg(any(
    test,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn collect_initialized<T, E>(
    count: usize,
    mut initialize: impl FnMut(usize) -> Result<T, E>,
) -> Result<Vec<T>, E> {
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        values.push(initialize(index)?);
    }
    Ok(values)
}

#[cfg(any(
    test,
    windows,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn release_reverse<T>(values: &mut Vec<T>, mut release: impl FnMut(T)) {
    for value in values.drain(..).rev() {
        release(value);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn all_request_type_bit_combinations() {
        let directions = [(Direction::Out, 0x00), (Direction::In, 0x80)];
        let types = [
            (RequestType::Standard, 0x00),
            (RequestType::Class, 0x20),
            (RequestType::Vendor, 0x40),
            (RequestType::Reserved, 0x60),
        ];
        let recipients = [
            (Recipient::Device, 0x00),
            (Recipient::Interface, 0x01),
            (Recipient::Endpoint, 0x02),
            (Recipient::Other, 0x03),
        ];
        for (direction, direction_bits) in directions {
            for (kind, kind_bits) in types {
                for (recipient, recipient_bits) in recipients {
                    assert_eq!(
                        request_type(direction, kind, recipient),
                        direction_bits | kind_bits | recipient_bits
                    );
                }
            }
        }
    }

    #[test]
    fn write_control_rejects_device_to_host_requests() {
        assert_eq!(validate_write_request_type(0x40), Ok(()));
        assert_eq!(validate_write_request_type(0xc0), Err(Error::InvalidParam));
    }

    #[test]
    fn partial_initialization_releases_completed_resources_once() {
        struct Resource<'a>(&'a AtomicUsize);

        impl Drop for Resource<'_> {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let drops = AtomicUsize::new(0);
        let result = collect_initialized(4, |index| {
            if index == 2 {
                Err(Error::Other)
            } else {
                Ok(Resource(&drops))
            }
        });
        assert!(result.is_err());
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn resources_are_released_once_in_reverse_order() {
        let mut resources = vec![0, 1, 2];
        let mut released = Vec::new();
        release_reverse(&mut resources, |resource| released.push(resource));
        assert!(resources.is_empty());
        assert_eq!(released, [2, 1, 0]);
    }
}
