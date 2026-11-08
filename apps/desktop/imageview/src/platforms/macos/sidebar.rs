/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The sidebar of an image window, like the one of Preview: a split view controller with the
//! pages of a file with several, like the sizes of an icon, next to the media view. Picking a
//! page shows it, whether the sidebar is collapsed is remembered for new windows.

use std::cell::{Cell, RefCell};
use std::ptr::null_mut;

use bwindow::WindowId;
use imageview_core::Media;
use imageview_core::appkit::{NS_VIEW_WIDTH_SIZABLE, Rect, create_media_view, ns_string};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::headers::*;
use super::view::rect;
use crate::AppEvent;

const NS_TABLE_VIEW_STYLE_SOURCE_LIST: i64 = 3;
const NS_TABLE_VIEW_UNIFORM_COLUMN_AUTORESIZING_STYLE: u64 = 1;
const NS_TEXT_ALIGNMENT_CENTER: i64 = 1;
const NS_VISUAL_EFFECT_BLENDING_MODE_WITHIN_WINDOW: i64 = 1;
const NS_VISUAL_EFFECT_MATERIAL_TITLEBAR: i64 = 3;
// Flexible left and right margins, which keep a view in the middle of a row.
const NS_VIEW_CENTERED_HORIZONTALLY: u64 = 1 | 4;

// The width of the sidebar and the size of its rows.
const MINIMUM_WIDTH: f64 = 140.0;
const WIDTH: f64 = 180.0;
const MAXIMUM_WIDTH: f64 = 320.0;
const THUMBNAIL_SIZE: f64 = 112.0;
const PADDING: f64 = 8.0;
const LABEL_HEIGHT: f64 = 16.0;
const ROW_HEIGHT: f64 = PADDING + THUMBNAIL_SIZE + 4.0 + 2.0 * LABEL_HEIGHT + PADDING;

// The user default that remembers whether the sidebar is collapsed.
const COLLAPSED_KEY: &str = "SidebarCollapsed";

pub(super) struct DelegateIvars {
    window: WindowId,
    pages: RefCell<Vec<Media>>,
    item: Retained<Object>,
    // Whether the sidebar is in the split view, files with one page have none.
    shown: Cell<bool>,
    // Whether the sidebar is hidden while the page is edited.
    hidden: Cell<bool>,
    // Whether the selection is changed by the window instead of the user.
    selecting: Cell<bool>,
}

define_class!(
    // The data source and delegate of the page table, which also follows the split view.
    #[unsafe(super(NSObject))]
    #[name = "ImageViewSidebarDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    impl Delegate {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn _number_of_rows(&self, _: *mut Object) -> isize {
            self.ivars().pages.borrow().len() as isize
        }

        #[unsafe(method(tableView:viewForTableColumn:row:))]
        fn _view_for_row(
            &self,
            _: *mut Object,
            column: *mut Object,
            row: isize,
        ) -> Option<Retained<Object>> {
            let index = usize::try_from(row).ok()?;
            // SAFETY: The table passes its column.
            let width: f64 = unsafe { msg_send![column, width] };
            self.ivars().pages.borrow().get(index).map(|media| page_view(index, media, width))
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn _selection_did_change(&self, notification: *mut Object) {
            if self.ivars().selecting.get() {
                return;
            }
            // SAFETY: The notification object is the page table.
            let row: isize = unsafe {
                let table: *mut Object = msg_send![notification, object];
                msg_send![table, selectedRow]
            };
            if let Ok(page) = usize::try_from(row) {
                super::send(AppEvent::ShowPage(self.ivars().window, page));
            }
        }

        // The sidebar was collapsed or expanded, or resized.
        #[unsafe(method(splitViewDidResize:))]
        fn _split_view_did_resize(&self, _: *mut Object) {
            if !self.ivars().shown.get() {
                return;
            }
            // SAFETY: The item is a live split view item and the defaults live forever.
            unsafe {
                let collapsed: Bool = msg_send![&*self.ivars().item, isCollapsed];
                let defaults: *mut Object = msg_send![class!(NSUserDefaults), standardUserDefaults];
                let _: () = msg_send![defaults, setBool: collapsed, forKey: ns_string(COLLAPSED_KEY)];
            }
        }
    }
);

/// The split view controller with the sidebar and the media view.
pub(super) struct Sidebar {
    controller: Retained<Object>,
    // The scroll view of the table, below the toolbar.
    list: Retained<Object>,
    table: Retained<Object>,
    delegate: Retained<Delegate>,
}

impl Sidebar {
    /// Puts the media view next to a sidebar, `place_below_toolbar` places it below the
    /// toolbar once it is in the window.
    pub(super) fn new(frame: Rect, content: *mut Object, window: WindowId) -> Self {
        // SAFETY: All objects are valid AppKit instances. The split view controller retains
        // its items, which retain their view controllers and views. The delegate is not
        // retained by the table and the notification center, it is removed from them on drop.
        unsafe {
            let table: Retained<Object> = msg_send![class!(NSTableView), new];
            let column: Allocated<Object> = msg_send![class!(NSTableColumn), alloc];
            let column: Retained<Object> =
                msg_send![column, initWithIdentifier: ns_string!("page")];
            let _: () = msg_send![&*table, addTableColumn: &*column];
            let _: () = msg_send![&*table, setHeaderView: null_mut::<Object>()];
            let _: () = msg_send![&*table, setStyle: NS_TABLE_VIEW_STYLE_SOURCE_LIST];
            let _: () = msg_send![&*table, setRowHeight: ROW_HEIGHT];
            // The rows are as wide as the sidebar, which centers their contents.
            let _: () = msg_send![&*table,
                setColumnAutoresizingStyle: NS_TABLE_VIEW_UNIFORM_COLUMN_AUTORESIZING_STYLE
            ];
            let clear: *mut Object = msg_send![class!(NSColor), clearColor];
            let _: () = msg_send![&*table, setBackgroundColor: clear];
            let scroll_view: Allocated<Object> = msg_send![class!(NSScrollView), alloc];
            let scroll_view: Retained<Object> = msg_send![scroll_view,
                initWithFrame: rect(0.0, 0.0, MINIMUM_WIDTH, frame.size.height)
            ];
            let _: () = msg_send![&*scroll_view, setDrawsBackground: Bool::NO];
            let _: () = msg_send![&*scroll_view, setHasVerticalScroller: Bool::YES];
            let _: () = msg_send![&*scroll_view, setAutohidesScrollers: Bool::YES];
            let _: () = msg_send![&*scroll_view, setDocumentView: &*table];
            // The sidebar reaches under the toolbar, its list starts below it.
            let item: Retained<Object> = msg_send![class!(NSSplitViewItem),
                sidebarWithViewController: &*view_controller(&container(frame, &scroll_view))
            ];
            let _: () = msg_send![&*item, setMinimumThickness: MINIMUM_WIDTH];
            let _: () = msg_send![&*item, setMaximumThickness: MAXIMUM_WIDTH];
            keep_on_window_resize(&item);

            let content_container = container(frame, &*content);
            add_toolbar_background(&content_container, content);
            let content_item: Retained<Object> = msg_send![class!(NSSplitViewItem),
                splitViewItemWithViewController: &*view_controller(&content_container)
            ];
            let controller: Retained<Object> = msg_send![class!(NSSplitViewController), new];
            let _: () = msg_send![&*controller, addSplitViewItem: &*content_item];
            let view: *mut Object = msg_send![&*controller, view];
            let _: () = msg_send![view, setFrame: frame];

            let delegate: Allocated<Delegate> = msg_send![Delegate::class(), alloc];
            let delegate: Retained<Delegate> = msg_send![
                super(delegate.set_ivars(DelegateIvars {
                    window,
                    pages: RefCell::new(Vec::new()),
                    item,
                    shown: Cell::new(false),
                    hidden: Cell::new(false),
                    selecting: Cell::new(false),
                })),
                init
            ];
            let _: () = msg_send![&*table, setDataSource: &*delegate];
            let _: () = msg_send![&*table, setDelegate: &*delegate];
            let split_view: *mut Object = msg_send![&*controller, splitView];
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![center,
                addObserver: &*delegate,
                selector: sel!(splitViewDidResize:),
                name: NSSplitViewDidResizeSubviewsNotification,
                object: split_view
            ];
            Self {
                controller,
                list: scroll_view,
                table,
                delegate,
            }
        }
    }

    /// The view of the split view controller, which fills the window.
    pub(super) fn view(&self) -> *mut Object {
        // SAFETY: The controller has its view.
        unsafe { msg_send![&*self.controller, view] }
    }

    /// Places a view of a container below the toolbar of its window, which the content
    /// reaches under. The constraint goes away when the view leaves the window.
    /// Returns the constraint, or `None` while the view is not in a window.
    pub(super) fn place_below_toolbar(view: *mut Object) -> Option<Retained<Object>> {
        // SAFETY: The view is in the content view of its window, which owns the guide.
        unsafe {
            let window: *mut Object = msg_send![view, window];
            if window.is_null() {
                return None;
            }
            let guide: *mut Object = msg_send![window, contentLayoutGuide];
            let anchor: *mut Object = msg_send![view, topAnchor];
            let to: *mut Object = msg_send![guide, topAnchor];
            let constraint: Retained<Object> = msg_send![anchor, constraintEqualToAnchor: to];
            let _: () = msg_send![&*constraint, setActive: Bool::YES];
            Some(constraint)
        }
    }

    /// Collapses or expands the sidebar with an animation, when it is shown.
    pub(super) fn toggle(&self) {
        if self.is_shown() {
            // SAFETY: The controller has the sidebar item, which toggling collapses or expands.
            unsafe {
                let _: () = msg_send![&*self.controller, toggleSidebar: null_mut::<Object>()];
            }
        }
    }

    /// Whether the sidebar is shown, with several pages and not hidden.
    pub(super) fn is_shown(&self) -> bool {
        self.delegate.ivars().shown.get()
    }

    /// The split view controller, which also has the inspector.
    pub(super) const fn controller(&self) -> *mut Object {
        Retained::as_ptr(&self.controller)
    }

    /// The split view, which the toolbar separator of the sidebar follows.
    pub(super) fn split_view(&self) -> *mut Object {
        // SAFETY: The controller has its split view.
        unsafe { msg_send![&*self.controller, splitView] }
    }

    /// Lists the pages with one selected, in a sidebar when there are several. Returns
    /// whether the sidebar was added or removed.
    pub(super) fn set_pages(&self, pages: &[Media], page: usize) -> bool {
        *self.delegate.ivars().pages.borrow_mut() = pages.to_vec();
        let changed = self.update(true);
        // SAFETY: The table is live. Its only column fills it, so the rows are as wide as the
        // sidebar.
        unsafe {
            let _: () = msg_send![&*self.table, sizeLastColumnToFit];
            let _: () = msg_send![&*self.table, reloadData];
        }
        self.select(page);
        changed
    }

    /// Hides the sidebar, or shows it again when there are several pages. Returns whether
    /// the sidebar was added or removed. The window keeps its size, so the media gets the room.
    pub(super) fn set_hidden(&self, hidden: bool) -> bool {
        self.delegate.ivars().hidden.set(hidden);
        self.update(false)
    }

    // Adds or removes the sidebar, returns whether it was. When resizing, the window grows by
    // an expanded sidebar or shrinks by it, so the media and toolbar keep their room.
    fn update(&self, resize_window: bool) -> bool {
        let ivars = self.delegate.ivars();
        let show = ivars.pages.borrow().len() > 1 && !ivars.hidden.get();
        let changed = ivars.shown.replace(show) != show;
        // SAFETY: The controller retains its items, adding one lays out the split view.
        unsafe {
            if changed && show {
                let defaults: *mut Object = msg_send![class!(NSUserDefaults), standardUserDefaults];
                let collapsed: Bool = msg_send![defaults, boolForKey: ns_string(COLLAPSED_KEY)];
                if resize_window && !collapsed.as_bool() {
                    self.grow_window(WIDTH);
                }
                let _: () = msg_send![&*ivars.item, setCollapsed: collapsed];
                let _: () = msg_send![&*self.controller, insertSplitViewItem: &*ivars.item, atIndex: 0isize];
                Self::place_below_toolbar(Retained::as_ptr(&self.list));
                if !collapsed.as_bool() {
                    let _: () =
                        msg_send![self.split_view(), setPosition: WIDTH, ofDividerAtIndex: 0isize];
                }
            } else if changed {
                let collapsed: Bool = msg_send![&*ivars.item, isCollapsed];
                let list: *mut Object = msg_send![&*self.list, superview];
                let frame: Rect = msg_send![list, frame];
                let _: () = msg_send![&*self.controller, removeSplitViewItem: &*ivars.item];
                if resize_window && !collapsed.as_bool() {
                    self.grow_window(-frame.size.width);
                }
            }
            if changed {
                let _: () = msg_send![self.view(), layoutSubtreeIfNeeded];
            }
        }
        changed
    }

    // Makes the window wider by a width, keeping its left edge in place.
    fn grow_window(&self, width: f64) {
        // SAFETY: The split view is in its window.
        unsafe {
            let window: *mut Object = msg_send![self.view(), window];
            let mut frame: Rect = msg_send![window, frame];
            frame.size.width += width;
            let _: () = msg_send![window, setFrame: frame, display: Bool::YES];
        }
    }

    /// Selects a page without showing it, which the window does.
    pub(super) fn select(&self, page: usize) {
        let ivars = self.delegate.ivars();
        if page >= ivars.pages.borrow().len() {
            return;
        }
        ivars.selecting.set(true);
        // SAFETY: The table has a row for every page.
        unsafe {
            let rows: *mut Object = msg_send![class!(NSIndexSet), indexSetWithIndex: page];
            let _: () =
                msg_send![&*self.table, selectRowIndexes: rows, byExtendingSelection: Bool::NO];
            let _: () = msg_send![&*self.table, scrollRowToVisible: page as isize];
        }
        ivars.selecting.set(false);
    }
}

impl Drop for Sidebar {
    fn drop(&mut self) {
        // SAFETY: The table and notification center forget the delegate before it is released.
        unsafe {
            let _: () = msg_send![&*self.table, setDataSource: null_mut::<Object>()];
            let _: () = msg_send![&*self.table, setDelegate: null_mut::<Object>()];
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![center, removeObserver: &*self.delegate];
        }
    }
}

/// A view that fills its container, but for the top which is placed below the toolbar.
pub(super) fn container(frame: Rect, view: &Object) -> Retained<Object> {
    // SAFETY: The container retains the view, the constraints are between them.
    unsafe {
        let container: Allocated<Object> = msg_send![class!(NSView), alloc];
        let container: Retained<Object> = msg_send![container, initWithFrame: frame];
        let _: () = msg_send![&*container, addSubview: view];
        let _: () = msg_send![view, setTranslatesAutoresizingMaskIntoConstraints: Bool::NO];
        let anchors: [(*mut Object, *mut Object); 3] = [
            (
                msg_send![view, bottomAnchor],
                msg_send![&*container, bottomAnchor],
            ),
            (
                msg_send![view, leadingAnchor],
                msg_send![&*container, leadingAnchor],
            ),
            (
                msg_send![view, trailingAnchor],
                msg_send![&*container, trailingAnchor],
            ),
        ];
        for (anchor, to) in anchors {
            let constraint: *mut Object = msg_send![anchor, constraintEqualToAnchor: to];
            let _: () = msg_send![constraint, setActive: Bool::YES];
        }
        container
    }
}

// Fills the part of a container above a view, which is below the toolbar, with the material of
// the titlebar, as the toolbar has no background of its own over the content.
fn add_toolbar_background(container: &Object, below: *mut Object) {
    // SAFETY: The container retains the background view, the constraints are between views of
    // the container.
    unsafe {
        let background: Retained<Object> = msg_send![class!(NSVisualEffectView), new];
        let _: () = msg_send![&*background, setMaterial: NS_VISUAL_EFFECT_MATERIAL_TITLEBAR];
        let _: () = msg_send![&*background,
            setBlendingMode: NS_VISUAL_EFFECT_BLENDING_MODE_WITHIN_WINDOW
        ];
        let _: () = msg_send![&*background, setTranslatesAutoresizingMaskIntoConstraints: Bool::NO];
        let _: () = msg_send![container, addSubview: &*background];
        let anchors: [(*mut Object, *mut Object); 4] = [
            (
                msg_send![&*background, topAnchor],
                msg_send![container, topAnchor],
            ),
            (
                msg_send![&*background, bottomAnchor],
                msg_send![below, topAnchor],
            ),
            (
                msg_send![&*background, leadingAnchor],
                msg_send![container, leadingAnchor],
            ),
            (
                msg_send![&*background, trailingAnchor],
                msg_send![container, trailingAnchor],
            ),
        ];
        for (anchor, to) in anchors {
            let constraint: *mut Object = msg_send![anchor, constraintEqualToAnchor: to];
            let _: () = msg_send![constraint, setActive: Bool::YES];
        }
    }
}

/// Keeps a split view item from collapsing when the window gets narrow, so it only collapses
/// when the user does, which is remembered.
pub(super) fn keep_on_window_resize(item: &Object) {
    // SAFETY: The item is a live split view item, the property is available since macOS 14.
    unsafe {
        let available: Bool =
            msg_send![item, respondsToSelector: sel!(setCanCollapseFromWindowResize:)];
        if available.as_bool() {
            let _: () = msg_send![item, setCanCollapseFromWindowResize: Bool::NO];
        }
    }
}

/// A view controller of a view.
pub(super) fn view_controller(view: &Object) -> Retained<Object> {
    // SAFETY: The view controller retains its view, which keeps it from loading a nib.
    unsafe {
        let controller: Retained<Object> = msg_send![class!(NSViewController), new];
        let _: () = msg_send![&*controller, setView: view];
        controller
    }
}

// The row of a page as wide as the sidebar: its media fitted in a square, with its number
// and size below it.
fn page_view(index: usize, media: &Media, width: f64) -> Retained<Object> {
    let (media_width, media_height) = media.size();
    let labels = [
        (format!("{}", index + 1), false),
        (
            format!(
                "{} \u{d7} {}",
                media_width.round() as i64,
                media_height.round() as i64
            ),
            true,
        ),
    ];
    // SAFETY: All views are valid AppKit instances, the row retains its subviews.
    unsafe {
        let row: Allocated<Object> = msg_send![class!(NSView), alloc];
        let row: Retained<Object> =
            msg_send![row, initWithFrame: rect(0.0, 0.0, width, ROW_HEIGHT)];
        // Rows are not flipped, so the thumbnail is placed from the top down.
        let mut y = ROW_HEIGHT - PADDING - THUMBNAIL_SIZE;
        let thumbnail = create_media_view(
            rect(
                (width - THUMBNAIL_SIZE) / 2.0,
                y,
                THUMBNAIL_SIZE,
                THUMBNAIL_SIZE,
            ),
            media.clone(),
        );
        let _: () = msg_send![&*thumbnail, setAutoresizingMask: NS_VIEW_CENTERED_HORIZONTALLY];
        let _: () = msg_send![&*row, addSubview: &*thumbnail];
        y -= 4.0;
        for (text, secondary) in labels {
            y -= LABEL_HEIGHT;
            let label: *mut Object =
                msg_send![class!(NSTextField), labelWithString: ns_string(&text)];
            let _: () = msg_send![label, setFrame: rect(0.0, y, width, LABEL_HEIGHT)];
            let _: () = msg_send![label, setAlignment: NS_TEXT_ALIGNMENT_CENTER];
            let _: () = msg_send![label, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE];
            if secondary {
                let color: *mut Object = msg_send![class!(NSColor), secondaryLabelColor];
                let _: () = msg_send![label, setTextColor: color];
                let size: f64 = msg_send![class!(NSFont), smallSystemFontSize];
                let font: *mut Object = msg_send![class!(NSFont), systemFontOfSize: size];
                let _: () = msg_send![label, setFont: font];
            }
            let _: () = msg_send![&*row, addSubview: label];
        }
        row
    }
}
