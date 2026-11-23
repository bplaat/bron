/*
 * Copyright (c) 2023-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::io;
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::time::Duration;

use bwindow::EventLoopProxy;
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use small_websocket::{Message, WebSocket};

use crate::CONFIG;
use crate::config::FixtureType;
use crate::dmx::{Color, DMX_STATE, Mode, ToggleTween};
use crate::usb::ErrorCategory;

// MARK: UsbStatus
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", content = "category", rename_all = "camelCase")]
pub(crate) enum UsbStatus {
    Connected,
    Disconnected,
    Error(ErrorCategory),
}

pub(crate) static USB_STATUS: Mutex<UsbStatus> = Mutex::new(UsbStatus::Disconnected);

static USB_STATUS_SENDER: LazyLock<Option<mpsc::Sender<String>>> = LazyLock::new(|| {
    let (sender, receiver) = mpsc::channel::<String>();
    std::thread::Builder::new()
        .name("usb-status-ipc".to_string())
        .spawn(move || {
            for message in receiver {
                send_to_connections(None, &message);
            }
        })
        .ok()
        .map(|_| sender)
});

pub(crate) fn set_usb_status(status: UsbStatus) {
    let mut current = USB_STATUS.lock().expect("Failed to lock USB status");
    if *current == status {
        return;
    }
    *current = status;
    drop(current);

    let message = serde_json::to_string(&IpcMessage::UsbStatusChanged { status })
        .expect("Failed to serialize USB status");
    if USB_STATUS_SENDER
        .as_ref()
        .is_none_or(|sender| sender.send(message).is_err())
    {
        warn!("USB status IPC thread stopped");
    }
}

// MARK: IpcMessage
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum IpcMessage {
    // State
    Start,
    Stop,
    GetState,
    GetStateResponse {
        state: State,
    },
    GetUsbStatus,
    GetUsbStatusResponse {
        status: UsbStatus,
    },
    UsbStatusChanged {
        status: UsbStatus,
    },
    SetColor {
        color: Color,
    },
    SetToggleColor {
        #[serde(rename = "toggleColor")]
        toggle_color: Color,
    },
    SetIntensity {
        intensity: f32,
    },
    SetToggleTween {
        #[serde(rename = "toggleTween")]
        toggle_tween: ToggleTween,
    },
    SetToggleSpeed {
        #[serde(rename = "toggleSpeed")]
        toggle_speed: Option<u64>,
    },
    SetStrobeSpeed {
        #[serde(rename = "strobeSpeed")]
        strobe_speed: Option<u64>,
    },
    SetSwitchesToggle {
        #[serde(rename = "switchesToggle")]
        switches_toggle: [bool; 4],
    },
    SetSwitchesPress {
        #[serde(rename = "switchesPress")]
        switches_press: [bool; 4],
    },
    SetMode {
        mode: Mode,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct State {
    pub color: Color,
    pub toggle_color: Color,
    pub intensity: f32,
    pub toggle_tween: ToggleTween,
    pub toggle_speed: Option<u64>,
    pub strobe_speed: Option<u64>,
    pub mode: Mode,
    pub switches_labels: Option<Vec<String>>,
    pub switches_toggle: Vec<bool>,
    pub switches_press: Vec<bool>,
}

// MARK: IpcConnection
pub(crate) static IPC_CONNECTIONS: Mutex<Vec<IpcConnection>> = Mutex::new(Vec::new());

pub(crate) enum IpcConnection {
    WebviewIpc(Arc<EventLoopProxy<crate::AppEvent>>),
    WebSocket(WebSocket),
}

impl PartialEq for IpcConnection {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::WebviewIpc(_), Self::WebviewIpc(_)) => true,
            (Self::WebSocket(ws1), Self::WebSocket(ws2)) => ws1 == ws2,
            _ => false,
        }
    }
}
impl Eq for IpcConnection {}

impl IpcConnection {
    pub(crate) fn send(&mut self, message: String) -> io::Result<()> {
        match self {
            Self::WebviewIpc(event_loop_proxy) => {
                let _ = event_loop_proxy.send_user_event(crate::AppEvent::UserEvent(message));
                Ok(())
            }
            Self::WebSocket(ws) => ws.send(Message::Text(message)),
        }
    }

    pub(crate) fn broadcast(&mut self, message: String) {
        send_to_connections(Some(self), &message);
    }
}

fn send_to_connections(sender: Option<&IpcConnection>, message: &str) {
    IPC_CONNECTIONS
        .lock()
        .expect("Failed to lock IPC connections")
        .retain_mut(|connection| {
            if sender.is_some_and(|sender| connection == sender) {
                return true;
            }
            if let Err(error) = connection.send(message.to_string()) {
                warn!("Removing failed IPC connection: {error}");
                false
            } else {
                true
            }
        });
}

// MARK: IPC Message Handler
pub(crate) fn ipc_message_handler(mut connection: IpcConnection, message: &str) -> bool {
    let message = match parse_client_message(message) {
        Ok(message) => message,
        Err(error) => {
            warn!("Rejecting invalid IPC message: {error}");
            return false;
        }
    };
    let mut dmx_state = DMX_STATE.lock().expect("Failed to lock DMX state");
    debug!("Received IPC message: {message:?}");
    match message {
        // State
        IpcMessage::Start => {
            dmx_state.is_running = true;
            connection.broadcast(
                serde_json::to_string(&IpcMessage::Start).expect("Failed to serialize IPC message"),
            );
        }
        IpcMessage::Stop => {
            dmx_state.is_running = false;
            connection.broadcast(
                serde_json::to_string(&IpcMessage::Stop).expect("Failed to serialize IPC message"),
            );
        }
        IpcMessage::GetState => {
            let config = CONFIG.lock().expect("Failed to lock config");
            let state = State {
                color: dmx_state.color,
                toggle_color: dmx_state.toggle_color,
                intensity: dmx_state.intensity,
                toggle_tween: dmx_state.toggle_tween,
                toggle_speed: dmx_state.toggle_speed.map(|d| d.as_millis() as u64),
                strobe_speed: dmx_state.strobe_speed.map(|d| d.as_millis() as u64),
                mode: dmx_state.mode,
                switches_labels: config.as_ref().and_then(|c| {
                    c.fixtures
                        .iter()
                        .find(|f| f.r#type == FixtureType::ShowtecMultidimMKII)
                        .and_then(|f| f.switches.clone())
                }),
                switches_toggle: dmx_state.switches_toggle.to_vec(),
                switches_press: dmx_state.switches_press.to_vec(),
            };
            if connection
                .send(
                    serde_json::to_string(&IpcMessage::GetStateResponse { state })
                        .expect("Failed to serialize IPC response"),
                )
                .is_err()
            {
                return false;
            }
        }
        IpcMessage::GetUsbStatus => {
            let status = *USB_STATUS.lock().expect("Failed to lock USB status");
            if connection
                .send(
                    serde_json::to_string(&IpcMessage::GetUsbStatusResponse { status })
                        .expect("Failed to serialize USB status response"),
                )
                .is_err()
            {
                return false;
            }
        }

        IpcMessage::SetColor { color } => {
            dmx_state.color = color;
            connection.broadcast(
                serde_json::to_string(&IpcMessage::SetColor { color })
                    .expect("Failed to serialize IPC message"),
            );
        }
        IpcMessage::SetToggleColor { toggle_color } => {
            dmx_state.toggle_color = toggle_color;
            connection.broadcast(
                serde_json::to_string(&IpcMessage::SetToggleColor { toggle_color })
                    .expect("Failed to serialize IPC message"),
            );
        }
        IpcMessage::SetIntensity { intensity } => {
            dmx_state.intensity = intensity;
            connection.broadcast(
                serde_json::to_string(&IpcMessage::SetIntensity { intensity })
                    .expect("Failed to serialize IPC message"),
            );
        }
        IpcMessage::SetToggleTween { toggle_tween } => {
            dmx_state.toggle_tween = toggle_tween;
            connection.broadcast(
                serde_json::to_string(&IpcMessage::SetToggleTween { toggle_tween })
                    .expect("Failed to serialize IPC message"),
            );
        }
        IpcMessage::SetToggleSpeed { toggle_speed } => {
            dmx_state.toggle_speed = toggle_speed.map(Duration::from_millis);
            connection.broadcast(
                serde_json::to_string(&IpcMessage::SetToggleSpeed { toggle_speed })
                    .expect("Failed to serialize IPC message"),
            );
        }
        IpcMessage::SetStrobeSpeed { strobe_speed } => {
            dmx_state.strobe_speed = strobe_speed.map(Duration::from_millis);
            connection.broadcast(
                serde_json::to_string(&IpcMessage::SetStrobeSpeed { strobe_speed })
                    .expect("Failed to serialize IPC message"),
            );
        }
        IpcMessage::SetSwitchesToggle { switches_toggle } => {
            dmx_state.switches_toggle = switches_toggle;
            connection.broadcast(
                serde_json::to_string(&IpcMessage::SetSwitchesToggle { switches_toggle })
                    .expect("Failed to serialize IPC message"),
            );
        }
        IpcMessage::SetSwitchesPress { switches_press } => {
            dmx_state.switches_press = switches_press;
            connection.broadcast(
                serde_json::to_string(&IpcMessage::SetSwitchesPress { switches_press })
                    .expect("Failed to serialize IPC message"),
            );
        }
        IpcMessage::SetMode { mode } => {
            dmx_state.mode = mode;
            connection.broadcast(
                serde_json::to_string(&IpcMessage::SetMode { mode })
                    .expect("Failed to serialize IPC message"),
            );
        }

        IpcMessage::GetStateResponse { .. }
        | IpcMessage::GetUsbStatusResponse { .. }
        | IpcMessage::UsbStatusChanged { .. } => unreachable!(),
    }
    true
}

fn parse_client_message(message: &str) -> Result<IpcMessage, &'static str> {
    let message = serde_json::from_str(message).map_err(|_| "invalid JSON or message shape")?;
    if matches!(
        message,
        IpcMessage::GetStateResponse { .. }
            | IpcMessage::GetUsbStatusResponse { .. }
            | IpcMessage::UsbStatusChanged { .. }
    ) {
        Err("response-only message received from client")
    } else {
        Ok(message)
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_usb_status_messages() {
        let disconnected = serde_json::to_string(&IpcMessage::UsbStatusChanged {
            status: UsbStatus::Disconnected,
        })
        .expect("Failed to serialize disconnected status");
        assert_eq!(
            disconnected,
            r#"{"type":"usbStatusChanged","status":{"state":"disconnected"}}"#
        );

        let error = serde_json::to_string(&IpcMessage::UsbStatusChanged {
            status: UsbStatus::Error(ErrorCategory::Access),
        })
        .expect("Failed to serialize error status");
        assert_eq!(
            error,
            r#"{"type":"usbStatusChanged","status":{"state":"error","category":"access"}}"#
        );
    }

    #[test]
    fn rejects_malformed_and_response_only_client_messages() {
        assert!(parse_client_message("not JSON").is_err());
        assert!(
            parse_client_message(
                r#"{"type":"getUsbStatusResponse","status":{"state":"connected"}}"#
            )
            .is_err()
        );
        assert!(parse_client_message(r#"{"type":"getUsbStatus"}"#).is_ok());
    }
}
