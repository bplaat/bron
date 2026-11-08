/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The controls of an edit tool in a card over the image, like a flyout that stays open while the
//! image is used: XAML generated from the controls of the tool, with Reset, Cancel and apply
//! buttons. Every change of a control sends all values to the application, which previews them.

use std::cell::Cell;
use std::ffi::c_void;
use std::fmt::Write;
use std::rc::{Rc, Weak};

use bwindow::WindowId;
use imageview_core::edit::{Control, Tool};

use super::headers::{w, wide};
use super::winrt::*;
use super::winui::{add_event, boxed_bool, escape_xml, find_name, load_xaml};
use crate::AppEvent;

// The largest size of an image side.
const MAXIMUM_LENGTH: f64 = 1_000_000.0;
const SLIDER_WIDTH: f64 = 180.0;

// The XAML of the card of a tool: its title, a grid of labeled controls named `Control` and
// their index, slider values named `Value` and their index, and the buttons.
fn xaml(tool: Tool, controls: &[Control]) -> String {
    let mut rows = String::new();
    let mut row = 0;
    for (index, control) in controls.iter().enumerate() {
        if let Control::Slider(parameter) = control
            && !parameter.section.is_empty()
        {
            _ = write!(
                rows,
                r#"<TextBlock Grid.Row="{row}" Grid.ColumnSpan="3" Margin="0,8,0,0" Text="{}" Style="{{StaticResource BodyStrongTextBlockStyle}}" />"#,
                escape_xml(parameter.section)
            );
            row += 1;
        }
        let label = |text: &str| {
            format!(
                r#"<TextBlock Grid.Row="{row}" VerticalAlignment="Center" HorizontalAlignment="Right" Text="{}:" />"#,
                escape_xml(text)
            )
        };
        let name = format!(r#"x:Name="Control{index}" Grid.Row="{row}" Grid.Column="1""#);
        match control {
            Control::Slider(parameter) => {
                let range = parameter.max - parameter.min;
                _ = write!(
                    rows,
                    r#"{}<Slider {name} Width="{SLIDER_WIDTH}" VerticalAlignment="Center" Minimum="{}" Maximum="{}" StepFrequency="{}" /><TextBlock x:Name="Value{index}" Grid.Row="{row}" Grid.Column="2" MinWidth="36" VerticalAlignment="Center" TextAlignment="Right" />"#,
                    label(parameter.label),
                    parameter.min,
                    parameter.max,
                    if range <= 10.0 { 0.1 } else { 1.0 },
                );
            }
            Control::Number { label: text } => {
                _ = write!(
                    rows,
                    r#"{}<NumberBox {name} SpinButtonPlacementMode="Inline" Minimum="1" Maximum="{MAXIMUM_LENGTH}" /><TextBlock Grid.Row="{row}" Grid.Column="2" VerticalAlignment="Center" Text="pixels" />"#,
                    label(text)
                );
            }
            Control::Choice {
                label: text,
                options,
            } => {
                let items: String = options
                    .iter()
                    .map(|option| format!(r#"<ComboBoxItem Content="{}" />"#, escape_xml(option)))
                    .collect();
                _ = write!(
                    rows,
                    r#"{}<ComboBox {name} Grid.ColumnSpan="2" MinWidth="{SLIDER_WIDTH}">{items}</ComboBox>"#,
                    label(text)
                );
            }
            Control::Toggle { label: text } => {
                _ = write!(
                    rows,
                    r#"<CheckBox {name} Grid.ColumnSpan="2" Content="{}" />"#,
                    escape_xml(text)
                );
            }
        }
        row += 1;
    }
    let definitions = r#"<RowDefinition Height="Auto" />"#.repeat(row);
    let reset = if tool.previews() {
        r#"<Button x:Name="Reset" Content="Reset" />"#
    } else {
        ""
    };
    format!(
        r#"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" RowSpacing="12">
    <Grid.RowDefinitions>
        <RowDefinition Height="Auto" />
        <RowDefinition Height="*" />
        <RowDefinition Height="Auto" />
    </Grid.RowDefinitions>
    <TextBlock Text="{}" Style="{{StaticResource BodyStrongTextBlockStyle}}" />
    <ScrollViewer Grid.Row="1" VerticalScrollBarVisibility="Auto" Padding="0,0,12,0">
        <Grid ColumnSpacing="8" RowSpacing="8">
            <Grid.ColumnDefinitions>
                <ColumnDefinition Width="Auto" />
                <ColumnDefinition Width="*" />
                <ColumnDefinition Width="Auto" />
            </Grid.ColumnDefinitions>
            <Grid.RowDefinitions>{definitions}</Grid.RowDefinitions>
            {rows}
        </Grid>
    </ScrollViewer>
    <Grid Grid.Row="2" ColumnSpacing="8">
        <Grid.ColumnDefinitions>
            <ColumnDefinition Width="*" />
            <ColumnDefinition Width="Auto" />
            <ColumnDefinition Width="Auto" />
        </Grid.ColumnDefinitions>
        {reset}
        <Button x:Name="Cancel" Grid.Column="1" Content="Cancel">
            <Button.KeyboardAccelerators>
                <KeyboardAccelerator Key="Escape" />
            </Button.KeyboardAccelerators>
        </Button>
        <Button x:Name="Apply" Grid.Column="2" Content="{}" Style="{{StaticResource AccentButtonStyle}}">
            <Button.KeyboardAccelerators>
                <KeyboardAccelerator Key="Enter" />
            </Button.KeyboardAccelerators>
        </Button>
    </Grid>
</Grid>"#,
        escape_xml(tool.title()),
        escape_xml(tool.apply_label()),
    )
}

// A control with the interface its value is read and changed through, and the label of the
// value of a slider.
struct Entry {
    control: Control,
    interface: ComPtr,
    value: Option<ComPtr>,
}

struct State {
    // The window the changes go to.
    window: WindowId,
    entries: Vec<Entry>,
    defaults: Vec<f32>,
    // Whether values are being shown, which does not count as a change.
    setting: Cell<bool>,
}

impl State {
    fn changed(&self, index: usize) {
        if self.setting.get() {
            return;
        }
        let values = self.values();
        self.show_values(&values);
        super::send(AppEvent::ToolChanged(self.window, index, values));
    }

    // The value of every control.
    fn values(&self) -> Vec<f32> {
        self.entries
            .iter()
            .map(|entry| {
                let interface = &entry.interface;
                // SAFETY: The interface of every entry is the one of its kind of control: slot 14
                // of IRangeBase and slot 10 of INumberBox are get_Value, slot 6 of ISelector is
                // get_SelectedIndex and slot 6 of IToggleButton is get_IsChecked, an
                // IReference<Boolean> with get_Value in slot 6.
                unsafe {
                    match entry.control {
                        Control::Slider(_) => {
                            let mut value = 0.0f64;
                            call!(interface, 14, &mut value => *mut f64);
                            value as f32
                        }
                        // An empty field is not a number.
                        Control::Number { .. } => {
                            let mut value = 0.0f64;
                            call!(interface, 10, &mut value => *mut f64);
                            if value.is_nan() {
                                0.0
                            } else {
                                value.round() as f32
                            }
                        }
                        Control::Choice { .. } => {
                            let mut index = 0i32;
                            call!(interface, 6, &mut index => *mut i32);
                            index.max(0) as f32
                        }
                        Control::Toggle { .. } => interface.get(6).map_or(0.0, |checked| {
                            let mut value = false;
                            call!(checked, 6, &mut value => *mut bool);
                            f32::from(u8::from(value))
                        }),
                    }
                }
            })
            .collect()
    }

    // Shows values in the controls that differ.
    fn set_values(&self, values: &[f32]) {
        self.setting.set(true);
        let current = self.values();
        for ((entry, value), current) in self.entries.iter().zip(values).zip(current) {
            if *value == current {
                continue;
            }
            let interface = &entry.interface;
            // SAFETY: The interface of every entry is the one of its kind of control: slot 15 of
            // IRangeBase and slot 11 of INumberBox are put_Value, slot 7 of ISelector is
            // put_SelectedIndex and slot 7 of IToggleButton is put_IsChecked.
            unsafe {
                match entry.control {
                    Control::Slider(_) => call!(interface, 15, f64::from(*value) => f64),
                    Control::Number { .. } => {
                        call!(interface, 11, f64::from(value.round()) => f64)
                    }
                    Control::Choice { .. } => call!(interface, 7, *value as i32 => i32),
                    Control::Toggle { .. } => match boxed_bool(*value != 0.0) {
                        Some(checked) => call!(interface, 7, checked.as_raw() => *mut c_void),
                        None => 0,
                    },
                };
            }
        }
        self.setting.set(false);
        self.show_values(values);
    }

    // Shows the slider values next to them.
    fn show_values(&self, values: &[f32]) {
        for (entry, value) in self.entries.iter().zip(values) {
            if let (Control::Slider(parameter), Some(label)) = (entry.control, &entry.value) {
                let text = HString::new(&parameter.value_text(*value));
                // SAFETY: Slot 27 of ITextBlock is put_Text.
                _ = unsafe { label.put(27, text.as_raw()) };
            }
        }
    }
}

/// The controls of the open tool, which the card shows until this is dropped.
pub(super) struct ToolPanel {
    content: ComPtr,
    state: Rc<State>,
}

impl ToolPanel {
    /// Creates the controls of a tool showing values for `window`, reset goes back to the
    /// defaults.
    pub(super) fn new(
        window: WindowId,
        tool: Tool,
        controls: &[Control],
        values: &[f32],
        defaults: &[f32],
    ) -> Result<Self> {
        let content = load_xaml(&xaml(tool, controls))?.cast(&IID_IFRAMEWORK_ELEMENT)?;
        let entries = controls
            .iter()
            .enumerate()
            .map(|(index, control)| {
                let iid = match control {
                    Control::Slider(_) => &IID_IRANGE_BASE,
                    Control::Number { .. } => &IID_INUMBER_BOX,
                    Control::Choice { .. } => &IID_ISELECTOR,
                    Control::Toggle { .. } => &IID_ITOGGLE_BUTTON,
                };
                Ok(Entry {
                    control: *control,
                    interface: find_name(&content, &wide(&format!("Control{index}")), iid)?,
                    value: matches!(control, Control::Slider(_))
                        .then(|| {
                            find_name(&content, &wide(&format!("Value{index}")), &IID_ITEXT_BLOCK)
                        })
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let state = Rc::new(State {
            window,
            entries,
            defaults: defaults.to_vec(),
            setting: Cell::new(false),
        });
        state.set_values(values);

        // The handlers keep the state weakly, the controls they belong to are part of it.
        let weak = Rc::downgrade(&state);
        let changed = |index: usize| {
            let weak: Weak<State> = weak.clone();
            move |_: &ComPtr, _: Option<&ComPtr>| {
                if let Some(state) = weak.upgrade() {
                    state.changed(index);
                }
            }
        };
        for (index, entry) in state.entries.iter().enumerate() {
            // Slot 16 of IRangeBase and ISelector and slot 44 of INumberBox are their changed
            // events, slot 14 of IButtonBase is add_Click.
            let (interface, slot, iid) = match entry.control {
                Control::Slider(_) => (
                    entry.interface.clone(),
                    16,
                    IID_RANGE_BASE_VALUE_CHANGED_EVENT_HANDLER,
                ),
                Control::Number { .. } => (
                    entry.interface.clone(),
                    44,
                    IID_NUMBER_BOX_VALUE_CHANGED_EVENT_HANDLER,
                ),
                Control::Choice { .. } => (
                    entry.interface.clone(),
                    16,
                    IID_SELECTION_CHANGED_EVENT_HANDLER,
                ),
                Control::Toggle { .. } => (
                    entry.interface.cast(&IID_IBUTTON_BASE)?,
                    14,
                    IID_ROUTED_EVENT_HANDLER,
                ),
            };
            add_event(&interface, slot, &event_handler(iid, changed(index)))?;
        }
        // Tools without a preview have nothing to reset.
        if let Ok(button) = find_name(&content, w!("Reset"), &IID_IBUTTON_BASE) {
            let reset = event_handler(IID_ROUTED_EVENT_HANDLER, move |_, _| {
                if let Some(state) = weak.upgrade() {
                    state.set_values(&state.defaults);
                    state.changed(usize::MAX);
                }
            });
            add_event(&button, 14, &reset)?;
        }
        for (name, action) in [(w!("Cancel"), "tool.cancel"), (w!("Apply"), "tool.apply")] {
            let button = find_name(&content, name, &IID_IBUTTON_BASE)?;
            let clicked = event_handler(IID_ROUTED_EVENT_HANDLER, move |_, _| {
                super::send(AppEvent::Action(action));
            });
            add_event(&button, 14, &clicked)?;
        }
        Ok(Self { content, state })
    }

    /// The root element of the controls.
    pub(super) const fn content(&self) -> &ComPtr {
        &self.content
    }

    /// Shows changed values, like the other side of a proportional resize.
    pub(super) fn set_values(&self, values: &[f32]) {
        self.state.set_values(values);
    }
}

#[cfg(test)]
mod tests {
    use imageview_core::edit::Filter;

    use super::*;

    #[test]
    fn tools_are_well_formed_xaml_with_their_controls() {
        for tool in [
            Tool::Crop,
            Tool::Resize,
            Tool::Filter(Filter::Adjust),
            Tool::Filter(Filter::Sharpen),
        ] {
            let controls = tool.controls(true);
            let xaml = xaml(tool, &controls);
            for index in 0..controls.len() {
                assert!(
                    xaml.contains(&format!(r#"x:Name="Control{index}""#)),
                    "{tool:?}"
                );
            }
            assert_eq!(xaml.matches('<').count(), xaml.matches('>').count());
            assert!(!xaml.contains("{{"));
        }
        assert_eq!(
            escape_xml(r#"a & "b" <c>"#),
            "a &amp; &quot;b&quot; &lt;c&gt;"
        );
        assert_eq!(escape_xml("{draft}\u{1}.png"), "{}{draft}.png");
    }
}
