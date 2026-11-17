// SPDX-License-Identifier: Apache-2.0
//! The macOS toast and recap card: borderless, non-activating floating panels that ignore the
//! mouse, so they can never take keyboard focus (design section 8).
//!
//! Status: seen on a Mac in real runs.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEventMask, NSFont,
    NSPanel, NSScreen, NSStatusWindowLevel, NSTextField, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString};

const TOAST_SIZE: (f64, f64) = (460.0, 64.0);
const RECAP_SIZE: (f64, f64) = (520.0, 170.0);
const PAD: f64 = 14.0;
/// Distance from the top of the screen.
const TOP_MARGIN: f64 = 48.0;

struct Panel {
    window: Retained<NSPanel>,
    label: Retained<NSTextField>,
    /// The width is fixed; the height grows with the text.
    size: Cell<(f64, f64)>,
    font_size: f64,
}

impl Panel {
    fn new(mtm: MainThreadMarker, size: (f64, f64), font_size: f64) -> Self {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size.0, size.1));
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc::<NSPanel>(),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        window.setFloatingPanel(true);
        window.setLevel(NSStatusWindowLevel);
        window.setOpaque(false);
        window.setHasShadow(true);
        window.setIgnoresMouseEvents(true);
        window.setHidesOnDeactivate(false);
        window.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
            0.09, 0.09, 0.11, 0.94,
        )));
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
        // SAFETY: the window is kept alive by `Panel`; AppKit must not also release it on close.
        unsafe { window.setReleasedWhenClosed(false) };

        let label = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
        label.setTextColor(Some(&NSColor::whiteColor()));
        label.setFont(Some(&NSFont::systemFontOfSize(font_size)));
        label.setFrame(NSRect::new(
            NSPoint::new(PAD, PAD),
            NSSize::new(size.0 - 2.0 * PAD, size.1 - 2.0 * PAD),
        ));
        if let Some(content) = window.contentView() {
            content.addSubview(&label);
        }
        Self {
            window,
            label,
            size: Cell::new(size),
            font_size,
        }
    }

    /// Tall enough for `text` once wrapped, never shorter than the panel's starting height.
    fn height_for(&self, text: &str, min_height: f64) -> f64 {
        let (width, _) = self.size.get();
        // About 0.55 of the font size per character is a fair average for the system font.
        let per_line = ((width - 2.0 * PAD) / (self.font_size * 0.55))
            .floor()
            .max(10.0);
        let mut lines = 0.0_f64;
        for line in text.lines() {
            let chars = f64::from(u32::try_from(line.chars().count().max(1)).unwrap_or(u32::MAX));
            lines += (chars / per_line).ceil();
        }
        (lines * self.font_size * 1.45 + 2.0 * PAD).max(min_height)
    }

    fn show(&self, mtm: MainThreadMarker, text: &str, min_height: f64) {
        self.label.setStringValue(&NSString::from_str(text));
        let height = self.height_for(text, min_height);
        let (width, _) = self.size.get();
        self.size.set((width, height));
        self.window.setContentSize(NSSize::new(width, height));
        self.label.setFrame(NSRect::new(
            NSPoint::new(PAD, PAD),
            NSSize::new(width - 2.0 * PAD, height - 2.0 * PAD),
        ));
        let screen = NSScreen::mainScreen(mtm);
        let frame = screen.map_or_else(
            || NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)),
            |s| s.frame(),
        );
        let x = frame.origin.x + (frame.size.width - width) / 2.0;
        let y = frame.origin.y + frame.size.height - height - TOP_MARGIN;
        self.window.setFrameOrigin(NSPoint::new(x, y));
        // `orderFrontRegardless` shows the panel without activating this process.
        self.window.orderFrontRegardless();
    }

    fn hide(&self) {
        self.window.orderOut(None);
    }
}

pub struct MacOverlay {
    mtm: Option<MainThreadMarker>,
    toast: RefCell<Option<Panel>>,
    recap: RefCell<Option<Panel>>,
    recap_hides_at: Cell<Option<Instant>>,
}

impl MacOverlay {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mtm = MainThreadMarker::new();
        if let Some(mtm) = mtm {
            let app = NSApplication::sharedApplication(mtm);
            // No Dock icon and never the active app.
            app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            // Windows may not draw until the app has finished launching.
            app.finishLaunching();
        } else {
            eprintln!("overlay disabled: must be created on the main thread");
        }
        Self {
            mtm,
            toast: RefCell::new(None),
            recap: RefCell::new(None),
            recap_hides_at: Cell::new(None),
        }
    }

    pub fn show_toast(&self, text: &str) {
        let Some(mtm) = self.mtm else { return };
        let mut slot = self.toast.borrow_mut();
        let panel = slot.get_or_insert_with(|| Panel::new(mtm, TOAST_SIZE, 15.0));
        panel.show(mtm, text, TOAST_SIZE.1);
    }

    pub fn hide_toast(&self) {
        if let Some(panel) = self.toast.borrow().as_ref() {
            panel.hide();
        }
    }

    pub fn show_recap(&self, lines: &[String], for_how_long: Duration) {
        let Some(mtm) = self.mtm else { return };
        let mut slot = self.recap.borrow_mut();
        let panel = slot.get_or_insert_with(|| Panel::new(mtm, RECAP_SIZE, 13.0));
        panel.show(mtm, &lines.join("\n"), RECAP_SIZE.1);
        self.recap_hides_at.set(Some(Instant::now() + for_how_long));
    }

    pub fn tick(&self, now: Instant) {
        if self.recap_hides_at.get().is_some_and(|at| now >= at) {
            self.recap_hides_at.set(None);
            if let Some(panel) = self.recap.borrow().as_ref() {
                panel.hide();
            }
        }
    }

    /// Hands pending events to `AppKit`. Carbon hotkey presses are delivered while the
    /// application's event queue is serviced, and nothing else here services it.
    pub fn dispatch_events(&self) {
        let Some(mtm) = self.mtm else { return };
        let app = NSApplication::sharedApplication(mtm);
        let until = NSDate::dateWithTimeIntervalSinceNow(0.005);
        // SAFETY: the default run loop mode constant is valid and `until` is a live date.
        while let Some(event) = unsafe {
            app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&until),
                NSDefaultRunLoopMode,
                true,
            )
        } {
            app.sendEvent(&event);
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.recap_hides_at.get()
    }
}
