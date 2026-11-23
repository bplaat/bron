/*
 * Copyright (c) 2023-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::time::{Duration, Instant};

use rusb::{Context, DeviceHandle, Error, UsbContext};
use serde::{Deserialize, Serialize};

const UDMX_VENDOR_ID: u16 = 0x16c0;
const UDMX_PRODUCT_ID: u16 = 0x05dc;
const UDMX_CONFIGURATION: u8 = 1;
const UDMX_INTERFACE: u8 = 0;
const UDMX_REQUEST: u8 = 0x02;
const UDMX_START_CHANNEL: u16 = 0;
const UDMX_MAX_CHANNELS: usize = 512;
const TRANSFER_TIMEOUT: Duration = Duration::from_millis(500);
const TRANSFER_ATTEMPTS: usize = 3;
const RECONNECT_INTERVAL: Duration = Duration::from_millis(500);
const TRANSIENT_FAILURE_LIMIT: u8 = 3;

trait ControlTransfer {
    fn write_control(
        &self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, Error>;
}

impl ControlTransfer for DeviceHandle<Context> {
    fn write_control(
        &self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
        timeout: Duration,
    ) -> Result<usize, Error> {
        self.write_control(request_type, request, value, index, data, timeout)
    }
}

fn write_udmx_frame(handle: &impl ControlTransfer, data: &[u8]) -> Result<usize, Error> {
    if data.is_empty() || data.len() > UDMX_MAX_CHANNELS {
        return Err(Error::InvalidParam);
    }
    let request_type = rusb::request_type(
        rusb::Direction::Out,
        rusb::RequestType::Vendor,
        rusb::Recipient::Device,
    );
    let mut last_error = Error::Other;
    for _ in 0..TRANSFER_ATTEMPTS {
        match handle.write_control(
            request_type,
            UDMX_REQUEST,
            data.len() as u16,
            UDMX_START_CHANNEL,
            data,
            TRANSFER_TIMEOUT,
        ) {
            Ok(transferred) if transferred == data.len() => return Ok(transferred),
            Ok(_) => last_error = Error::Io,
            Err(error) if is_transient(error) => last_error = error,
            Err(error) => return Err(error),
        }
    }
    Err(last_error)
}

trait UsbHandle {
    fn write_frame(&self, data: &[u8]) -> Result<usize, Error>;
}

struct NativeHandle(DeviceHandle<Context>);

impl UsbHandle for NativeHandle {
    fn write_frame(&self, data: &[u8]) -> Result<usize, Error> {
        write_udmx_frame(&self.0, data)
    }
}

trait UsbConnector {
    fn open(&mut self) -> Result<Option<Box<dyn UsbHandle>>, Error>;
}

struct NativeConnector;

impl UsbConnector for NativeConnector {
    fn open(&mut self) -> Result<Option<Box<dyn UsbHandle>>, Error> {
        let context = Context::new()?;
        for device in context.devices()?.iter() {
            let descriptor = device.device_descriptor()?;
            if descriptor.vendor_id() != UDMX_VENDOR_ID
                || descriptor.product_id() != UDMX_PRODUCT_ID
            {
                continue;
            }
            let handle = device.open()?;
            match handle.set_active_configuration(UDMX_CONFIGURATION) {
                Ok(()) | Err(Error::Busy) => {}
                Err(error) => return Err(error),
            }
            handle.claim_interface(UDMX_INTERFACE)?;
            return Ok(Some(Box::new(NativeHandle(handle))));
        }
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionEvent {
    Connected,
    Recovered,
    Disconnected(ErrorCategory),
    Error(ErrorCategory),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ErrorCategory {
    Access,
    Busy,
    NoDevice,
    Timeout,
    Pipe,
    Unsupported,
    Other,
}

impl From<Error> for ErrorCategory {
    fn from(error: Error) -> Self {
        match error {
            Error::Access => Self::Access,
            Error::Busy => Self::Busy,
            Error::NoDevice | Error::NotFound => Self::NoDevice,
            Error::Timeout => Self::Timeout,
            Error::Pipe => Self::Pipe,
            Error::NotSupported => Self::Unsupported,
            _ => Self::Other,
        }
    }
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Access => "access denied",
            Self::Busy => "device busy",
            Self::NoDevice => "device unavailable",
            Self::Timeout => "transfer timeout",
            Self::Pipe => "control pipe stalled",
            Self::Unsupported => "operation unsupported",
            Self::Other => "USB error",
        })
    }
}

pub(crate) struct UdmxConnection {
    connector: Box<dyn UsbConnector>,
    handle: Option<Box<dyn UsbHandle>>,
    next_attempt: Instant,
    consecutive_failures: u8,
    last_error: Option<ErrorCategory>,
}

impl UdmxConnection {
    pub(crate) fn new(now: Instant) -> Self {
        Self::with_connector(now, Box::new(NativeConnector))
    }

    fn with_connector(now: Instant, connector: Box<dyn UsbConnector>) -> Self {
        Self {
            connector,
            handle: None,
            next_attempt: now,
            consecutive_failures: 0,
            last_error: None,
        }
    }

    pub(crate) fn poll(&mut self, now: Instant) -> Option<ConnectionEvent> {
        if self.handle.is_some() || now < self.next_attempt {
            return None;
        }
        self.next_attempt = now + RECONNECT_INTERVAL;
        match self.connector.open() {
            Ok(Some(handle)) => {
                self.handle = Some(handle);
                self.consecutive_failures = 0;
                self.last_error = None;
                Some(ConnectionEvent::Connected)
            }
            Ok(None) => None,
            Err(error) => self.changed_error(error),
        }
    }

    pub(crate) fn send(&mut self, now: Instant, data: &[u8]) -> Option<ConnectionEvent> {
        let result = self.handle.as_ref()?.write_frame(data);
        match result {
            Ok(_) => {
                self.consecutive_failures = 0;
                self.last_error.take().map(|_| ConnectionEvent::Recovered)
            }
            Err(error @ (Error::NoDevice | Error::NotFound)) => self.disconnect(now, error.into()),
            Err(error) if is_transient(error) => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                let category = error.into();
                if self.consecutive_failures >= TRANSIENT_FAILURE_LIMIT {
                    self.disconnect(now, category)
                } else {
                    self.changed_category(category)
                }
            }
            Err(error) => self.changed_error(error),
        }
    }

    fn disconnect(&mut self, now: Instant, category: ErrorCategory) -> Option<ConnectionEvent> {
        self.handle = None;
        self.next_attempt = now + RECONNECT_INTERVAL;
        self.consecutive_failures = 0;
        self.last_error = Some(category);
        Some(ConnectionEvent::Disconnected(category))
    }

    fn changed_error(&mut self, error: Error) -> Option<ConnectionEvent> {
        self.changed_category(error.into())
    }

    fn changed_category(&mut self, category: ErrorCategory) -> Option<ConnectionEvent> {
        if self.last_error == Some(category) {
            None
        } else {
            self.last_error = Some(category);
            Some(ConnectionEvent::Error(category))
        }
    }
}

const fn is_transient(error: Error) -> bool {
    matches!(
        error,
        Error::Io
            | Error::Timeout
            | Error::Overflow
            | Error::Pipe
            | Error::Interrupted
            | Error::Other
    )
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use super::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Transfer {
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: Vec<u8>,
        timeout: Duration,
    }

    struct RecordingControl(RefCell<Option<Transfer>>);

    impl ControlTransfer for RecordingControl {
        fn write_control(
            &self,
            request_type: u8,
            request: u8,
            value: u16,
            index: u16,
            data: &[u8],
            timeout: Duration,
        ) -> Result<usize, Error> {
            self.0.replace(Some(Transfer {
                request_type,
                request,
                value,
                index,
                data: data.to_vec(),
                timeout,
            }));
            Ok(data.len())
        }
    }

    struct ScriptedControl {
        results: RefCell<VecDeque<Result<usize, Error>>>,
        calls: RefCell<usize>,
    }

    impl ControlTransfer for ScriptedControl {
        fn write_control(
            &self,
            _request_type: u8,
            _request: u8,
            _value: u16,
            _index: u16,
            data: &[u8],
            _timeout: Duration,
        ) -> Result<usize, Error> {
            *self.calls.borrow_mut() += 1;
            self.results
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(data.len()))
        }
    }

    struct FakeHandle {
        results: Rc<RefCell<VecDeque<Result<usize, Error>>>>,
        drops: Rc<RefCell<usize>>,
    }

    impl UsbHandle for FakeHandle {
        fn write_frame(&self, data: &[u8]) -> Result<usize, Error> {
            self.results
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(data.len()))
        }
    }

    impl Drop for FakeHandle {
        fn drop(&mut self) {
            *self.drops.borrow_mut() += 1;
        }
    }

    struct FakeConnector {
        available: Rc<RefCell<bool>>,
        opens: Rc<RefCell<usize>>,
        results: Rc<RefCell<VecDeque<Result<usize, Error>>>>,
        drops: Rc<RefCell<usize>>,
    }

    impl UsbConnector for FakeConnector {
        fn open(&mut self) -> Result<Option<Box<dyn UsbHandle>>, Error> {
            *self.opens.borrow_mut() += 1;
            if *self.available.borrow() {
                Ok(Some(Box::new(FakeHandle {
                    results: Rc::clone(&self.results),
                    drops: Rc::clone(&self.drops),
                })))
            } else {
                Ok(None)
            }
        }
    }

    type FakeParts = (
        UdmxConnection,
        Rc<RefCell<bool>>,
        Rc<RefCell<usize>>,
        Rc<RefCell<VecDeque<Result<usize, Error>>>>,
        Rc<RefCell<usize>>,
    );

    fn fake(now: Instant) -> FakeParts {
        let available = Rc::new(RefCell::new(false));
        let opens = Rc::new(RefCell::new(0));
        let results = Rc::new(RefCell::new(VecDeque::new()));
        let drops = Rc::new(RefCell::new(0));
        let connector = FakeConnector {
            available: Rc::clone(&available),
            opens: Rc::clone(&opens),
            results: Rc::clone(&results),
            drops: Rc::clone(&drops),
        };
        (
            UdmxConnection::with_connector(now, Box::new(connector)),
            available,
            opens,
            results,
            drops,
        )
    }

    #[test]
    fn sends_exact_udmx_setup_and_enforces_limit() {
        let control = RecordingControl(RefCell::new(None));
        let data = [1, 2, 3];
        assert_eq!(write_udmx_frame(&control, &data), Ok(3));
        assert_eq!(
            control.0.borrow().as_ref(),
            Some(&Transfer {
                request_type: 0x40,
                request: 0x02,
                value: 3,
                index: 0,
                data: data.to_vec(),
                timeout: Duration::from_millis(500),
            })
        );
        assert_eq!(write_udmx_frame(&control, &[]), Err(Error::InvalidParam));
        assert_eq!(
            write_udmx_frame(&control, &[0; UDMX_MAX_CHANNELS + 1]),
            Err(Error::InvalidParam)
        );
    }

    #[test]
    fn retries_transient_and_short_transfers() {
        let control = ScriptedControl {
            results: RefCell::new(VecDeque::from([Err(Error::Timeout), Ok(2), Ok(3)])),
            calls: RefCell::new(0),
        };
        assert_eq!(write_udmx_frame(&control, &[1, 2, 3]), Ok(3));
        assert_eq!(*control.calls.borrow(), 3);
    }

    #[test]
    fn does_not_retry_permanent_transfer_errors() {
        let control = ScriptedControl {
            results: RefCell::new(VecDeque::from([Err(Error::Access)])),
            calls: RefCell::new(0),
        };
        assert_eq!(write_udmx_frame(&control, &[1]), Err(Error::Access));
        assert_eq!(*control.calls.borrow(), 1);
    }

    #[test]
    fn starts_disconnected_and_connects_later() {
        let now = Instant::now();
        let (mut connection, available, opens, _, _) = fake(now);
        assert_eq!(connection.poll(now), None);
        assert_eq!(*opens.borrow(), 1);
        *available.borrow_mut() = true;
        assert_eq!(
            connection.poll(now + RECONNECT_INTERVAL),
            Some(ConnectionEvent::Connected)
        );
        assert_eq!(*opens.borrow(), 2);
    }

    #[test]
    fn unplug_drops_handle_and_reconnects_with_current_frame() {
        let now = Instant::now();
        let (mut connection, available, _, results, drops) = fake(now);
        *available.borrow_mut() = true;
        assert_eq!(connection.poll(now), Some(ConnectionEvent::Connected));
        results.borrow_mut().push_back(Err(Error::NoDevice));
        assert_eq!(
            connection.send(now, &[1]),
            Some(ConnectionEvent::Disconnected(ErrorCategory::NoDevice))
        );
        assert_eq!(*drops.borrow(), 1);
        assert_eq!(connection.send(now, &[2]), None);
        assert_eq!(
            connection.poll(now + RECONNECT_INTERVAL),
            Some(ConnectionEvent::Connected)
        );
        assert_eq!(connection.send(now + RECONNECT_INTERVAL, &[2]), None);
    }

    #[test]
    fn three_transient_failures_force_reopen_and_success_resets_count() {
        let now = Instant::now();
        let (mut connection, available, _, results, drops) = fake(now);
        *available.borrow_mut() = true;
        connection.poll(now);
        results.borrow_mut().extend([
            Err(Error::Timeout),
            Ok(1),
            Err(Error::Timeout),
            Err(Error::Timeout),
            Err(Error::Timeout),
        ]);
        assert_eq!(
            connection.send(now, &[0]),
            Some(ConnectionEvent::Error(ErrorCategory::Timeout))
        );
        assert_eq!(connection.send(now, &[0]), Some(ConnectionEvent::Recovered));
        assert_eq!(
            connection.send(now, &[0]),
            Some(ConnectionEvent::Error(ErrorCategory::Timeout))
        );
        assert_eq!(connection.send(now, &[0]), None);
        assert_eq!(
            connection.send(now, &[0]),
            Some(ConnectionEvent::Disconnected(ErrorCategory::Timeout))
        );
        assert_eq!(*drops.borrow(), 1);
        assert_eq!(
            connection.poll(now + RECONNECT_INTERVAL),
            Some(ConnectionEvent::Connected)
        );
    }

    #[test]
    fn repeated_error_categories_are_deduplicated() {
        let now = Instant::now();
        let (mut connection, available, _, results, _) = fake(now);
        *available.borrow_mut() = true;
        connection.poll(now);
        results
            .borrow_mut()
            .extend([Err(Error::Access), Err(Error::Access), Err(Error::Busy)]);
        assert_eq!(
            connection.send(now, &[0]),
            Some(ConnectionEvent::Error(ErrorCategory::Access))
        );
        assert_eq!(connection.send(now, &[0]), None);
        assert_eq!(
            connection.send(now, &[0]),
            Some(ConnectionEvent::Error(ErrorCategory::Busy))
        );
    }
}
