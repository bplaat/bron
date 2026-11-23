/*
 * Copyright (c) 2023-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::env;
use std::fmt::{self, Display};
use std::io::{self, IsTerminal};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};

use log::{info, trace, warn};
use serde::{Deserialize, Serialize};

use crate::config::{Config, DMX_LENGTH, DMX_SWITCHES_LENGTH, FixtureType};
use crate::ipc::{self, UsbStatus};
use crate::usb;

// MARK: Color
#[derive(Debug, Copy, Clone)]
pub(crate) struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub(crate) const BLACK: Color = Color { r: 0, g: 0, b: 0 };

    pub(crate) fn apply_intensity(self, intensity: f32) -> Color {
        Color {
            r: (self.r as f32 * intensity) as u8,
            g: (self.g as f32 * intensity) as u8,
            b: (self.b as f32 * intensity) as u8,
        }
    }

    pub(crate) fn tween(self, other: Color, t: f32) -> Color {
        Color {
            r: (self.r as f32 + (other.r as f32 - self.r as f32) * t) as u8,
            g: (self.g as f32 + (other.g as f32 - self.g as f32) * t) as u8,
            b: (self.b as f32 + (other.b as f32 - self.b as f32) * t) as u8,
        }
    }
}

impl Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl Serialize for Color {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value: u32 = ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32);
        serializer.serialize_u32(value)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Ok(Color {
            r: ((value >> 16) & 0xFF) as u8,
            g: ((value >> 8) & 0xFF) as u8,
            b: (value & 0xFF) as u8,
        })
    }
}

// MARK: Mode
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    Black,
    Manual,
    Auto,
}

// MARK: ToggleTween
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ToggleTween {
    Direct,
    Linear,
    Ease,
}

// MARK: DmxState
#[derive(Clone)]
pub(crate) struct DmxState {
    pub is_running: bool,
    pub mode: Mode,
    pub color: Color,
    pub toggle_color: Color,
    pub intensity: f32,
    pub toggle_tween: ToggleTween,
    pub toggle_speed: Option<Duration>,
    pub strobe_speed: Option<Duration>,
    pub switches_toggle: [bool; DMX_SWITCHES_LENGTH],
    pub switches_press: [bool; DMX_SWITCHES_LENGTH],
}

pub(crate) static DMX_STATE: Mutex<DmxState> = Mutex::new(DmxState {
    is_running: false,
    mode: Mode::Manual,
    color: Color::BLACK,
    toggle_tween: ToggleTween::Direct,
    toggle_color: Color::BLACK,
    intensity: 1.0,
    toggle_speed: None,
    strobe_speed: None,
    switches_toggle: [false; DMX_SWITCHES_LENGTH],
    switches_press: [false; DMX_SWITCHES_LENGTH],
});

// MARK: DMX Thread
pub(crate) fn dmx_thread(config: Config) {
    let mut connection = usb::UdmxConnection::new(Instant::now());
    let frame_interval = Duration::from_secs_f64(1.0 / config.dmx_fps.max(1) as f64);

    // Only send channels up to the last one used by a fixture to keep transfers short
    let send_length = config
        .fixtures
        .iter()
        .map(|f| f.addr - 1 + f.r#type.channel_count())
        .max()
        .unwrap_or(config.dmx_length)
        .min(config.dmx_length)
        .min(DMX_LENGTH);

    let mut dmx = vec![0u8; config.dmx_length];
    let mut previous_toggle_speed = None;
    let mut toggle_color_time = Instant::now();
    let mut is_toggle_color = false;
    let mut strobe_time = Instant::now();
    let mut is_strobe = false;
    let use_colors = io::stdout().is_terminal()
        && env::var_os("NO_COLOR").is_none()
        && env::var_os("CI").is_none();

    loop {
        log_connection_event(connection.poll(Instant::now()));
        let dmx_state = DMX_STATE.lock().expect("Failed to lock DMX state").clone();
        if !dmx_state.is_running {
            // FIXME: Create async framework don't do micro sleeps
            sleep(Duration::from_millis(100));
            continue;
        }
        let frame_started = Instant::now();

        // Update timers
        if previous_toggle_speed != dmx_state.toggle_speed {
            previous_toggle_speed = dmx_state.toggle_speed;
            toggle_color_time = Instant::now();
            is_toggle_color = false;
        }
        if let Some(toggle_speed) = dmx_state.toggle_speed
            && toggle_color_time.elapsed() > toggle_speed
        {
            is_toggle_color = !is_toggle_color;
            toggle_color_time = Instant::now();
        }
        if let Some(strobe_speed) = dmx_state.strobe_speed
            && strobe_time.elapsed() > strobe_speed
        {
            is_strobe = !is_strobe;
            strobe_time = Instant::now();
        }

        // Update DMX data
        dmx.fill(0);
        let mut dump_color_state = false;
        for fixture in &config.fixtures {
            match fixture.r#type {
                FixtureType::AmericanDJP56Led
                | FixtureType::AmericanDJMegaTripar
                | FixtureType::AyraCompar10
                | FixtureType::AyraCompar20 => {
                    let base_addr = fixture.addr - 1;
                    let color = if is_strobe {
                        Color::BLACK
                    } else {
                        match dmx_state.toggle_tween {
                            ToggleTween::Direct => {
                                if is_toggle_color {
                                    dmx_state.toggle_color
                                } else {
                                    dmx_state.color
                                }
                            }
                            ToggleTween::Linear => {
                                let t = if let Some(toggle_speed) = dmx_state.toggle_speed {
                                    toggle_color_time.elapsed().as_secs_f32()
                                        / toggle_speed.as_secs_f32()
                                } else {
                                    0.0
                                };
                                if is_toggle_color {
                                    dmx_state.color.tween(dmx_state.toggle_color, t)
                                } else {
                                    dmx_state.toggle_color.tween(dmx_state.color, t)
                                }
                            }
                            ToggleTween::Ease => {
                                let t = if let Some(toggle_speed) = dmx_state.toggle_speed {
                                    toggle_color_time.elapsed().as_secs_f32()
                                        / toggle_speed.as_secs_f32()
                                } else {
                                    0.0
                                };
                                let t = t * t * (3.0 - 2.0 * t);
                                if is_toggle_color {
                                    dmx_state.color.tween(dmx_state.toggle_color, t)
                                } else {
                                    dmx_state.toggle_color.tween(dmx_state.color, t)
                                }
                            }
                        }
                    };
                    let color_with_intensity = color.apply_intensity(dmx_state.intensity);
                    if !dump_color_state {
                        dump_color_state = true;
                        if use_colors {
                            trace!(
                                "Color: \x1b[38;2;{};{};{}m{}\x1b[0m",
                                color_with_intensity.r,
                                color_with_intensity.g,
                                color_with_intensity.b,
                                color_with_intensity
                            );
                        } else {
                            trace!("Color: {color_with_intensity}");
                        }
                    }

                    // American DJ P56 LED
                    if fixture.r#type == FixtureType::AmericanDJP56Led {
                        if dmx_state.mode == Mode::Manual {
                            dmx[base_addr] = color_with_intensity.r;
                            dmx[base_addr + 1] = color_with_intensity.g;
                            dmx[base_addr + 2] = color_with_intensity.b;
                        }
                        if dmx_state.mode == Mode::Auto {
                            dmx[base_addr + 5] = 224;
                        }
                    }

                    // American DJ Mega Tripar
                    if fixture.r#type == FixtureType::AmericanDJMegaTripar {
                        if dmx_state.mode == Mode::Manual {
                            dmx[base_addr] = color.r;
                            dmx[base_addr + 1] = color.g;
                            dmx[base_addr + 2] = color.b;
                            dmx[base_addr + 6] = (dmx_state.intensity * 255.0) as u8;
                        }
                        if dmx_state.mode == Mode::Auto {
                            dmx[base_addr + 5] = 240;
                        }
                    }

                    // Ayra Compar 10
                    if fixture.r#type == FixtureType::AyraCompar10 {
                        if dmx_state.mode == Mode::Manual {
                            dmx[base_addr] = (dmx_state.intensity * 255.0) as u8;
                            dmx[base_addr + 2] = color.r;
                            dmx[base_addr + 3] = color.g;
                            dmx[base_addr + 4] = color.b;
                        }
                        if dmx_state.mode == Mode::Auto {
                            dmx[base_addr + 7] = 221;
                        }
                    }

                    // Ayra Compar 20
                    if fixture.r#type == FixtureType::AyraCompar20 {
                        if dmx_state.mode == Mode::Manual {
                            dmx[base_addr] = (dmx_state.intensity * 255.0) as u8;
                            dmx[base_addr + 2] = color.r;
                            dmx[base_addr + 3] = color.g;
                            dmx[base_addr + 4] = color.b;
                        }
                        if dmx_state.mode == Mode::Auto {
                            dmx[base_addr + 5] = 221;
                        }
                    }
                }

                FixtureType::ShowtecMultidimMKII => {
                    let base_addr = fixture.addr - 1;
                    if dmx_state.mode == Mode::Manual {
                        for i in 0..DMX_SWITCHES_LENGTH {
                            if dmx_state.switches_toggle[i] || dmx_state.switches_press[i] {
                                dmx[base_addr + i] = 255;
                            } else {
                                dmx[base_addr + i] = 0;
                            }
                        }
                    } else {
                        for i in 0..DMX_SWITCHES_LENGTH {
                            dmx[base_addr + i] = 0;
                        }
                    }
                }
            }
        }

        log_connection_event(connection.send(Instant::now(), &dmx[..send_length]));
        sleep(frame_interval.saturating_sub(frame_started.elapsed()));
    }
}

fn log_connection_event(event: Option<usb::ConnectionEvent>) {
    match event {
        Some(usb::ConnectionEvent::Connected) => {
            info!("uDMX device connected");
            ipc::set_usb_status(UsbStatus::Connected);
        }
        Some(usb::ConnectionEvent::Recovered) => {
            info!("uDMX communication recovered");
            ipc::set_usb_status(UsbStatus::Connected);
        }
        Some(usb::ConnectionEvent::Disconnected(category)) => {
            warn!("uDMX device disconnected: {category}");
            ipc::set_usb_status(status_for_error(category));
        }
        Some(usb::ConnectionEvent::Error(category)) => {
            warn!("uDMX error: {category}");
            ipc::set_usb_status(status_for_error(category));
        }
        None => {}
    }
}

const fn status_for_error(category: usb::ErrorCategory) -> UsbStatus {
    if matches!(category, usb::ErrorCategory::NoDevice) {
        UsbStatus::Disconnected
    } else {
        UsbStatus::Error(category)
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_connection_failures_to_gui_status() {
        assert_eq!(
            status_for_error(usb::ErrorCategory::NoDevice),
            UsbStatus::Disconnected
        );
        assert_eq!(
            status_for_error(usb::ErrorCategory::Access),
            UsbStatus::Error(usb::ErrorCategory::Access)
        );
    }
}
