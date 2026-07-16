//! Input handling: tool modes, keyboard shortcuts, mouse interactions, and
//! the view controller that maps input events to viewport mutations. [DS-NAV, DS-SCROLL, DS-ZOOM]
//!
//! ## Architecture
//!
//! The shell (C++/Qt) translates OS input events into `InputEvent` values
//! and passes them to the Rust view controller. The controller interprets
//! events in the context of the current tool mode and shortcut configuration,
//! then mutates the `ViewportState` accordingly.
//!
//! This separation keeps input policy in Rust (testable, consistent) while
//! the shell handles only platform event translation.
#![allow(missing_docs)] // M0: exhaustive field docs deferred to component-level review

use crate::layout::ViewportState;
use crate::scroll::ScrollPhysics;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Tool modes
// ---------------------------------------------------------------------------

/// The active interaction tool. [DS-NAV-1 through DS-NAV-4]
///
/// Each tool defines how mouse/pen input is interpreted. The Select tool
/// is the default resting state; every specialized tool has a clear return
/// path to Select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolMode {
    /// Default: click to select objects, drag to move, drag handles to resize.
    Select,
    /// Drag to pan/scroll the viewport. [DS-CURSOR-1]
    Hand,
    /// Drag to zoom to a rectangle (marquee zoom). [DS-ZOOM-5]
    ZoomToRect,
    /// Measure distance/area. [FR-MEAS]
    Measure,
    /// Draw ink annotation. [FR-ANNOT-7]
    Ink,
    /// Draw shape annotation.
    Shape,
    /// Place stamp annotation.
    Stamp,
    /// Add text annotation / sticky note.
    Note,
    /// Mark redaction regions. [FR-RED-5]
    Redact,
}

impl ToolMode {
    /// Whether this tool is "sticky" (stays active after one use) by default.
    ///
    /// Per DS-NAV-4, stickiness is user-configurable. These are the defaults
    /// that match Acrobat behavior.
    pub fn default_sticky(&self) -> bool {
        match self {
            Self::Select => true,
            Self::Hand => true,
            Self::ZoomToRect => false, // Marquee zoom: one-shot
            Self::Measure => false,
            Self::Ink => true,
            Self::Shape => false,
            Self::Stamp => false,
            Self::Note => false,
            Self::Redact => true,
        }
    }

    /// Whether this tool supports drag interaction.
    pub fn supports_drag(&self) -> bool {
        matches!(
            self,
            Self::Hand | Self::Select | Self::ZoomToRect | Self::Measure
                | Self::Ink | Self::Shape | Self::Redact
        )
    }
}

// ---------------------------------------------------------------------------
// Input actions
// ---------------------------------------------------------------------------

/// High-level user actions derived from raw input events.
///
/// The view controller translates `InputEvent` values into these actions
/// based on the current tool mode and shortcut configuration, then
/// executes them against the `ViewportState`.
#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    // -- Navigation --
    /// Scroll the viewport by (dx, dy) in document-space points.
    ScrollBy { dx: f32, dy: f32 },
    /// Scroll to center a specific page in the viewport.
    ScrollToPage(u32),
    /// Scroll by a number of pages (positive = forward).
    ScrollByPages(i32),
    /// Go to the first page.
    FirstPage,
    /// Go to the last page.
    LastPage,
    /// Navigate to the previous view (history back). [DS-HIST-1]
    PreviousView,
    /// Navigate to the next view (history forward). [DS-HIST-1]

    NextView,

    // -- Zoom --
    /// Zoom by a factor toward a focal point (focus_x, focus_y in device pixels).
    ZoomBy { factor: f32, focus_x: f32, focus_y: f32 },
    /// Zoom to fit page width.
    ZoomFitWidth,
    /// Zoom to fit page height.
    ZoomFitHeight,
    /// Zoom to fit the entire page.
    ZoomFitPage,
    /// Step to the next higher zoom level.
    ZoomIn,
    /// Step to the next lower zoom level.
    ZoomOut,
    /// Reset zoom to 100% (actual size).
    ZoomActualSize,
    /// Zoom to a specific scale factor.
    ZoomTo(f32),

    // -- Layout --
    /// Switch to single-page layout.
    LayoutSingle,
    /// Switch to continuous (vertical scroll) layout.
    LayoutContinuous,
    /// Switch to facing (side-by-side) layout.
    LayoutFacing,
    /// Switch to continuous-facing layout.
    LayoutContinuousFacing,

    // -- Tools --
    /// Switch to the select tool.
    ToolSelect,
    /// Switch to the hand/pan tool.
    ToolHand,

    // -- View controls --
    /// Rotate the view 90 degrees clockwise.
    RotateClockwise,
    /// Rotate the view 90 degrees counter-clockwise.
    RotateCounterClockwise,
    /// Reset rotation to 0.
    RotateReset,
    /// Toggle distraction-free mode (hide all panels). [DS-SHELL-4]
    TogglePanels,
    /// Enter presentation mode (full screen without chrome). [DS-SHELL-4]
    PresentationMode,
    /// Escape: cancel current action, close transient surface, deselect, or return to Select. [DS-NAV-2]
    Escape,

    // -- No-op --
    /// No action (e.g. key press not bound to anything).
    None,
}

// ---------------------------------------------------------------------------
// Input events (from the shell)
// ---------------------------------------------------------------------------

/// A keyboard key identifier. Matches Qt key codes conceptually.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Letter keys.
    Char(char),
    /// Function keys.
    F(u8),
    /// Arrow keys.
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    /// Navigation keys.
    Home,
    End,
    PageUp,
    PageDown,
    Space,
    Escape,
    Enter,
    Tab,
    Backspace,
    Delete,
    /// Modifier-like keys that also carry actions.
    Plus,   // '+' or '=' key for zoom-in
    Minus,  // '-' key for zoom-out
    Zero,   // '0' key for fit/actual-size
    Unknown(u32),
}

/// Modifier keys held during an input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool, // Cmd on macOS, Win on Windows
}

impl Modifiers {
    /// Ctrl (or Cmd on macOS — caller maps platform key).
    pub fn control(&self) -> bool {
        self.ctrl || self.meta
    }
}

/// Raw input events from the shell.
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// A key was pressed.
    KeyDown { key: Key, modifiers: Modifiers },
    /// A key was released.
    KeyUp { key: Key, modifiers: Modifiers },
    /// Mouse/pointer button pressed. `x`, `y` are in canvas device pixels.
    MousePress { x: f32, y: f32, button: MouseButton, modifiers: Modifiers },
    /// Mouse/pointer moved (with buttons held for drag, or just hover).
    MouseMove { x: f32, y: f32, buttons: MouseButtons },
    /// Mouse button released.
    MouseRelease { x: f32, y: f32, button: MouseButton },
    /// Mouse wheel scroll. `delta_y` is positive for scroll-up (away from user).
    /// `x`, `y` are the pointer position for pointer-anchored zoom.
    Wheel { delta_x: f32, delta_y: f32, x: f32, y: f32, modifiers: Modifiers },
    /// Two-finger pinch gesture (trackpad). `scale` is the pinch scale factor
    /// (1.0 = no change, >1.0 = zoom in). `center_x/y` is the gesture centroid.
    Pinch { scale: f32, center_x: f32, center_y: f32 },
}

/// Mouse buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// Bitmask of currently held mouse buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MouseButtons {
    pub left: bool,
    pub middle: bool,
    pub right: bool,
}

// ---------------------------------------------------------------------------
// Shortcut definitions
// ---------------------------------------------------------------------------

/// A keyboard shortcut: key + modifiers → action.
#[derive(Debug, Clone, PartialEq)]
pub struct Shortcut {
    pub key: Key,
    pub modifiers: Modifiers,
    pub action: InputAction,
}

/// Registry of keyboard shortcuts. Supports lookup and customization.
#[derive(Debug, Clone)]
pub struct ShortcutRegistry {
    shortcuts: Vec<Shortcut>,
}

impl ShortcutRegistry {
    /// Create the default shortcut set matching Acrobat conventions. [DS-NAV-2]
    pub fn defaults() -> Self {
        let mut shortcuts = Vec::new();

        let ctrl = Modifiers { ctrl: true, ..Default::default() };
        let shift = Modifiers { shift: true, ..Default::default() };
        let alt = Modifiers { alt: true, ..Default::default() };
        let none = Modifiers::default();

        // -- Navigation --
        shortcuts.push(Shortcut { key: Key::ArrowUp, modifiers: none, action: InputAction::ScrollBy { dx: 0.0, dy: -40.0 } });
        shortcuts.push(Shortcut { key: Key::ArrowDown, modifiers: none, action: InputAction::ScrollBy { dx: 0.0, dy: 40.0 } });
        shortcuts.push(Shortcut { key: Key::ArrowLeft, modifiers: none, action: InputAction::ScrollBy { dx: -40.0, dy: 0.0 } });
        shortcuts.push(Shortcut { key: Key::ArrowRight, modifiers: none, action: InputAction::ScrollBy { dx: 40.0, dy: 0.0 } });
        shortcuts.push(Shortcut { key: Key::Home, modifiers: none, action: InputAction::FirstPage });
        shortcuts.push(Shortcut { key: Key::End, modifiers: none, action: InputAction::LastPage });
        shortcuts.push(Shortcut { key: Key::PageUp, modifiers: none, action: InputAction::ScrollByPages(-1) });
        shortcuts.push(Shortcut { key: Key::PageDown, modifiers: none, action: InputAction::ScrollByPages(1) });
        shortcuts.push(Shortcut { key: Key::Space, modifiers: none, action: InputAction::ScrollByPages(1) });
        shortcuts.push(Shortcut { key: Key::Space, modifiers: shift, action: InputAction::ScrollByPages(-1) });
        shortcuts.push(Shortcut { key: Key::ArrowLeft, modifiers: alt, action: InputAction::PreviousView });
        shortcuts.push(Shortcut { key: Key::ArrowRight, modifiers: alt, action: InputAction::NextView });

        // -- Zoom (Ctrl/Cmd) --
        shortcuts.push(Shortcut { key: Key::Plus, modifiers: ctrl, action: InputAction::ZoomIn });
        shortcuts.push(Shortcut { key: Key::Minus, modifiers: ctrl, action: InputAction::ZoomOut });
        shortcuts.push(Shortcut { key: Key::Zero, modifiers: ctrl, action: InputAction::ZoomFitPage });
        // Ctrl+1 = actual size (100%)
        shortcuts.push(Shortcut { key: Key::Char('1'), modifiers: ctrl, action: InputAction::ZoomActualSize });
        // Ctrl+2 = fit width
        shortcuts.push(Shortcut { key: Key::Char('2'), modifiers: ctrl, action: InputAction::ZoomFitWidth });

        // -- Layout --
        shortcuts.push(Shortcut { key: Key::Char('1'), modifiers: none, action: InputAction::LayoutSingle });
        shortcuts.push(Shortcut { key: Key::Char('2'), modifiers: none, action: InputAction::LayoutContinuous });
        shortcuts.push(Shortcut { key: Key::Char('3'), modifiers: none, action: InputAction::LayoutFacing });
        shortcuts.push(Shortcut { key: Key::Char('4'), modifiers: none, action: InputAction::LayoutContinuousFacing });

        // -- Tools --
        shortcuts.push(Shortcut { key: Key::Char('v'), modifiers: none, action: InputAction::ToolSelect });
        shortcuts.push(Shortcut { key: Key::Char('h'), modifiers: none, action: InputAction::ToolHand });
        // 'V' (shift+v) = select, 'H' (shift+h) = hand (Acrobat convention)
        shortcuts.push(Shortcut { key: Key::Char('V'), modifiers: none, action: InputAction::ToolSelect });
        shortcuts.push(Shortcut { key: Key::Char('H'), modifiers: none, action: InputAction::ToolHand });

        // -- View controls --
        shortcuts.push(Shortcut { key: Key::Char('r'), modifiers: none, action: InputAction::RotateClockwise });
        shortcuts.push(Shortcut { key: Key::Char('R'), modifiers: none, action: InputAction::RotateCounterClockwise });
        shortcuts.push(Shortcut { key: Key::F(8), modifiers: none, action: InputAction::TogglePanels });
        shortcuts.push(Shortcut { key: Key::Escape, modifiers: none, action: InputAction::Escape });

        Self { shortcuts }
    }

    /// Look up an action for a key + modifiers combination.
    pub fn lookup(&self, key: Key, modifiers: &Modifiers) -> InputAction {
        for s in &self.shortcuts {
            if s.key == key && s.modifiers == *modifiers {
                return s.action.clone();
            }
        }
        InputAction::None
    }

    /// Get all registered shortcuts.
    pub fn shortcuts(&self) -> &[Shortcut] {
        &self.shortcuts
    }

    /// Override a shortcut (for user customization).
    pub fn set_shortcut(&mut self, key: Key, modifiers: Modifiers, action: InputAction) {
        self.shortcuts.retain(|s| !(s.key == key && s.modifiers == modifiers));
        self.shortcuts.push(Shortcut { key, modifiers, action });
    }
}

// ---------------------------------------------------------------------------
// Mouse drag state
// ---------------------------------------------------------------------------

/// Tracks the current mouse drag state for panning and other drag operations.
#[derive(Debug, Clone, Default)]
pub struct DragState {
    /// Whether a drag is in progress.
    pub active: bool,
    /// Start position in device pixels (when drag began).
    pub start_x: f32,
    pub start_y: f32,
    /// Last position in device pixels (for computing delta).
    pub last_x: f32,
    pub last_y: f32,
    /// Scroll position at drag start (to restore if drag is cancelled).
    pub scroll_x_at_start: f32,
    pub scroll_y_at_start: f32,
}

// ---------------------------------------------------------------------------
// View controller
// ---------------------------------------------------------------------------

/// Processes input events and mutates the `ViewportState`.
///
/// The shell feeds raw `InputEvent` values into this controller, which
/// interprets them based on the current tool mode and shortcut config,
/// then applies the resulting viewport mutations.
pub struct ViewController {
    /// Current tool mode.
    pub tool: ToolMode,
    /// Keyboard shortcut registry.
    pub shortcuts: ShortcutRegistry,
    /// Current drag state (for hand/pan and other drag operations).
    pub drag: DragState,
    /// Whether tool stickiness is enabled (user-configurable).
    pub sticky_tools: bool,
    /// Scroll physics for velocity tracking and momentum.
    pub scroll_physics: ScrollPhysics,
    /// Timestamp of the last scroll input event (for velocity estimation).
    last_scroll_time: Option<Instant>,
}

impl ViewController {
    /// Create a new view controller with default settings.
    pub fn new() -> Self {
        Self {
            tool: ToolMode::Select,
            shortcuts: ShortcutRegistry::defaults(),
            drag: DragState::default(),
            sticky_tools: true,
            scroll_physics: ScrollPhysics::new(),
            last_scroll_time: None,
        }
    }

    /// Process a raw input event against the current viewport state.
    ///
    /// Returns a list of actions that were derived from the event. The caller
    /// executes these actions against the `ViewportState`.
    pub fn process_event(&mut self, event: &InputEvent, state: &ViewportState) -> Vec<InputAction> {
        match event {
            InputEvent::KeyDown { key, modifiers } => {
                let action = self.shortcuts.lookup(*key, modifiers);
                vec![action]
            }

            InputEvent::MousePress { x, y, button, modifiers: _ } => {
                match *button {
                    MouseButton::Left => {
                        match self.tool {
                            ToolMode::Hand => {
                                // Start pan drag.
                                self.drag = DragState {
                                    active: true,
                                    start_x: *x,
                                    start_y: *y,
                                    last_x: *x,
                                    last_y: *y,
                                    scroll_x_at_start: state.scroll_x,
                                    scroll_y_at_start: state.scroll_y,
                                };
                                vec![]
                            }
                            ToolMode::ZoomToRect => {
                                // Start marquee selection.
                                self.drag = DragState {
                                    active: true,
                                    start_x: *x,
                                    start_y: *y,
                                    last_x: *x,
                                    last_y: *y,
                                    scroll_x_at_start: 0.0,
                                    scroll_y_at_start: 0.0,
                                };
                                vec![]
                            }
                            _ => vec![],
                        }
                    }
                    MouseButton::Middle => {
                        // Middle-click always pans (Acrobat convention).
                        self.tool = ToolMode::Hand;
                        self.drag = DragState {
                            active: true,
                            start_x: *x,
                            start_y: *y,
                            last_x: *x,
                            last_y: *y,
                            scroll_x_at_start: state.scroll_x,
                            scroll_y_at_start: state.scroll_y,
                        };
                        vec![]
                    }
                    MouseButton::Right => vec![], // Context menu handled by shell.
                }
            }

            InputEvent::MouseMove { x, y, .. } => {
                if self.drag.active {
                    let dx = (self.drag.last_x - x) / state.scale;
                    let dy = (self.drag.last_y - y) / state.scale;
                    self.drag.last_x = *x;
                    self.drag.last_y = *y;
                    self.track_scroll_velocity(dx, dy);
                    vec![InputAction::ScrollBy { dx, dy }]
                } else {
                    vec![]
                }
            }

            InputEvent::MouseRelease { button, .. } => {
                if *button == MouseButton::Left || *button == MouseButton::Middle {
                    if self.drag.active {
                        let was_middle = *button == MouseButton::Middle;
                        self.drag.active = false;
                        if was_middle {
                            // Revert to previous tool after middle-click pan.
                            self.tool = ToolMode::Select;
                        }
                        // For ZoomToRect, the release completes the zoom.
                        // The caller checks drag state to finalize marquee zoom.
                    }
                }
                vec![]
            }

            InputEvent::Wheel { delta_x, delta_y, x, y, modifiers } => {
                if modifiers.ctrl || modifiers.meta {
                    // Ctrl/Cmd + wheel = zoom toward pointer. [DS-POINT-1]
                    let factor = if *delta_y > 0.0 { 1.1 } else { 1.0 / 1.1 };
                    vec![InputAction::ZoomBy { factor, focus_x: *x, focus_y: *y }]
                } else if modifiers.shift {
                    // Shift + wheel = horizontal scroll. [DS-POINT-1]
                    let dx = delta_x - delta_y; // Normalize wheel delta to horizontal.
                    let doc_dx = dx * 30.0;
                    self.track_scroll_velocity(doc_dx, 0.0);
                    vec![InputAction::ScrollBy { dx: doc_dx, dy: 0.0 }]
                } else {
                    // Plain wheel = vertical scroll. [DS-POINT-1]
                    let dy = delta_y * 30.0;
                    let dx = delta_x * 30.0;
                    self.track_scroll_velocity(dx, dy);
                    vec![InputAction::ScrollBy { dx, dy }]
                }
            }

            InputEvent::Pinch { scale, center_x, center_y } => {
                vec![InputAction::ZoomBy {
                    factor: *scale,
                    focus_x: *center_x,
                    focus_y: *center_y,
                }]
            }

            _ => vec![],
        }
    }

    /// Track scroll velocity from a scroll input event.
    ///
    /// `dx`, `dy` are the scroll deltas in document-space points. The method
    /// computes the time since the last scroll event and feeds it to the
    /// scroll physics for velocity estimation.
    fn track_scroll_velocity(&mut self, dx: f32, dy: f32) {
        let now = Instant::now();
        let dt = self.last_scroll_time
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(0.016); // Default to ~60fps if no previous event.
        self.last_scroll_time = Some(now);
        self.scroll_physics.on_scroll(dx, dy, dt);
    }

    /// Tick the scroll physics (momentum decay, edge resistance).
    ///
    /// Call this once per frame from the shell. `doc_height` is the total
    /// document height in document-space points. Returns `true` if the
    /// scroll position changed (repaint needed).
    pub fn tick_physics(&mut self, dt: f32, state: &mut ViewportState, doc_height: f32) -> bool {
        self.scroll_physics.tick(dt, state, doc_height)
    }

    /// Stop all scroll momentum (e.g., on explicit navigation like Page Up).
    pub fn stop_momentum(&mut self) {
        self.scroll_physics.stop();
    }

    /// Get a reference to the scroll physics.
    pub fn scroll_physics(&self) -> &ScrollPhysics {
        &self.scroll_physics
    }

    /// Finalize a marquee zoom after drag release.
    ///
    /// Call this after receiving a `MouseRelease` when the tool is `ZoomToRect`
    /// and a drag was active. Converts the drag rectangle to document-space
    /// coordinates and returns the zoom-to-rect action.
    /// Compute the document-space rectangle from a marquee drag.
    ///
    /// Call this after a `MouseRelease` when the tool is `ZoomToRect` and a
    /// drag was active. Returns `(doc_x, doc_y, doc_w, doc_h)` in document
    /// space, or `None` if the drag was too small.
    pub fn marquee_rect(&self, state: &ViewportState) -> Option<(f32, f32, f32, f32)> {
        if self.tool != ToolMode::ZoomToRect || !self.drag.active {
            return None;
        }

        let x1 = self.drag.start_x.min(self.drag.last_x);
        let y1 = self.drag.start_y.min(self.drag.last_y);
        let x2 = self.drag.start_x.max(self.drag.last_x);
        let y2 = self.drag.start_y.max(self.drag.last_y);
        let rect_w = x2 - x1;
        let rect_h = y2 - y1;

        if rect_w < 5.0 || rect_h < 5.0 {
            return None;
        }

        let doc_x = state.scroll_x + x1 / state.scale;
        let doc_y = state.scroll_y + y1 / state.scale;
        let doc_w = rect_w / state.scale;
        let doc_h = rect_h / state.scale;

        Some((doc_x, doc_y, doc_w, doc_h))
    }
}

impl Default for ViewController {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn default_state() -> ViewportState {
        ViewportState::new(800.0, 600.0)
    }

    #[test]
    fn arrow_keys_scroll() {
        let mut vc = ViewController::new();
        let state = default_state();
        let actions = vc.process_event(
            &InputEvent::KeyDown { key: Key::ArrowDown, modifiers: Modifiers::default() },
            &state,
        );
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            InputAction::ScrollBy { dy, .. } => assert!(*dy > 0.0),
            other => panic!("expected ScrollBy, got {other:?}"),
        }
    }

    #[test]
    fn ctrl_plus_zooms_in() {
        let mut vc = ViewController::new();
        let state = default_state();
        let actions = vc.process_event(
            &InputEvent::KeyDown {
                key: Key::Plus,
                modifiers: Modifiers { ctrl: true, ..Default::default() },
            },
            &state,
        );
        assert_eq!(actions, vec![InputAction::ZoomIn]);
    }

    #[test]
    fn ctrl_minus_zooms_out() {
        let mut vc = ViewController::new();
        let state = default_state();
        let actions = vc.process_event(
            &InputEvent::KeyDown {
                key: Key::Minus,
                modifiers: Modifiers { ctrl: true, ..Default::default() },
            },
            &state,
        );
        assert_eq!(actions, vec![InputAction::ZoomOut]);
    }

    #[test]
    fn h_key_switches_to_hand() {
        let mut vc = ViewController::new();
        let state = default_state();
        let actions = vc.process_event(
            &InputEvent::KeyDown { key: Key::Char('h'), modifiers: Modifiers::default() },
            &state,
        );
        assert_eq!(actions, vec![InputAction::ToolHand]);
    }

    #[test]
    fn hand_tool_starts_drag() {
        let mut vc = ViewController::new();
        vc.tool = ToolMode::Hand;
        let state = default_state();

        // Mouse press starts drag.
        vc.process_event(
            &InputEvent::MousePress {
                x: 400.0, y: 300.0,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            &state,
        );
        assert!(vc.drag.active);
        assert_eq!(vc.drag.start_x, 400.0);

        // Mouse move produces ScrollBy.
        let actions = vc.process_event(
            &InputEvent::MouseMove { x: 350.0, y: 300.0, buttons: MouseButtons { left: true, ..Default::default() } },
            &state,
        );
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            InputAction::ScrollBy { dx, dy } => {
                // Dragged 50px left → scroll right by 50/scale.
                assert!(*dx > 0.0);
                assert_eq!(*dy, 0.0);
            }
            other => panic!("expected ScrollBy, got {other:?}"),
        }

        // Mouse release ends drag.
        vc.process_event(
            &InputEvent::MouseRelease { x: 350.0, y: 300.0, button: MouseButton::Left },
            &state,
        );
        assert!(!vc.drag.active);
    }

    #[test]
    fn ctrl_wheel_zooms() {
        let mut vc = ViewController::new();
        let state = default_state();
        let actions = vc.process_event(
            &InputEvent::Wheel {
                delta_x: 0.0,
                delta_y: 120.0,
                x: 400.0,
                y: 300.0,
                modifiers: Modifiers { ctrl: true, ..Default::default() },
            },
            &state,
        );
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            InputAction::ZoomBy { factor, focus_x, focus_y } => {
                assert!(*factor > 1.0); // Zoom in.
                assert_eq!(*focus_x, 400.0);
                assert_eq!(*focus_y, 300.0);
            }
            other => panic!("expected ZoomBy, got {other:?}"),
        }
    }

    #[test]
    fn plain_wheel_scrolls() {
        let mut vc = ViewController::new();
        let state = default_state();
        // Negative delta_y = scroll up (Qt convention: scroll toward user = positive).
        let actions = vc.process_event(
            &InputEvent::Wheel {
                delta_x: 0.0,
                delta_y: -120.0,
                x: 400.0,
                y: 300.0,
                modifiers: Modifiers::default(),
            },
            &state,
        );
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            InputAction::ScrollBy { dy, .. } => assert!(*dy < 0.0), // Scroll up (negative dy).
            other => panic!("expected ScrollBy, got {other:?}"),
        }
    }

    #[test]
    fn middle_click_pans_and_reverts() {
        let mut vc = ViewController::new();
        let state = default_state();

        // Middle-click starts pan.
        vc.process_event(
            &InputEvent::MousePress {
                x: 400.0, y: 300.0,
                button: MouseButton::Middle,
                modifiers: Modifiers::default(),
            },
            &state,
        );
        assert_eq!(vc.tool, ToolMode::Hand);
        assert!(vc.drag.active);

        // Middle-release reverts to Select.
        vc.process_event(
            &InputEvent::MouseRelease { x: 400.0, y: 300.0, button: MouseButton::Middle },
            &state,
        );
        assert_eq!(vc.tool, ToolMode::Select);
        assert!(!vc.drag.active);
    }

    #[test]
    fn escape_returns_to_select() {
        let mut vc = ViewController::new();
        vc.tool = ToolMode::Ink;
        let state = default_state();
        let actions = vc.process_event(
            &InputEvent::KeyDown { key: Key::Escape, modifiers: Modifiers::default() },
            &state,
        );
        assert_eq!(actions, vec![InputAction::Escape]);
    }

    #[test]
    fn shortcut_registry_lookup() {
        let reg = ShortcutRegistry::defaults();
        let action = reg.lookup(Key::Char('h'), &Modifiers::default());
        assert_eq!(action, InputAction::ToolHand);
    }

    #[test]
    fn unbound_key_returns_none() {
        let reg = ShortcutRegistry::defaults();
        let action = reg.lookup(Key::Char('z'), &Modifiers::default());
        assert_eq!(action, InputAction::None);
    }

    #[test]
    fn wheel_scroll_tracks_velocity() {
        let mut vc = ViewController::new();
        let state = default_state();

        // Send a wheel event — should track velocity.
        vc.process_event(
            &InputEvent::Wheel {
                delta_x: 0.0,
                delta_y: -120.0,
                x: 400.0,
                y: 300.0,
                modifiers: Modifiers::default(),
            },
            &state,
        );

        assert!(vc.scroll_physics.velocity_y().abs() > 0.0,
            "velocity should be tracked after wheel scroll");
    }

    #[test]
    fn hand_drag_tracks_velocity() {
        let mut vc = ViewController::new();
        vc.tool = ToolMode::Hand;
        let state = default_state();

        // Start drag.
        vc.process_event(
            &InputEvent::MousePress {
                x: 400.0, y: 300.0,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            },
            &state,
        );

        // Move — should track velocity.
        vc.process_event(
            &InputEvent::MouseMove {
                x: 350.0, y: 250.0,
                buttons: MouseButtons { left: true, ..Default::default() },
            },
            &state,
        );

        assert!(vc.scroll_physics.velocity_y().abs() > 0.0 || vc.scroll_physics.velocity_x().abs() > 0.0,
            "velocity should be tracked after hand drag");
    }

    #[test]
    fn tick_physics_returns_false_when_idle() {
        let mut vc = ViewController::new();
        let mut state = default_state();
        let changed = vc.tick_physics(0.016, &mut state, 5000.0);
        assert!(!changed, "no change when no velocity");
    }

    #[test]
    fn tick_physics_applies_momentum() {
        let mut vc = ViewController::new();
        let mut state = default_state();

        // Simulate a fast scroll.
        vc.scroll_physics.on_scroll(0.0, 500.0, 0.01);

        // First tick should move the viewport.
        let changed = vc.tick_physics(0.016, &mut state, 5000.0);
        assert!(changed, "momentum should move viewport");
        assert!(state.scroll_y > 0.0, "scroll_y should increase: {}", state.scroll_y);
    }

    #[test]
    fn stop_momentum_cancels() {
        let mut vc = ViewController::new();
        vc.scroll_physics.on_scroll(0.0, 500.0, 0.01);
        assert!(vc.scroll_physics.is_momentum_active());

        vc.stop_momentum();
        assert!(!vc.scroll_physics.is_momentum_active());
    }
}
