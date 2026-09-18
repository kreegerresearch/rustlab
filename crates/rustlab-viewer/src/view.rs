//! Per-panel view state for the 2D plot panels.
//!
//! Two decisions live here, both as pure functions so they can be unit
//! tested without a GUI:
//!
//! * [`bounds_action`] — when the renderer should push the script's
//!   `xlim` / `ylim` into `egui_plot`'s bounds memory. Pushing them on
//!   *every* frame (what the viewer used to do) silently undid the
//!   user's own zoom/pan a frame after they made it, so the limits are
//!   applied only on the panel's first show, when fresh limits arrive
//!   from the script, and on Home.
//! * [`zoom_factor_from_scroll`] — how a wheel notch maps to a zoom
//!   factor. Plain scroll zooms in the viewer (egui_plot's own default
//!   is scroll-to-pan, ctrl+scroll-to-zoom), so this mirrors the
//!   `exp(delta * speed)` curve egui uses for ctrl+scroll and the two
//!   gestures feel the same.

/// A script-supplied axis limit pair (`plot_limits` / `xlim` / `ylim`).
/// `None` on either end means the script did not pin that edge.
pub type AxisLimits = (Option<f64>, Option<f64>);

/// Interaction state a panel carries across frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelView {
    /// Set when the panel is created and whenever *changed* limits
    /// arrive from the script; cleared once the renderer has pushed
    /// them into the plot's bounds memory.
    pub pending_limits: bool,
    /// Set by the panel's Home button (or the Home key while the panel
    /// is hovered); cleared after the reset has been applied.
    pub home_requested: bool,
}

impl Default for PanelView {
    fn default() -> Self {
        Self {
            // A fresh panel has never shown, so its limits (if any) still
            // need to reach egui_plot.
            pending_limits: true,
            home_requested: false,
        }
    }
}

/// What the renderer should do with `egui_plot`'s bounds memory this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundsAction {
    /// Leave the bounds alone — whatever the user zoomed or panned to
    /// stays put.
    Keep,
    /// Push the script's limits. `auto_x` / `auto_y` hand an axis the
    /// script did *not* fully pin back to data auto-fit.
    Apply { auto_x: bool, auto_y: bool },
    /// No script limits to restore: fit the data.
    AutoFit,
}

/// Decide how this frame should treat the panel's bounds.
///
/// An axis counts as pinned only when the script gave *both* ends of it
/// (`plot_limits(fig, 1, [0, 10], [])` pins x and leaves y automatic);
/// a half-open limit is ignored, which is the behaviour the viewer has
/// always had.
pub fn bounds_action(xlim: AxisLimits, ylim: AxisLimits, view: PanelView) -> BoundsAction {
    if !view.pending_limits && !view.home_requested {
        return BoundsAction::Keep;
    }
    let x_fixed = xlim.0.is_some() && xlim.1.is_some();
    let y_fixed = ylim.0.is_some() && ylim.1.is_some();
    if x_fixed || y_fixed {
        BoundsAction::Apply {
            auto_x: !x_fixed,
            auto_y: !y_fixed,
        }
    } else if view.home_requested {
        BoundsAction::AutoFit
    } else {
        // First show of a limit-free panel: egui_plot already auto-fits
        // by default, so there is nothing to push.
        BoundsAction::Keep
    }
}

/// Same curve egui uses for ctrl+scroll zoom (`Options::scroll_zoom_speed`).
const SCROLL_ZOOM_SPEED: f64 = 1.0 / 200.0;

/// Ceiling on a single frame's zoom step. Mouse wheels report in large,
/// unsmoothed notches on some platforms; without a clamp one flick could
/// jump several orders of magnitude and lose the data off-screen.
const MAX_ZOOM_STEP: f64 = 4.0;

/// Zoom factor for a vertical scroll delta, in `egui_plot`'s convention:
/// `> 1.0` zooms in (shrinks the visible range), `< 1.0` zooms out,
/// `1.0` is a no-op. Non-finite input is treated as no scroll.
pub fn zoom_factor_from_scroll(scroll: f32) -> f32 {
    let delta = scroll as f64;
    if !delta.is_finite() || delta == 0.0 {
        return 1.0;
    }
    ((delta * SCROLL_ZOOM_SPEED).exp()).clamp(1.0 / MAX_ZOOM_STEP, MAX_ZOOM_STEP) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: AxisLimits = (None, None);

    fn first_show() -> PanelView {
        PanelView::default()
    }

    fn settled() -> PanelView {
        PanelView {
            pending_limits: false,
            home_requested: false,
        }
    }

    fn home() -> PanelView {
        PanelView {
            pending_limits: false,
            home_requested: true,
        }
    }

    #[test]
    fn first_show_applies_script_limits() {
        assert_eq!(
            bounds_action(
                (Some(0.0), Some(10.0)),
                (Some(-1.0), Some(1.0)),
                first_show()
            ),
            BoundsAction::Apply {
                auto_x: false,
                auto_y: false
            }
        );
    }

    #[test]
    fn unpinned_axis_stays_automatic() {
        // x pinned, y left to the data.
        assert_eq!(
            bounds_action((Some(0.0), Some(10.0)), NONE, first_show()),
            BoundsAction::Apply {
                auto_x: false,
                auto_y: true
            }
        );
        // Only y pinned.
        assert_eq!(
            bounds_action(NONE, (Some(0.0), Some(1.0)), first_show()),
            BoundsAction::Apply {
                auto_x: true,
                auto_y: false
            }
        );
    }

    #[test]
    fn half_open_limits_are_ignored() {
        // A single edge is not enough to pin an axis, so this is the same
        // as having no limits at all.
        assert_eq!(
            bounds_action((Some(0.0), None), (None, Some(1.0)), first_show()),
            BoundsAction::Keep
        );
    }

    #[test]
    fn limit_free_first_show_keeps_default_auto_fit() {
        assert_eq!(bounds_action(NONE, NONE, first_show()), BoundsAction::Keep);
    }

    /// The regression this whole latch exists for: once the limits have
    /// been applied, later frames must not touch the bounds again or the
    /// user's zoom is undone one frame after they scroll.
    #[test]
    fn settled_panel_keeps_user_zoom() {
        assert_eq!(
            bounds_action((Some(0.0), Some(10.0)), (Some(-1.0), Some(1.0)), settled()),
            BoundsAction::Keep
        );
        assert_eq!(bounds_action(NONE, NONE, settled()), BoundsAction::Keep);
    }

    #[test]
    fn home_restores_script_limits_when_present() {
        assert_eq!(
            bounds_action((Some(0.0), Some(10.0)), (Some(-1.0), Some(1.0)), home()),
            BoundsAction::Apply {
                auto_x: false,
                auto_y: false
            }
        );
    }

    #[test]
    fn home_without_limits_fits_the_data() {
        assert_eq!(bounds_action(NONE, NONE, home()), BoundsAction::AutoFit);
        assert_eq!(
            bounds_action((Some(0.0), None), NONE, home()),
            BoundsAction::AutoFit
        );
    }

    #[test]
    fn scroll_up_zooms_in_scroll_down_zooms_out() {
        assert!(zoom_factor_from_scroll(50.0) > 1.0);
        assert!(zoom_factor_from_scroll(-50.0) < 1.0);
    }

    #[test]
    fn opposite_scrolls_cancel_out() {
        let inward = zoom_factor_from_scroll(37.0) as f64;
        let outward = zoom_factor_from_scroll(-37.0) as f64;
        assert!((inward * outward - 1.0).abs() < 1e-6);
    }

    #[test]
    fn no_scroll_is_a_no_op() {
        assert_eq!(zoom_factor_from_scroll(0.0), 1.0);
        assert_eq!(zoom_factor_from_scroll(f32::NAN), 1.0);
        assert_eq!(zoom_factor_from_scroll(f32::INFINITY), 1.0);
    }

    #[test]
    fn one_notch_is_a_modest_step() {
        // A single logitech notch on a macbook reports ~14 points.
        let f = zoom_factor_from_scroll(14.0);
        assert!(f > 1.0 && f < 1.2, "one notch zoomed by {f}x");
    }

    #[test]
    fn giant_wheel_flicks_are_clamped() {
        assert_eq!(zoom_factor_from_scroll(100_000.0), MAX_ZOOM_STEP as f32);
        assert_eq!(
            zoom_factor_from_scroll(-100_000.0),
            (1.0 / MAX_ZOOM_STEP) as f32
        );
    }
}
