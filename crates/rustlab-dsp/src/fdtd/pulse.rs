//! Source waveforms for the FDTD stepper.

use std::f64::consts::PI;

/// `exp(-((t - t0) / tau)^2) * cos(2π f0 (t - t0))`.
///
/// `f0 = 0` is a baseband Gaussian. The caller checks `tau > 0` and that
/// `t0` and `f0` are finite.
pub fn gaussian_pulse(t: f64, t0: f64, tau: f64, f0: f64) -> f64 {
    let s = t - t0;
    let envelope = (-(s / tau) * (s / tau)).exp();
    if f0 == 0.0 {
        envelope
    } else {
        envelope * (2.0 * PI * f0 * s).cos()
    }
}
