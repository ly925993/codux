use crate::heroicons::HeroIconName;
use anyhow::Result;
#[cfg(target_os = "macos")]
use cocoa::appkit::NSEventModifierFlags;
use codux_runtime::project_store::ProjectRuntimeTarget;
use codux_runtime::runtime_terminal::RuntimeTerminalController;
use codux_runtime::terminal_pty::{
    EventSink, TerminalEvent, TerminalInputSnapshot, TerminalManager, TerminalOutputSnapshot,
    TerminalPtyConfig, TerminalPtySession, terminal_viewport_local_owner,
};
use codux_terminal_core::{
    HeadlessTerminalScreen, HeadlessTerminalSnapshotRequest, TerminalInputMode,
    TerminalScreenCellSnapshot, TerminalScreenColor, TerminalScreenCursorShape,
    TerminalScreenCursorSnapshot, TerminalScreenImage, TerminalScreenSnapshot,
    TerminalScreenUnderline, TerminalSelectionSpanKind,
};
use gpui::{
    App, AppContext, Bounds, ClipboardEntry, ClipboardItem, ContentMask, Context, Corners,
    CursorStyle, Edges, Element, ElementId, Entity, ExternalPaths, FocusHandle, Focusable, Font,
    FontFeatures, FontStyle, FontWeight, GlobalElementId, Hsla, ImageFormat, InputHandler,
    InspectorElementId, InteractiveElement, IntoElement, KeyDownEvent, Keystroke, LayoutId,
    Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    NavigationDirection, ParentElement, Pixels, Point, Render, RenderImage, ScrollWheelEvent,
    SharedString, Size, StatefulInteractiveElement, Style, Styled, Subscription, Task, TextAlign,
    TextRun, TouchPhase, UTF16Selection, UnderlineStyle, WeakEntity, Window, div, px, quad, rgb,
    transparent_black,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_component::scroll::{Scrollbar, ScrollbarAxis, ScrollbarHandle, ScrollbarShow};
use gpui_component::{ActiveTheme, Icon, Sizable, Size as ComponentSize, WindowExt};
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};
use parking_lot::Mutex;
use regex::Regex;
use std::{
    cell::{Cell as StdCell, RefCell},
    collections::{HashMap, HashSet, VecDeque, hash_map::DefaultHasher},
    env, fs,
    hash::{Hash, Hasher},
    io::Write,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, LazyLock, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub use codux_runtime::terminal_pty::TerminalLaunchContext;

// Every clipboard producer shares one sequence so a slower selection task
// cannot overwrite a newer manual copy or OSC 52 clipboard write.
static TERMINAL_CLIPBOARD_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
fn terminal_native_control_modifier_pressed() -> bool {
    unsafe {
        // GPUI normalizes Control+left-click to a right-click with `control=false`; AppKit still
        // exposes the physical key state, which lets terminal links distinguish that gesture.
        let modifiers: NSEventModifierFlags = msg_send![class!(NSEvent), modifierFlags];
        modifiers.contains(NSEventModifierFlags::NSControlKeyMask)
    }
}

#[cfg(not(target_os = "macos"))]
fn terminal_native_control_modifier_pressed() -> bool {
    false
}

include!("terminal/pane.rs");
include!("terminal/config.rs");
include!("terminal/agent_draft.rs");
include!("terminal/view.rs");
include!("terminal/protocol.rs");
include!("terminal/render.rs");
include!("terminal/model.rs");
include!("terminal/content.rs");
include!("terminal/grid_version.rs");
include!("terminal/builtin_glyphs.rs");
include!("terminal/element.rs");
include!("terminal/input.rs");
include!("terminal/events.rs");
include!("terminal/keys.rs");
#[cfg(target_os = "windows")]
include!("terminal/clipboard_windows.rs");
include!("terminal/mouse.rs");
include!("terminal/renderer.rs");
include!("terminal/palette.rs");
#[cfg(test)]
mod tests;
