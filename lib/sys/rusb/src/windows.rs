/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! WinUSB backend with minimal handwritten Win32 declarations.

#![allow(unsafe_code)]

use std::collections::HashSet;
use std::ffi::{OsString, c_void};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStringExt;
use std::ptr::{self, NonNull};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::time::Duration;

use crate::windows::headers::*;
use crate::{DeviceDescriptor, Error};

mod headers;

static TRANSFER_REAPER: LazyLock<Option<mpsc::Sender<PendingTransfer>>> = LazyLock::new(|| {
    let (sender, receiver) = mpsc::channel::<PendingTransfer>();
    std::thread::Builder::new()
        .name("winusb-transfer-reaper".to_string())
        .spawn(move || {
            for transfer in receiver {
                transfer.drain();
            }
        })
        .ok()
        .map(|_| sender)
});

pub(crate) struct Context;

impl Context {
    pub(crate) const fn new() -> Result<Self, Error> {
        Ok(Self)
    }

    pub(crate) fn devices(&self) -> Result<Vec<Arc<Device>>, Error> {
        let usb = wide("USB");
        // SAFETY: pointers are valid and SetupAPI returns an owned set handle.
        let set = unsafe {
            SetupDiGetClassDevsW(
                ptr::null(),
                usb.as_ptr(),
                ptr::null_mut(),
                DIGCF_PRESENT | DIGCF_ALLCLASSES,
            )
        };
        let set = DeviceInfoSet::new(set)?;
        let mut devices = Vec::new();
        let mut seen_paths = HashSet::new();
        let mut index = 0;
        loop {
            // SAFETY: this all-zero representation is valid once cbSize is populated.
            let mut info: SpDevinfoData = unsafe { zeroed() };
            info.size = size_of::<SpDevinfoData>() as Dword;
            // SAFETY: set and output storage are valid.
            if unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut info) } == 0 {
                // SAFETY: GetLastError describes the immediately preceding failure.
                let error = unsafe { GetLastError() };
                if error == ERROR_NO_MORE_ITEMS {
                    break;
                }
                return Err(map_win32_error(error));
            }
            index += 1;
            let Some((vendor_id, product_id)) = hardware_ids(&set, &mut info)? else {
                continue;
            };
            let guids = interface_guids(&set, &mut info)?;
            for guid in guids {
                for path in interface_paths(guid)? {
                    if path_matches_ids(&path, vendor_id, product_id)
                        && seen_paths.insert(path.clone())
                    {
                        devices.push(Arc::new(Device {
                            path,
                            vendor_id,
                            product_id,
                        }));
                    }
                }
            }
        }
        Ok(devices)
    }
}

struct DeviceInfoSet(Hdevinfo);

impl DeviceInfoSet {
    fn new(raw: Hdevinfo) -> Result<Self, Error> {
        if raw == INVALID_HANDLE_VALUE {
            // SAFETY: GetLastError describes the failed SetupAPI call.
            Err(map_win32_error(unsafe { GetLastError() }))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        // SAFETY: this is the one destroy for the owned SetupAPI set.
        unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

struct RegistryKey(Hkey);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: this is the one close for the opened registry key.
        unsafe { RegCloseKey(self.0) };
    }
}

pub(crate) struct Device {
    path: Vec<u16>,
    vendor_id: u16,
    product_id: u16,
}

impl Device {
    pub(crate) const fn device_descriptor(&self) -> Result<DeviceDescriptor, Error> {
        Ok(DeviceDescriptor::new(self.vendor_id, self.product_id))
    }

    pub(crate) fn open(&self) -> Result<Handle, Error> {
        // SAFETY: path is null-terminated and all other arguments follow CreateFileW's contract.
        let file = unsafe {
            CreateFileW(
                self.path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        };
        if file == INVALID_HANDLE_VALUE {
            // SAFETY: GetLastError describes CreateFileW's failure.
            return Err(map_win32_error(unsafe { GetLastError() }));
        }
        let file = FileHandle(file);
        let mut winusb = ptr::null_mut();
        // SAFETY: file is a live WinUSB device handle and output storage is valid.
        if unsafe { WinUsb_Initialize(file.0, &mut winusb) } == 0 {
            // SAFETY: GetLastError describes WinUsb_Initialize's failure.
            return Err(map_win32_error(unsafe { GetLastError() }));
        }
        Ok(Handle(Arc::new(NativeHandle {
            file,
            winusb: NonNull::new(winusb).ok_or(Error::Other)?,
            associated: Mutex::new(Vec::new()),
        })))
    }
}

struct FileHandle(HandleValue);

impl Drop for FileHandle {
    fn drop(&mut self) {
        // SAFETY: this is the one close for the CreateFileW handle.
        unsafe { CloseHandle(self.0) };
    }
}

struct NativeHandle {
    file: FileHandle,
    winusb: NonNull<c_void>,
    associated: Mutex<Vec<(u8, NonNull<c_void>)>>,
}

// SAFETY: WinUSB and kernel handles may be used from multiple threads. The associated-interface
// collection is mutex-protected, and Arc prevents handles from closing during an operation.
unsafe impl Send for NativeHandle {}
// SAFETY: see the Send implementation; all mutable Rust state is synchronized.
unsafe impl Sync for NativeHandle {}

pub(crate) struct Handle(Arc<NativeHandle>);

impl Handle {
    pub(crate) const fn set_active_configuration(&self, configuration: u8) -> Result<(), Error> {
        if configuration == 1 {
            Ok(())
        } else {
            Err(Error::NotSupported)
        }
    }

    pub(crate) fn claim_interface(&self, interface: u8) -> Result<(), Error> {
        if interface == 0 {
            return Ok(());
        }
        let mut associated = self.0.associated.lock().map_err(|_| Error::Other)?;
        if associated.iter().any(|(number, _)| *number == interface) {
            return Ok(());
        }
        let mut raw = ptr::null_mut();
        // SAFETY: the primary interface is live and the zero-based associated index is valid.
        if unsafe { WinUsb_GetAssociatedInterface(self.0.winusb.as_ptr(), interface - 1, &mut raw) }
            == 0
        {
            // SAFETY: GetLastError describes the immediately preceding WinUSB call.
            return Err(map_win32_error(unsafe { GetLastError() }));
        }
        associated.push((interface, NonNull::new(raw).ok_or(Error::Other)?));
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
        let mut transfer = PendingTransfer::new(Arc::clone(&self.0), data.to_vec())?;
        let setup = WinusbSetupPacket {
            request_type,
            request,
            value,
            index,
            length,
        };
        // SAFETY: the owned interface, buffer, event, and boxed OVERLAPPED remain live until the
        // operation completes, including after a timeout when ownership moves to the reaper.
        let started = unsafe {
            WinUsb_ControlTransfer(
                transfer.native.winusb.as_ptr(),
                setup,
                transfer.buffer.as_mut_ptr(),
                Dword::from(length),
                ptr::null_mut(),
                transfer.overlapped.as_mut(),
            )
        };
        if started != 0 {
            // An overlapped operation may complete synchronously; retrieve its byte count.
            return transfer.result();
        }
        // SAFETY: GetLastError describes WinUsb_ControlTransfer's result.
        let error = unsafe { GetLastError() };
        if error != ERROR_IO_PENDING {
            return Err(map_win32_error(error));
        }
        // SAFETY: the owned event remains live and timeout conversion is bounded.
        match unsafe { WaitForSingleObject(transfer.event.0, timeout_millis(timeout)) } {
            WAIT_OBJECT_0 => transfer.result(),
            WAIT_TIMEOUT => {
                transfer.cancel_and_reap();
                Err(Error::Timeout)
            }
            _ => {
                // SAFETY: GetLastError describes WaitForSingleObject's failure.
                let error = map_win32_error(unsafe { GetLastError() });
                transfer.cancel_and_reap();
                Err(error)
            }
        }
    }
}

struct PendingTransfer {
    native: Arc<NativeHandle>,
    buffer: Vec<u8>,
    overlapped: Box<Overlapped>,
    event: EventHandle,
}

// SAFETY: every field is exclusively owned by the transfer. Windows permits the contained native
// handles to complete and be queried on a different thread.
unsafe impl Send for PendingTransfer {}

impl PendingTransfer {
    fn new(native: Arc<NativeHandle>, buffer: Vec<u8>) -> Result<Self, Error> {
        // SAFETY: default security, unnamed event, initially nonsignaled.
        let event = unsafe { CreateEventW(ptr::null_mut(), 0, 0, ptr::null()) };
        let event = EventHandle::new(event)?;
        // SAFETY: an all-zero OVERLAPPED is valid before its event field is assigned.
        let mut overlapped: Box<Overlapped> = Box::new(unsafe { zeroed() });
        overlapped.event = event.0;
        Ok(Self {
            native,
            buffer,
            overlapped,
            event,
        })
    }

    fn cancel_and_reap(mut self) {
        // SAFETY: this targets the exact live operation owned by this value.
        unsafe { CancelIoEx(self.native.file.0, self.overlapped.as_mut()) };
        let transfer = match TRANSFER_REAPER.as_ref() {
            Some(reaper) => match reaper.send(self) {
                Ok(()) => return,
                Err(error) => error.0,
            },
            None => self,
        };
        // A failed cleanup worker is exceptional. Leaking preserves memory safety and the caller's
        // timeout guarantee because Windows may still access this operation.
        std::mem::forget(transfer);
    }

    fn drain(mut self) {
        // SAFETY: the reaper owns all operation storage until the event is signaled.
        if unsafe { WaitForSingleObject(self.event.0, INFINITE) } != WAIT_OBJECT_0 {
            // Windows may still own pointers into this value. Preserve them if the valid event
            // unexpectedly cannot be waited on.
            std::mem::forget(self);
            return;
        }
        let _ = self.result();
    }

    fn result(&mut self) -> Result<usize, Error> {
        let mut transferred = 0;
        // SAFETY: the operation completed and both output pointers are valid.
        if unsafe {
            WinUsb_GetOverlappedResult(
                self.native.winusb.as_ptr(),
                self.overlapped.as_mut(),
                &mut transferred,
                0,
            )
        } == 0
        {
            // SAFETY: GetLastError describes the result query failure.
            Err(map_win32_error(unsafe { GetLastError() }))
        } else {
            Ok(transferred as usize)
        }
    }
}

impl Drop for NativeHandle {
    fn drop(&mut self) {
        let associated = self
            .associated
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        crate::release_reverse(associated, |(_, handle)| {
            // SAFETY: each associated interface handle is owned exactly once.
            unsafe { WinUsb_Free(handle.as_ptr()) };
        });
        // SAFETY: this is the one free for the primary interface, before the file closes.
        unsafe { WinUsb_Free(self.winusb.as_ptr()) };
    }
}

struct EventHandle(HandleValue);

impl EventHandle {
    fn new(raw: HandleValue) -> Result<Self, Error> {
        if raw.is_null() {
            // SAFETY: GetLastError describes CreateEventW's failure.
            Err(map_win32_error(unsafe { GetLastError() }))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for EventHandle {
    fn drop(&mut self) {
        // SAFETY: this is the one close for the event handle.
        unsafe { CloseHandle(self.0) };
    }
}

fn hardware_ids(
    set: &DeviceInfoSet,
    info: &mut SpDevinfoData,
) -> Result<Option<(u16, u16)>, Error> {
    let mut required = 0;
    let mut value_type = 0;
    // SAFETY: this size query intentionally supplies no output buffer.
    unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            set.0,
            info,
            SPDRP_HARDWAREID,
            &mut value_type,
            ptr::null_mut(),
            0,
            &mut required,
        );
    }
    // SAFETY: GetLastError describes the size query.
    let error = unsafe { GetLastError() };
    if error == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if error != ERROR_INSUFFICIENT_BUFFER || required == 0 {
        return Err(map_win32_error(error));
    }
    let mut bytes = vec![0u8; required as usize];
    // SAFETY: the allocated buffer has exactly the requested byte capacity.
    if unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            set.0,
            info,
            SPDRP_HARDWAREID,
            &mut value_type,
            bytes.as_mut_ptr(),
            required,
            &mut required,
        )
    } == 0
    {
        // SAFETY: GetLastError describes the property read failure.
        return Err(map_win32_error(unsafe { GetLastError() }));
    }
    Ok(parse_hardware_ids(&bytes_to_wide(&bytes)))
}

fn interface_guids(set: &DeviceInfoSet, info: &mut SpDevinfoData) -> Result<Vec<Guid>, Error> {
    // SAFETY: arguments select this device's hardware registry key for read access.
    let key =
        unsafe { SetupDiOpenDevRegKey(set.0, info, DICS_FLAG_GLOBAL, 0, DIREG_DEV, KEY_READ) };
    if key == INVALID_HANDLE_VALUE {
        // Devices without a user-space interface are not openable through WinUSB.
        return Ok(Vec::new());
    }
    let key = RegistryKey(key);
    let mut values = Vec::new();
    for name in ["DeviceInterfaceGUIDs", "DeviceInterfaceGUID"] {
        if let Some((value_type, data)) = registry_value(&key, name)? {
            values.push((value_type, data));
        }
    }
    Ok(first_registry_guids(&values))
}

fn first_registry_guids(values: &[(Dword, Vec<u16>)]) -> Vec<Guid> {
    values
        .iter()
        .map(|(value_type, data)| parse_registry_guids(*value_type, data))
        .find(|guids| !guids.is_empty())
        .unwrap_or_default()
}

fn registry_value(key: &RegistryKey, name: &str) -> Result<Option<(Dword, Vec<u16>)>, Error> {
    let name = wide(name);
    let mut value_type = 0;
    let mut size = 0;
    // SAFETY: this is a standard registry value size query.
    let result = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            ptr::null_mut(),
            &mut value_type,
            ptr::null_mut(),
            &mut size,
        )
    };
    if result as Dword == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != 0 {
        return Err(map_win32_error(result as Dword));
    }
    let mut data = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY: `data` is writable for `size` bytes.
    let result = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            ptr::null_mut(),
            &mut value_type,
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if result != 0 {
        return Err(map_win32_error(result as Dword));
    }
    data.truncate((size as usize).div_ceil(2));
    Ok(Some((value_type, data)))
}

fn interface_paths(guid: Guid) -> Result<Vec<Vec<u16>>, Error> {
    // SAFETY: arguments request all present interfaces registered for this GUID.
    let raw = unsafe {
        SetupDiGetClassDevsW(
            &guid,
            ptr::null(),
            ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    let set = DeviceInfoSet::new(raw)?;
    let mut paths = Vec::new();
    let mut index = 0;
    loop {
        // SAFETY: all-zero is valid once cbSize is populated.
        let mut interface: SpDeviceInterfaceData = unsafe { zeroed() };
        interface.size = size_of::<SpDeviceInterfaceData>() as Dword;
        // SAFETY: set, GUID, and output storage are valid.
        if unsafe {
            SetupDiEnumDeviceInterfaces(set.0, ptr::null_mut(), &guid, index, &mut interface)
        } == 0
        {
            // SAFETY: GetLastError describes enumeration failure.
            let error = unsafe { GetLastError() };
            if error == ERROR_NO_MORE_ITEMS {
                break;
            }
            return Err(map_win32_error(error));
        }
        index += 1;
        let mut required = 0;
        // SAFETY: this query obtains the required detail buffer size.
        unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &mut interface,
                ptr::null_mut(),
                0,
                &mut required,
                ptr::null_mut(),
            );
        }
        // SAFETY: GetLastError describes the size query.
        if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        let word_count = (required as usize).div_ceil(size_of::<usize>());
        let mut storage = vec![0usize; word_count];
        let detail = storage.as_mut_ptr().cast::<u8>();
        // SP_DEVICE_INTERFACE_DETAIL_DATA_W.cbSize is 8 on all supported 64-bit Windows targets.
        // SAFETY: storage is aligned and large enough for the returned detail structure.
        unsafe { detail.cast::<Dword>().write(8) };
        // SAFETY: all input and output structures are valid for `required` bytes.
        if unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &mut interface,
                detail.cast(),
                required,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } == 0
        {
            // SAFETY: GetLastError describes the detail read failure.
            return Err(map_win32_error(unsafe { GetLastError() }));
        }
        // The UTF-16 DevicePath field starts immediately after the DWORD cbSize.
        // SAFETY: SetupAPI wrote a null-terminated path within `required` bytes.
        let path = unsafe {
            std::slice::from_raw_parts(
                detail.add(size_of::<Dword>()).cast::<u16>(),
                (required as usize - size_of::<Dword>()) / 2,
            )
        };
        let end = path
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(path.len());
        let mut path = path[..end].to_vec();
        path.push(0);
        paths.push(path);
    }
    Ok(paths)
}

fn parse_hardware_ids(data: &[u16]) -> Option<(u16, u16)> {
    wide_strings(data).find_map(|id| {
        let upper = id.to_ascii_uppercase();
        let vendor = find_hex_component(&upper, "VID_")?;
        let product = find_hex_component(&upper, "PID_")?;
        Some((vendor, product))
    })
}

fn find_hex_component(value: &str, marker: &str) -> Option<u16> {
    let start = value.find(marker)? + marker.len();
    let digits = value.get(start..start + 4)?;
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(digits, 16).ok()
}

fn parse_registry_guids(value_type: Dword, data: &[u16]) -> Vec<Guid> {
    if value_type != REG_SZ && value_type != REG_MULTI_SZ {
        return Vec::new();
    }
    wide_strings(data)
        .filter_map(|value| parse_guid(&value))
        .collect()
}

fn parse_guid(value: &str) -> Option<Guid> {
    let value = value.trim();
    if value.len() != 38 || !value.starts_with('{') || !value.ends_with('}') {
        return None;
    }
    let value = &value[1..value.len() - 1];
    let mut parts = value.split('-');
    let first = parts.next()?;
    let second = parts.next()?;
    let third = parts.next()?;
    let fourth = parts.next()?;
    let fifth = parts.next()?;
    if parts.next().is_some()
        || first.len() != 8
        || second.len() != 4
        || third.len() != 4
        || fourth.len() != 4
        || fifth.len() != 12
    {
        return None;
    }
    let data1 = u32::from_str_radix(first, 16).ok()?;
    let data2 = u16::from_str_radix(second, 16).ok()?;
    let data3 = u16::from_str_radix(third, 16).ok()?;
    let tail = format!("{fourth}{fifth}");
    let mut data4 = [0; 8];
    for (index, byte) in data4.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&tail[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(Guid {
        data1,
        data2,
        data3,
        data4,
    })
}

fn path_matches_ids(path: &[u16], vendor_id: u16, product_id: u16) -> bool {
    let path = OsString::from_wide(path)
        .to_string_lossy()
        .to_ascii_uppercase();
    path.contains(&format!("VID_{vendor_id:04X}"))
        && path.contains(&format!("PID_{product_id:04X}"))
}

fn wide_strings(data: &[u16]) -> impl Iterator<Item = String> + '_ {
    data.split(|unit| *unit == 0)
        .filter(|value| !value.is_empty())
        .map(|value| OsString::from_wide(value).to_string_lossy().into_owned())
}

fn bytes_to_wide(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

const fn timeout_millis(timeout: Duration) -> Dword {
    let milliseconds = timeout.as_millis();
    if milliseconds >= INFINITE as u128 {
        INFINITE - 1
    } else {
        milliseconds as Dword
    }
}

const fn map_win32_error(code: Dword) -> Error {
    match code {
        ERROR_ACCESS_DENIED => Error::Access,
        ERROR_BUSY => Error::Busy,
        ERROR_FILE_NOT_FOUND => Error::NotFound,
        ERROR_DEVICE_NOT_CONNECTED => Error::NoDevice,
        ERROR_SEM_TIMEOUT | ERROR_TIMEOUT => Error::Timeout,
        ERROR_INVALID_PARAMETER => Error::InvalidParam,
        ERROR_NOT_ENOUGH_MEMORY => Error::NoMem,
        ERROR_NOT_SUPPORTED => Error::NotSupported,
        ERROR_OPERATION_ABORTED => Error::Interrupted,
        ERROR_GEN_FAILURE => Error::Pipe,
        _ => Error::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16_multi(values: &[&str]) -> Vec<u16> {
        values
            .iter()
            .flat_map(|value| value.encode_utf16().chain([0]))
            .chain([0])
            .collect()
    }

    #[test]
    fn parses_hardware_ids_and_rejects_malformed_values() {
        let ids = utf16_multi(&["USB\\VID_16C0&PID_05DC&REV_0100"]);
        assert_eq!(parse_hardware_ids(&ids), Some((0x16c0, 0x05dc)));
        assert_eq!(parse_hardware_ids(&utf16_multi(&["USB\\VID_X&C"])), None);
        assert_eq!(parse_hardware_ids(&utf16_multi(&["USB\\PID_05DC"])), None);
    }

    #[test]
    fn parses_reg_sz_and_reg_multi_sz_guids() {
        let first = "{0DD9BE09-BBEA-44A0-AB59-2F098406949C}";
        let second = "{12345678-1234-5678-90AB-CDEF01234567}";
        assert_eq!(
            parse_registry_guids(REG_SZ, &utf16_multi(&[first])).len(),
            1
        );
        let parsed = parse_registry_guids(REG_MULTI_SZ, &utf16_multi(&[first, second]));
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].data1, 0x0dd9_be09);
        assert!(parse_registry_guids(4, &utf16_multi(&[first])).is_empty());
        let values = [
            (REG_MULTI_SZ, utf16_multi(&[])),
            (REG_SZ, utf16_multi(&[second])),
        ];
        assert_eq!(first_registry_guids(&values)[0].data1, 0x1234_5678);
    }

    #[test]
    fn maps_win32_errors() {
        assert_eq!(map_win32_error(ERROR_ACCESS_DENIED), Error::Access);
        assert_eq!(map_win32_error(ERROR_BUSY), Error::Busy);
        assert_eq!(map_win32_error(ERROR_DEVICE_NOT_CONNECTED), Error::NoDevice);
        assert_eq!(map_win32_error(ERROR_TIMEOUT), Error::Timeout);
        assert_eq!(map_win32_error(0xffff), Error::Other);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn windows_64_bit_ffi_layouts_match_sdk() {
        assert_eq!(size_of::<Guid>(), 16);
        assert_eq!(align_of::<Guid>(), 4);
        assert_eq!(size_of::<SpDevinfoData>(), 32);
        assert_eq!(align_of::<SpDevinfoData>(), 8);
        assert_eq!(size_of::<SpDeviceInterfaceData>(), 32);
        assert_eq!(align_of::<SpDeviceInterfaceData>(), 8);
        assert_eq!(size_of::<Overlapped>(), 32);
        assert_eq!(align_of::<Overlapped>(), 8);
        assert_eq!(size_of::<WinusbSetupPacket>(), 8);
    }

    #[test]
    fn converts_and_saturates_timeouts() {
        assert_eq!(timeout_millis(Duration::ZERO), 0);
        assert_eq!(timeout_millis(Duration::from_millis(500)), 500);
        assert_eq!(timeout_millis(Duration::MAX), INFINITE - 1);
    }
}
