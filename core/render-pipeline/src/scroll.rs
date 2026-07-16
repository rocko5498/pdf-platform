//! Scroll physics: velocity tracking, momentum decay, edge resistance. [SDS §6.8, §6.9]
//!
//! Tracks scroll velocity from input events and optionally applies kinetic
//! momentum after the user releases. Rubber-banding at document edges provides
//! tactile feedback without letting the viewport escape.
//!
//! ## Design
//!
//! - **Velocity tracking**: computed from the time-weighted exponential moving
//!   average of recent scroll deltas. Fast recent events dominate; old events
//!   decay away. This gives smooth, responsive velocity estimation without
//!   needing exact timestamps for each event.
//! - **Momentum decay**: after the user releases (scroll events stop), the
//!   velocity decays exponentially each frame. When velocity drops below a
//!   threshold, momentum stops.
//! - **Edge resistance**: when the scroll position exceeds the document bounds,
//!   a restoring force pulls it back. The force is proportional to the overshoot
//!   (like a spring), creating a rubber-band feel.
//!
//! ## Integration
//!
//! The shell calls [`ScrollPhysics::on_scroll`] for each scroll input event,
//! then calls [`ScrollPhysics::tick`] each frame with the elapsed time.
//! The resulting scroll position and velocity are written back to `ViewportState`.

use crate::layout::ViewportState;

/// Configuration for scroll physics behavior.
#[derive(Debug, Clone)]
pub struct ScrollPhysicsConfig {
    /// Smoothing factor for velocity estimation (0.0..1.0).
    /// Higher = more responsive, lower = smoother. Default: 0.3.
    pub velocity_smoothing: f32,
    /// Exponential decay rate for momentum (units: 1/second).
    /// Higher = stops faster. Default: 8.0.
    pub momentum_decay: f32,
    /// Minimum velocity to sustain momentum (doc-points/sec). Default: 10.0.
    pub momentum_threshold: f32,
    /// Spring constant for edge resistance (higher = stiffer). Default: 300.0.
    pub edge_spring: f32,
    /// Damping for edge spring (higher = less bouncy). Default: 12.0.
    pub edge_damping: f32,
    /// Maximum overshoot past document bounds (doc-points). Default: 80.0.
    pub max_overshoot: f32,
}

impl Default for ScrollPhysicsConfig {
    fn default() -> Self {
        Self {
            velocity_smoothing: 0.3,
            momentum_decay: 8.0,
            momentum_threshold: 10.0,
            edge_spring: 300.0,
            edge_damping: 12.0,
            max_overshoot: 80.0,
        }
    }
}

/// Scroll physics state: tracks velocity and manages momentum/edge behavior.
#[derive(Debug, Clone)]
pub struct ScrollPhysics {
    /// Configuration.
    config: ScrollPhysicsConfig,
    /// Current estimated velocity in document-space points per second.
    /// Positive = scrolling down, negative = scrolling up.
    velocity_y: f32,
    /// Current estimated horizontal velocity.
    velocity_x: f32,
    /// Whether momentum is currently active (after user release).
    momentum_active: bool,
    /// Accumulated overshoot beyond document bounds (for edge resistance).
    edge_overshoot_y: f32,
    /// Time since last scroll input (for velocity estimation).
    time_since_last_input: f32,
}

impl ScrollPhysics {
    /// Create a new scroll physics instance.
    pub fn new() -> Self {
        Self {
            config: ScrollPhysicsConfig::default(),
            velocity_y: 0.0,
            velocity_x: 0.0,
            momentum_active: false,
            edge_overshoot_y: 0.0,
            time_since_last_input: 0.0,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(config: ScrollPhysicsConfig) -> Self {
        Self {
            config,
            ..Self::new()
        }
    }

    /// Get the current scroll velocity (doc-points/sec).
    pub fn velocity_y(&self) -> f32 {
        self.velocity_y
    }

    /// Get the current horizontal scroll velocity.
    pub fn velocity_x(&self) -> f32 {
        self.velocity_x
    }

    /// Whether momentum is currently driving the scroll.
    pub fn is_momentum_active(&self) -> bool {
        self.momentum_active
    }

    /// Record a scroll input event.
    ///
    /// `dx`, `dy` are the scroll deltas in document-space points (positive
    /// = right/down). `dt` is the time since the last scroll event in seconds.
    ///
    /// This updates the velocity estimate and activates momentum tracking.
    pub fn on_scroll(&mut self, dx: f32, dy: f32, dt: f32) {
        if dt <= 0.0 {
            return;
        }

        // Instantaneous velocity from this event.
        let instant_vy = dy / dt;
        let instant_vx = dx / dt;

        // Exponential moving average.
        let alpha = self.config.velocity_smoothing;
        self.velocity_y = alpha * instant_vy + (1.0 - alpha) * self.velocity_y;
        self.velocity_x = alpha * instant_vx + (1.0 - alpha) * self.velocity_x;

        self.time_since_last_input = 0.0;
        self.momentum_active = true;
    }

    /// Apply one frame of physics simulation.
    ///
    /// `dt` is the elapsed time since the last frame in seconds.
    /// `state` is the current viewport state (will be modified).
    /// `doc_height` is the total document height in document-space points.
    ///
    /// Returns `true` if the scroll position changed (i.e., momentum or edge
    /// resistance is active and a repaint is needed).
    pub fn tick(&mut self, dt: f32, state: &mut ViewportState, doc_height: f32) -> bool {
        if dt <= 0.0 || dt > 0.5 {
            // Skip unreasonable dt (e.g., after a long pause).
            self.time_since_last_input += dt;
            return false;
        }

        self.time_since_last_input += dt;
        let mut changed = false;

        // -- Momentum decay --
        if self.momentum_active {
            // Check if we've received no input for a while (user released).
            let idle_time = self.time_since_last_input;
            if idle_time > 0.05 {
                // Apply exponential decay to velocity.
                let decay = (-self.config.momentum_decay * dt).exp();
                self.velocity_y *= decay;
                self.velocity_x *= decay;

                // Stop momentum when velocity is negligible.
                if self.velocity_y.abs() < self.config.momentum_threshold
                    && self.velocity_x.abs() < self.config.momentum_threshold
                {
                    self.velocity_y = 0.0;
                    self.velocity_x = 0.0;
                    self.momentum_active = false;
                }
            }
        }

        // -- Apply velocity --
        if self.velocity_y.abs() > 0.1 || self.velocity_x.abs() > 0.1 {
            let scroll_dy = self.velocity_y * dt;
            let scroll_dx = self.velocity_x * dt;
            state.scroll_y += scroll_dy;
            state.scroll_x += scroll_dx;
            changed = true;
        }

        // -- Edge resistance --
        let vis_h = state.viewport_height / state.scale;
        let max_scroll = (doc_height - vis_h).max(0.0);

        let overshoot_top = -state.scroll_y;
        let overshoot_bottom = state.scroll_y - max_scroll;
        let max_os = self.config.max_overshoot;

        if overshoot_top > 0.0 && self.velocity_y <= 0.0 {
            // Scrolled past the top.
            let clamped_overshoot = overshoot_top.min(max_os);
            let spring_force = -self.config.edge_spring * clamped_overshoot;
            let damp_force = -self.config.edge_damping * self.velocity_y;
            let acceleration = spring_force + damp_force;
            self.velocity_y += acceleration * dt;

            // Pull scroll back toward 0.
            state.scroll_y = (-clamped_overshoot + (spring_force * dt * dt * 0.5)).max(-max_os);
            self.edge_overshoot_y = clamped_overshoot;
            changed = true;
        } else if overshoot_bottom > 0.0 && self.velocity_y >= 0.0 {
            // Scrolled past the bottom.
            let clamped_overshoot = overshoot_bottom.min(max_os);
            let spring_force = -self.config.edge_spring * clamped_overshoot;
            let damp_force = -self.config.edge_damping * self.velocity_y;
            let acceleration = spring_force + damp_force;
            self.velocity_y += acceleration * dt;

            // Pull scroll back toward max_scroll.
            state.scroll_y = max_scroll + clamped_overshoot + (spring_force * dt * dt * 0.5);
            state.scroll_y = state.scroll_y.min(max_scroll + max_os);
            self.edge_overshoot_y = clamped_overshoot;
            changed = true;
        } else {
            self.edge_overshoot_y = 0.0;
        }

        // -- Normal clamping (no overshoot when not momentum-active) --
        if !self.momentum_active && self.edge_overshoot_y <= 0.0 {
            state.scroll_y = state.scroll_y.clamp(0.0, max_scroll.max(0.0));
            state.scroll_x = state.scroll_x.clamp(0.0, 0.0);
        }

        // -- Update viewport state velocity for prefetch --
        state.scroll_velocity_y = self.velocity_y;

        changed
    }

    /// Stop all momentum and edge resistance.
    ///
    /// Called when the user takes an explicit action (e.g., clicking a
    /// bookmark, pressing Page Up) that should cancel inertial scrolling.
    pub fn stop(&mut self) {
        self.velocity_y = 0.0;
        self.velocity_x = 0.0;
        self.momentum_active = false;
        self.edge_overshoot_y = 0.0;
    }

    /// Reset the physics state (e.g., on document close).
    pub fn reset(&mut self) {
        self.stop();
        self.time_since_last_input = 0.0;
    }
}

impl Default for ScrollPhysics {
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
    fn velocity_tracking() {
        let mut physics = ScrollPhysics::new();
        // Simulate a fast downward scroll: 100px in 10ms = 10000 px/sec.
        // With alpha=0.3, first event gives 0.3 * 10000 = 3000.
        physics.on_scroll(0.0, 100.0, 0.01);
        assert!(physics.velocity_y() > 1000.0, "velocity should be tracked: {}", physics.velocity_y());

        // Second event: 0.3 * 8000 + 0.7 * 3000 = 4500
        physics.on_scroll(0.0, 80.0, 0.01);
        assert!(physics.velocity_y() > 3000.0, "velocity should increase: {}", physics.velocity_y());
    }

    #[test]
    fn momentum_decay() {
        let mut physics = ScrollPhysics::new();
        let mut state = default_state();
        state.scroll_velocity_y = 0.0;

        // Start with a high velocity.
        physics.on_scroll(0.0, 100.0, 0.01);
        let initial_v = physics.velocity_y();

        // Tick several frames — velocity should decay.
        for _ in 0..30 {
            physics.tick(0.016, &mut state, 5000.0);
        }
        assert!(physics.velocity_y().abs() < initial_v.abs(),
            "velocity should decay: {} < {}", physics.velocity_y(), initial_v);
    }

    #[test]
    fn momentum_stops() {
        let mut physics = ScrollPhysics::new();
        let mut state = default_state();

        physics.on_scroll(0.0, 10.0, 0.01);
        assert!(physics.is_momentum_active());

        // Tick enough frames for momentum to stop.
        for _ in 0..300 {
            physics.tick(0.016, &mut state, 5000.0);
        }
        assert!(!physics.is_momentum_active(), "momentum should stop");
        assert!(physics.velocity_y().abs() < 1.0);
    }

    #[test]
    fn edge_resistance_top() {
        let mut physics = ScrollPhysics::new();
        let mut state = default_state();
        state.scroll_y = -20.0; // Past the top.

        let changed = physics.tick(0.016, &mut state, 5000.0);
        assert!(changed, "edge resistance should be active");
        // Scroll should be pulled back toward 0.
        assert!(state.scroll_y >= -80.0, "should not overshoot too far: {}", state.scroll_y);
    }

    #[test]
    fn edge_resistance_bottom() {
        let mut physics = ScrollPhysics::new();
        let mut state = default_state();
        state.scroll_y = 5100.0; // Past the bottom (doc height 5000, viewport 600).

        let changed = physics.tick(0.016, &mut state, 5000.0);
        assert!(changed, "edge resistance should be active");
        assert!(state.scroll_y <= 5080.0, "should not overshoot too far: {}", state.scroll_y);
    }

    #[test]
    fn stop_cancels_momentum() {
        let mut physics = ScrollPhysics::new();
        physics.on_scroll(0.0, 100.0, 0.01);
        assert!(physics.is_momentum_active());

        physics.stop();
        assert!(!physics.is_momentum_active());
        assert_eq!(physics.velocity_y(), 0.0);
    }

    #[test]
    fn no_movement_when_velocity_zero() {
        let mut physics = ScrollPhysics::new();
        let mut state = default_state();
        let original_y = state.scroll_y;

        let changed = physics.tick(0.016, &mut state, 5000.0);
        assert!(!changed);
        assert_eq!(state.scroll_y, original_y);
    }

    #[test]
    fn velocity_updates_viewport_state() {
        let mut physics = ScrollPhysics::new();
        let mut state = default_state();

        physics.on_scroll(0.0, 50.0, 0.01);
        physics.tick(0.016, &mut state, 5000.0);

        assert!(state.scroll_velocity_y.abs() > 0.0,
            "scroll_velocity_y should be set: {}", state.scroll_velocity_y);
    }

    #[test]
    fn zero_dt_ignored() {
        let mut physics = ScrollPhysics::new();
        let _state = default_state();
        physics.on_scroll(0.0, 100.0, 0.0);
        // dt=0 should not update velocity.
        assert_eq!(physics.velocity_y(), 0.0);
    }
}
