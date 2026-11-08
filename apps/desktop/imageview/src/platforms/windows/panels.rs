/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The panels beside the image, opened with their header bar buttons and hidden while editing:
//! a sidebar with the pages of a file with several, like the sizes of an icon, like a navigation
//! pane, and an inspector with the information about the image. Their content is XAML generated
//! from the pages and information. The sidebar starts open, the inspector closed.

use std::cell::{Cell, RefCell};
use std::fmt::Write;

use bwindow::WindowId;
use imageview_core::{InfoSection, Media, render_media};

use super::headers::wide;
use super::winrt::*;
use super::winui::{
    Bitmap, INSPECTOR_WIDTH, Island, Panel, SIDEBAR_WIDTH, add_event, escape_xml, find_name,
    load_xaml, set_image, set_visible,
};
use crate::AppEvent;

// The largest side of a page thumbnail in logical pixels, and how many pixels it has for each,
// so it stays sharp on high density displays.
const THUMBNAIL_SIZE: f64 = 112.0;
const THUMBNAIL_DENSITY: f64 = 2.0;

const XMLNS: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml""#;

// Whether a panel is open with its button, has something to show, and is hidden while editing.
struct PanelState {
    open: Cell<bool>,
    available: Cell<bool>,
    hidden: Cell<bool>,
}

impl PanelState {
    const fn new(open: bool, available: bool) -> Self {
        Self {
            open: Cell::new(open),
            available: Cell::new(available),
            hidden: Cell::new(false),
        }
    }

    const fn is_available(&self) -> bool {
        self.available.get() && !self.hidden.get()
    }

    const fn is_shown(&self) -> bool {
        self.open.get() && self.is_available()
    }

    fn toggle(&self) {
        if self.is_available() {
            self.open.set(!self.open.get());
        }
    }
}

/// The sidebar and inspector of a window.
pub(super) struct Panels {
    window: WindowId,
    pages: PanelState,
    info: PanelState,
    // The selection marks of the pages in the sidebar.
    selected: RefCell<Vec<ComPtr>>,
}

impl Panels {
    /// Panels for a window, the sidebar open and the inspector closed.
    pub(super) const fn new(window: WindowId) -> Self {
        Self {
            window,
            pages: PanelState::new(true, false),
            info: PanelState::new(false, true),
            selected: RefCell::new(Vec::new()),
        }
    }

    /// The width the shown panels take beside the image in logical pixels.
    pub(super) const fn width(&self) -> f64 {
        let pages = if self.pages.is_shown() {
            SIDEBAR_WIDTH
        } else {
            0.0
        };
        let info = if self.info.is_shown() {
            INSPECTOR_WIDTH
        } else {
            0.0
        };
        pages + info
    }

    /// Shows the panels and their buttons as they are.
    pub(super) fn show(&self, island: &Island) {
        island.set_panel_available(Panel::Pages, self.pages.is_available());
        island.set_panel_open(Panel::Pages, self.pages.is_shown());
        island.set_panel_open(Panel::Info, self.info.is_shown());
    }

    /// Opens or closes a panel.
    pub(super) fn toggle(&self, panel: Panel) {
        match panel {
            Panel::Pages => self.pages.toggle(),
            Panel::Info => self.info.toggle(),
        }
    }

    /// Hides the panels while editing, or shows them again.
    pub(super) fn set_hidden(&self, hidden: bool) {
        self.pages.hidden.set(hidden);
        self.info.hidden.set(hidden);
    }

    /// Lists the pages with one selected, the sidebar shows when there are several.
    pub(super) fn set_pages(&self, island: &Island, pages: &[Media], page: usize) -> Result<()> {
        self.pages.available.set(pages.len() > 1);
        let content = load_xaml(&pages_xaml(pages))?.cast(&IID_IFRAMEWORK_ELEMENT)?;
        let mut selected = Vec::with_capacity(pages.len());
        for (index, media) in pages.iter().enumerate() {
            if let Some((bitmap, size)) = thumbnail(media) {
                set_image(
                    &find_name(&content, &wide(&format!("Thumbnail{index}")), &IID_IIMAGE)?,
                    &bitmap,
                    size,
                );
            }
            let window = self.window;
            let clicked = event_handler(IID_ROUTED_EVENT_HANDLER, move |_, _| {
                super::send(AppEvent::ShowPage(window, index));
            });
            // Slot 14 of IButtonBase is add_Click.
            add_event(
                &find_name(&content, &wide(&format!("Page{index}")), &IID_IBUTTON_BASE)?,
                14,
                &clicked,
            )?;
            selected.push(find_name(
                &content,
                &wide(&format!("Selected{index}")),
                &IID_IUI_ELEMENT,
            )?);
        }
        *self.selected.borrow_mut() = selected;
        island.set_panel_content(Panel::Pages, &content);
        self.select(page);
        Ok(())
    }

    /// Marks a page as selected without showing it, which the window does.
    pub(super) fn select(&self, page: usize) {
        for (index, mark) in self.selected.borrow().iter().enumerate() {
            set_visible(mark, index == page);
        }
    }

    /// Lists sections in the inspector, each a title with names and values below it.
    pub(super) fn set_sections(&self, island: &Island, sections: &[InfoSection]) -> Result<()> {
        // The information of another image never stays when this one cannot be shown.
        let content = load_xaml(&info_xaml(sections))
            .or_else(|_| load_xaml(&format!("<StackPanel {XMLNS} />")))?;
        island.set_panel_content(Panel::Info, &content);
        Ok(())
    }
}

// A thumbnail of media fitted in a square, rendered at a higher density, with its size in
// logical pixels. Small images are enlarged, with their pixels drawn as squares.
fn thumbnail(media: &Media) -> Option<(Bitmap, (f64, f64))> {
    let (width, height) = media.size();
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let scale = THUMBNAIL_SIZE / width.max(height);
    let pixels = render_media(media.clone(), scale * THUMBNAIL_DENSITY)?;
    let bitmap = Bitmap::new(pixels.width(), pixels.height())?;
    bitmap.write_rgba(pixels.data());
    Some((bitmap, (width * scale, height * scale)))
}

// The XAML of the sidebar: a button for each page with its thumbnail named `Thumbnail` and its
// index, its number and size, and a selection mark named `Selected` and its index, like the
// selected item of a navigation pane.
fn pages_xaml(pages: &[Media]) -> String {
    let mut items = String::new();
    for (index, media) in pages.iter().enumerate() {
        let (width, height) = media.size();
        _ = write!(
            items,
            r#"<Button x:Name="Page{index}" HorizontalAlignment="Stretch" HorizontalContentAlignment="Stretch" Padding="0" Background="Transparent" BorderThickness="0" AllowFocusOnInteraction="False">
    <Grid Padding="8">
        <Grid x:Name="Selected{index}" Margin="-8" Visibility="Collapsed">
            <Border Background="{{ThemeResource SubtleFillColorSecondaryBrush}}" CornerRadius="4" />
            <Rectangle Width="3" Height="16" RadiusX="1.5" RadiusY="1.5" HorizontalAlignment="Left" Fill="{{ThemeResource AccentFillColorDefaultBrush}}" />
        </Grid>
        <StackPanel Spacing="4">
            <Grid Width="{THUMBNAIL_SIZE}" Height="{THUMBNAIL_SIZE}" HorizontalAlignment="Center">
                <Image x:Name="Thumbnail{index}" Stretch="Fill" HorizontalAlignment="Center" VerticalAlignment="Center" />
            </Grid>
            <TextBlock Text="{number}" HorizontalAlignment="Center" />
            <TextBlock Text="{width} &#xD7; {height}" HorizontalAlignment="Center" Style="{{StaticResource CaptionTextBlockStyle}}" Foreground="{{ThemeResource TextFillColorSecondaryBrush}}" />
        </StackPanel>
    </Grid>
</Button>"#,
            number = index + 1,
            width = width.round() as i64,
            height = height.round() as i64,
        );
    }
    format!(r#"<StackPanel {XMLNS} Padding="6" Spacing="2">{items}</StackPanel>"#)
}

// The XAML of the inspector: for each section its title, then its names with selectable values
// below them.
fn info_xaml(sections: &[InfoSection]) -> String {
    let mut rows = String::new();
    for section in sections {
        _ = write!(
            rows,
            r#"<TextBlock Text="{}" Margin="0,16,0,8" Style="{{StaticResource BodyStrongTextBlockStyle}}" />"#,
            escape_xml(section.title)
        );
        for (name, value) in &section.rows {
            _ = write!(
                rows,
                r#"<TextBlock Text="{}" Style="{{StaticResource CaptionTextBlockStyle}}" Foreground="{{ThemeResource TextFillColorSecondaryBrush}}" /><TextBlock Text="{}" Margin="0,0,0,8" TextWrapping="Wrap" IsTextSelectionEnabled="True" />"#,
                escape_xml(name),
                escape_xml(value)
            );
        }
    }
    format!(r#"<StackPanel {XMLNS} Padding="16,0,16,16">{rows}</StackPanel>"#)
}

// MARK: Tests

#[cfg(test)]
mod tests {
    use imageview_core::Info;

    use super::*;

    #[test]
    fn panels_are_xaml_with_their_pages_and_information() {
        let page = Media::Raster(imageview_core::Raster {
            width: 16,
            height: 8,
            frames: Vec::new(),
            loop_count: imageview_core::LoopCount::Infinite,
        });
        let pages = pages_xaml(&[page.clone(), page.clone()]);
        for name in ["Page1", "Thumbnail1", "Selected1"] {
            assert!(pages.contains(&format!(r#"x:Name="{name}""#)), "{name}");
        }
        assert!(pages.contains("16 &#xD7; 8"));
        let sections = Info::default().sections(std::path::Path::new("a <b>.png"), &page, 0, 2);
        let info = info_xaml(&sections);
        assert!(info.contains("a &lt;b&gt;.png"));
        assert!(info.contains(r#"Text="1 of 2""#));
    }
}
