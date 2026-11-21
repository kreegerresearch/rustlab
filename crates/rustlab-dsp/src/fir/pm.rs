//! Parks-McClellan optimal equiripple FIR filter design.
//!
//! Implements the Remez exchange algorithm: finds the unique FIR filter of a
//! given length whose frequency response minimises the *maximum* weighted error
//! against a piecewise-linear target — the Chebyshev (minimax) optimum.  The
//! result is **equiripple**: every ripple lobe in every band has exactly the
//! same height.  This is strictly better than windowed-sinc or Kaiser designs,
//! which waste stopband attenuation (ripple decays away from the transition
//! band).
//!
//! # Reference
//! Parks & McClellan, "Chebyshev Approximation for Nonrecursive Digital Filters
//! with Linear Phase", *IEEE Trans. Circuit Theory*, 1972.

use rustlab_core::Sum0;
use super::design::FirFilter;
use crate::error::DspError;
use ndarray::Array1;
use num_complex::Complex;
use std::f64::consts::PI;

// ── Tuning constants ─────────────────────────────────────────────────────────

/// Maximum Remez exchange iterations before giving up.
const MAX_ITER: usize = 100;

/// Convergence threshold on the relative change of δ (peak ripple).
const CONV_TOL: f64 = 1e-7;

/// Grid points generated per extremal frequency.  16 is standard;
/// more gives better extremal placement at the cost of speed.
const GRID_DENSITY: usize = 16;

// ── Internal types ────────────────────────────────────────────────────────────

/// One specified frequency band.
#[derive(Debug, Clone)]
struct Band {
    lower: f64, // normalised [0, 1] where 1 = Nyquist
    upper: f64,
    desired_lower: f64, // desired amplitude at the lower edge
    desired_upper: f64, // desired amplitude at the upper edge (linear interpolation within band)
    weight: f64,        // error weight for this band
}

/// One point on the dense frequency grid.
#[derive(Debug, Clone, Copy)]
struct Pt {
    freq: f64,    // normalised frequency [0, 1]
    desired: f64, // desired amplitude at this frequency
    weight: f64,  // error weight
}

impl Pt {
    /// The cosine-domain variable x = cos(π·freq) ∈ [−1, 1].
    /// x = 1 at DC (freq=0), x = −1 at Nyquist (freq=1).
    #[inline]
    fn x(self) -> f64 {
        (PI * self.freq).cos()
    }
}

// ── Public interface ──────────────────────────────────────────────────────────

/// Design an equiripple linear-phase FIR filter using the Parks-McClellan
/// (Remez exchange) algorithm.
///
/// # Parameters
///
/// - `n_taps` — number of filter taps.  Odd values produce a **Type I** filter
///   (symmetric, no zero forced at DC or Nyquist) and are recommended for
///   general use.  If an even value is given it is automatically rounded up to
///   the next odd integer.
///
/// - `bands` — band-edge frequencies as consecutive pairs, normalised to
///   `[0, 1]` where `1 = Nyquist`.  Example for a lowpass with a 0.25–0.35
///   transition band: `&[0.0, 0.25, 0.35, 1.0]`.  Gaps between pairs are
///   transition bands and are ignored by the optimiser.
///
/// - `desired` — desired amplitude at each band edge (same length as `bands`).
///   The desired response is linearly interpolated within each band.  Most
///   filters use constant-amplitude bands (both edges the same).
///
/// - `weights` — one non-negative weight per band pair.  A larger weight
///   forces tighter equiripple in that band at the expense of others.  Pass
///   `&[]` to use uniform weights of 1.0 for all bands.
///
/// # Errors
///
/// Returns [`DspError::InvalidPmSpec`] if the arguments are malformed (wrong
/// lengths, out-of-range frequencies, etc.).
pub fn firpm(
    n_taps: usize,
    bands: &[f64],
    desired: &[f64],
    weights: &[f64],
) -> Result<FirFilter, DspError> {
    // ── Validate ──────────────────────────────────────────────────────────────
    if n_taps < 3 {
        return Err(DspError::InvalidPmSpec(format!(
            "n_taps must be >= 3, got {n_taps}"
        )));
    }
    if bands.len() < 4 || bands.len() % 2 != 0 {
        return Err(DspError::InvalidPmSpec(
            "bands must have an even length >= 4 (one pair per band)".into(),
        ));
    }
    if desired.len() != bands.len() {
        return Err(DspError::InvalidPmSpec(format!(
            "desired must have the same length as bands ({}), got {}",
            bands.len(),
            desired.len()
        )));
    }
    for (i, &f) in bands.iter().enumerate() {
        if !(0.0..=1.0).contains(&f) {
            return Err(DspError::InvalidPmSpec(format!(
                "bands[{i}] = {f} is outside [0, 1]"
            )));
        }
    }
    for i in 0..bands.len() - 1 {
        if bands[i] > bands[i + 1] {
            return Err(DspError::InvalidPmSpec(format!(
                "bands must be non-decreasing; bands[{i}] > bands[{}]",
                i + 1
            )));
        }
    }
    let n_bands = bands.len() / 2;
    let weights: Vec<f64> = if weights.is_empty() {
        vec![1.0; n_bands]
    } else if weights.len() != n_bands {
        return Err(DspError::InvalidPmSpec(format!(
            "weights must have length {n_bands} (one per band), got {}",
            weights.len()
        )));
    } else {
        for (i, &w) in weights.iter().enumerate() {
            if w < 0.0 {
                return Err(DspError::InvalidPmSpec(format!(
                    "weights[{i}] = {w} is negative"
                )));
            }
        }
        weights.to_vec()
    };

    // ── Build band structs ────────────────────────────────────────────────────
    let band_specs: Vec<Band> = (0..n_bands)
        .map(|i| Band {
            lower: bands[2 * i],
            upper: bands[2 * i + 1],
            desired_lower: desired[2 * i],
            desired_upper: desired[2 * i + 1],
            weight: weights[i],
        })
        .collect();

    // ── Force odd tap count (Type I) ──────────────────────────────────────────
    let n_taps = if n_taps % 2 == 0 { n_taps + 1 } else { n_taps };
    let m = (n_taps - 1) / 2; // half-order: H(ω) = Σ_{k=0}^{m} a[k] cos(kω)

    // ── Run Remez ─────────────────────────────────────────────────────────────
    let n_ext = m + 2;
    let n_grid = (GRID_DENSITY * n_ext).max(512);
    let grid = build_grid(&band_specs, n_grid);

    if grid.len() < n_ext {
        return Err(DspError::InvalidPmSpec(format!(
            "grid too small ({}) for {n_ext} extremals — widen the bands or reduce n_taps",
            grid.len()
        )));
    }

    let cos_coeffs = remez(&grid, m)?;

    // ── Convert cosine coefficients → FIR taps ────────────────────────────────
    // H(ω) = a[0] + Σ_{k=1}^{m} a[k] cos(kω)
    // The symmetric impulse response: h[m] = a[0], h[m±k] = a[k]/2
    let mut taps = vec![0.0_f64; n_taps];
    taps[m] = cos_coeffs[0];
    for k in 1..=m {
        let half = cos_coeffs[k] / 2.0;
        taps[m - k] = half;
        taps[m + k] = half;
    }

    let coefficients = Array1::from_iter(taps.iter().map(|&h| Complex::new(h, 0.0)));
    Ok(FirFilter { coefficients })
}

// ── Quantized integer-coefficient design ─────────────────────────────────────

/// Design an integer-coefficient equiripple FIR filter.
///
/// Runs Parks-McClellan and then iteratively refines so that rounding the
/// coefficients to `bits`-bit integers stays as close as possible to the
/// minimax optimum.
///
/// Returns a `FirFilter` whose coefficients are integer-valued (stored as
/// `f64`, e.g. `127.0`, `-256.0`).  Divide by `(2^(bits-1) − 1)` to
/// recover the unit-gain floating-point response.
///
/// # Parameters
///
/// Same as [`firpm`], plus:
///
/// - `bits` — coefficient word length (2–32).  Coefficients will be in
///   `[−2^(bits-1), 2^(bits-1)−1]`.
/// - `n_iter` — number of iterative-compensation passes.  0 = plain round
///   with no refinement; 4–8 iterations are usually sufficient.
pub fn firpmq(
    n_taps: usize,
    bands: &[f64],
    desired: &[f64],
    weights: &[f64],
    bits: u32,
    n_iter: usize,
) -> Result<FirFilter, DspError> {
    if !(2..=32).contains(&bits) {
        return Err(DspError::InvalidPmSpec(format!(
            "firpmq: bits must be in [2, 32], got {bits}"
        )));
    }

    let i_max = (1i64 << (bits - 1)) as f64 - 1.0; // e.g. 32 767 for 16-bit

    // ── Initial float design ──────────────────────────────────────────────────
    let h0 = firpm(n_taps, bands, desired, weights)?;
    let n = h0.coefficients.len();
    let mut h_float: Vec<f64> = h0.coefficients.iter().map(|c| c.re).collect();

    // Scale so that max(|h|) maps to i_max.
    let peak: f64 = h_float.iter().map(|&x| x.abs()).fold(0.0_f64, f64::max);
    if peak < 1e-30 {
        return Err(DspError::InvalidPmSpec("firpmq: all-zero filter".into()));
    }
    let scale = i_max / peak;

    // ── Initial round ─────────────────────────────────────────────────────────
    let mut h_int: Vec<f64> = h_float
        .iter()
        .map(|&x| (x * scale).round().clamp(-i_max, i_max))
        .collect();

    // ── Iterative quantization-error compensation ─────────────────────────────
    //
    // Each pass computes the frequency-response error introduced by rounding
    // and pre-distorts the desired response to compensate.  After k iterations
    // the remaining rounding error shrinks toward zero.
    let cur_weights = if weights.is_empty() {
        vec![1.0; bands.len() / 2]
    } else {
        weights.to_vec()
    };

    for _ in 0..n_iter {
        // Residual = fractional part lost to rounding, in integer-scaled units.
        let residual: Vec<f64> = h_float
            .iter()
            .zip(h_int.iter())
            .map(|(&f, &i)| f * scale - i)
            .collect();

        // Evaluate the residual's frequency response at each band edge and
        // add it back to the desired target (pre-distortion).
        let adjusted_desired: Vec<f64> = bands
            .iter()
            .zip(desired.iter())
            .map(|(&freq, &d)| d + eval_fir_response(&residual, freq) / scale)
            .collect();

        let h_new = match firpm(n, bands, &adjusted_desired, &cur_weights) {
            Ok(f) => f,
            Err(_) => break,
        };
        h_float = h_new.coefficients.iter().map(|c| c.re).collect();
        h_int = h_float
            .iter()
            .map(|&x| (x * scale).round().clamp(-i_max, i_max))
            .collect();
    }

    // ── Enforce symmetry ──────────────────────────────────────────────────────
    // firpm always produces a symmetric (Type-I) filter; rounding independently
    // on each side can break it at the ±0.5 boundary.  Average and re-round.
    let m = (n - 1) / 2;
    for k in 0..m {
        let avg = ((h_int[k] + h_int[n - 1 - k]) / 2.0).round();
        h_int[k] = avg;
        h_int[n - 1 - k] = avg;
    }

    let coefficients = Array1::from_iter(h_int.iter().map(|&i| Complex::new(i, 0.0)));
    Ok(FirFilter { coefficients })
}

/// Evaluate the real-valued frequency response of a (symmetric) FIR tap
/// vector `h` at normalized frequency `f` ∈ [0, 1] (where 1 = Nyquist).
///
/// Uses the exact DTFT sum:
/// `H(f) = Σ_k h[k] · cos((k − m) · π · f)`, valid for any real h.
fn eval_fir_response(h: &[f64], f: f64) -> f64 {
    let omega = PI * f;
    let m = (h.len() as f64 - 1.0) / 2.0;
    h.iter()
        .enumerate()
        .map(|(k, &hk)| hk * ((k as f64 - m) * omega).cos())
        .sum()
}

// ── Grid construction ─────────────────────────────────────────────────────────

/// Build the dense frequency grid by distributing `n_grid` points
/// proportionally across all specified bands.
fn build_grid(bands: &[Band], n_grid: usize) -> Vec<Pt> {
    let total_bw: f64 = bands.iter().map(|b| (b.upper - b.lower).max(0.0)).sum0();
    if total_bw == 0.0 {
        return vec![];
    }

    let mut grid = Vec::with_capacity(n_grid + bands.len());
    for band in bands {
        let bw = (band.upper - band.lower).max(0.0);
        if bw == 0.0 {
            continue;
        }
        let n_pts = ((bw / total_bw * n_grid as f64).ceil() as usize).max(2);
        for i in 0..n_pts {
            let t = i as f64 / (n_pts - 1) as f64;
            let freq = band.lower + t * bw;
            let desired = band.desired_lower + t * (band.desired_upper - band.desired_lower);
            grid.push(Pt {
                freq,
                desired,
                weight: band.weight,
            });
        }
    }
    grid
}

// ── Remez exchange ────────────────────────────────────────────────────────────

/// Core Remez exchange loop.  Returns the `m+1` cosine series coefficients
/// `a[0], a[1], …, a[m]` of the optimal filter.
fn remez(grid: &[Pt], m: usize) -> Result<Vec<f64>, DspError> {
    let n_grid = grid.len();
    let n_ext = m + 2;

    // Initial extremals: evenly spaced across the grid indices
    let mut ext: Vec<usize> = (0..n_ext).map(|i| i * (n_grid - 1) / (n_ext - 1)).collect();

    let mut delta = 0.0_f64;

    for _ in 0..MAX_ITER {
        // Barycentric magnitudes at the current extremal x-values
        let x_ext: Vec<f64> = ext.iter().map(|&i| grid[i].x()).collect();
        let mag = bary_magnitudes(&x_ext);

        // Compute signed peak ripple δ
        let new_delta = compute_delta(&ext, grid, &mag);

        // Dense weighted error on the full grid
        let errors = compute_errors(grid, &ext, &mag, new_delta);

        // New extremal set from the error
        let new_ext = find_extremals(&errors, n_ext, n_grid);

        // Convergence: δ stabilised and the extremal set is unchanged
        let delta_converged = if delta.abs() > 1e-14 {
            ((new_delta.abs() - delta.abs()) / delta.abs()).abs() < CONV_TOL
        } else {
            new_delta.abs() < CONV_TOL
        };

        delta = new_delta;

        if delta_converged && new_ext == ext {
            break;
        }
        ext = new_ext;
    }

    // Final coefficient extraction using the converged extremals
    let x_ext: Vec<f64> = ext.iter().map(|&i| grid[i].x()).collect();
    let mag = bary_magnitudes(&x_ext);
    Ok(extract_cosine_coeffs(grid, &ext, &mag, delta, m))
}

// ── Barycentric weights ───────────────────────────────────────────────────────

/// Compute the **positive magnitude** of the Lagrange barycentric weights for
/// nodes `x[0] > x[1] > … > x[n-1]` (strictly decreasing order).
///
/// For nodes in decreasing order the sign of the k-th weight is `(-1)^k`, so
/// storing only the positive magnitudes is sufficient — callers apply the sign
/// manually.
///
/// Computed in log-space to avoid overflow/underflow for large m.
/// Returned values are normalised so the maximum is 1.
fn bary_magnitudes(x: &[f64]) -> Vec<f64> {
    let n = x.len();
    // log|λ_k| = − Σ_{j≠k} log|x_k − x_j|
    let mut log_m = vec![0.0_f64; n];
    for k in 0..n {
        for j in 0..n {
            if j != k {
                let d = (x[k] - x[j]).abs();
                if d < 1e-20 {
                    log_m[k] = f64::NEG_INFINITY;
                    break;
                }
                log_m[k] -= d.ln();
            }
        }
    }
    let max_log = log_m.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    log_m
        .iter()
        .map(|&l| {
            if l.is_finite() {
                (l - max_log).exp()
            } else {
                0.0
            }
        })
        .collect()
}

// ── δ computation ─────────────────────────────────────────────────────────────

/// Compute the signed peak ripple δ from the current extremal set.
///
/// Derivation (see module docs): the leading coefficient of the degree-M+1
/// interpolant through the M+2 extremals must be zero for H to have degree ≤ M.
/// This gives:
///
/// ```text
/// δ = [Σ_k (−1)^k |λ_k| D_k] / [Σ_k |λ_k| / W_k]
/// ```
///
/// where |λ_k| are the barycentric magnitudes (decreasing-node convention,
/// so the true λ_k = (−1)^k |λ_k|), D_k is the desired response, and W_k the
/// weight at extremal k.
fn compute_delta(ext: &[usize], grid: &[Pt], mag: &[f64]) -> f64 {
    let numer: f64 = ext
        .iter()
        .zip(mag.iter())
        .enumerate()
        .map(|(k, (&i, &m))| {
            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
            sign * m * grid[i].desired
        })
        .sum();
    let denom: f64 = ext
        .iter()
        .zip(mag.iter())
        .map(|(&i, &m)| m / grid[i].weight)
        .sum();
    if denom.abs() < 1e-30 {
        0.0
    } else {
        numer / denom
    }
}

// ── Error evaluation ──────────────────────────────────────────────────────────

/// Evaluate the weighted error E(ω) = W(ω)[D(ω) − H(ω)] at every grid point.
///
/// H is reconstructed from the current extremals via the barycentric Lagrange
/// formula:
///
/// ```text
/// H(x) = [Σ_k (−1)^k |λ_k| v_k / (x − x_k)] / [Σ_k (−1)^k |λ_k| / (x − x_k)]
/// ```
///
/// where v_k = D_k − (−1)^k δ/W_k is the target response at extremal k, and
/// x = cos(π·freq).
fn compute_errors(grid: &[Pt], ext: &[usize], mag: &[f64], delta: f64) -> Vec<f64> {
    let n_ext = ext.len();
    let x_ext: Vec<f64> = ext.iter().map(|&i| grid[i].x()).collect();

    // Target values at each extremal: v_k = D_k − (−1)^k δ/W_k
    let v_ext: Vec<f64> = (0..n_ext)
        .map(|k| {
            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
            grid[ext[k]].desired - sign * delta / grid[ext[k]].weight
        })
        .collect();

    grid.iter()
        .map(|pt| {
            let x = pt.x();

            // If x coincides with an extremal node return the exact value
            if let Some(k) = x_ext.iter().position(|&xe| (x - xe).abs() < 1e-13) {
                return pt.weight * (pt.desired - v_ext[k]);
            }

            // Barycentric Lagrange: apply (−1)^k sign from decreasing-order convention
            let mut numer = 0.0_f64;
            let mut denom = 0.0_f64;
            for k in 0..n_ext {
                let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
                let t = sign * mag[k] / (x - x_ext[k]);
                numer += t * v_ext[k];
                denom += t;
            }
            let h = if denom.abs() < 1e-30 {
                v_ext[0]
            } else {
                numer / denom
            };
            pt.weight * (pt.desired - h)
        })
        .collect()
}

// ── Extremal search ───────────────────────────────────────────────────────────

/// Find `n_ext` alternating extremals from the dense error vector.
///
/// Strategy:
/// 1. Scan the grid, partitioning it into sign-constant regions.
/// 2. From each region take the index with the maximum |E| → naturally
///    produces a sign-alternating list.
/// 3. If there are more than `n_ext`, repeatedly drop the end (first or last)
///    with the smaller |E| until the count reaches `n_ext`.
/// 4. If there are fewer than `n_ext` (degenerate/infeasible spec), fall back
///    to a uniform spread across the grid.
fn find_extremals(errors: &[f64], n_ext: usize, n_grid: usize) -> Vec<usize> {
    // ── Partition into sign regions ───────────────────────────────────────────
    // Each region is a contiguous run of non-zero same-sign error values.
    let mut regions: Vec<Vec<usize>> = Vec::new();
    let mut cur_sign = 0.0_f64;
    let mut cur_region: Vec<usize> = Vec::new();

    for (i, &e) in errors.iter().enumerate() {
        let s = if e > 0.0 {
            1.0
        } else if e < 0.0 {
            -1.0
        } else {
            0.0
        };
        if s == 0.0 {
            continue;
        }
        if s != cur_sign {
            if !cur_region.is_empty() {
                regions.push(cur_region.clone());
                cur_region.clear();
            }
            cur_sign = s;
        }
        cur_region.push(i);
    }
    if !cur_region.is_empty() {
        regions.push(cur_region);
    }

    // ── Best index per sign region (max |E|) ──────────────────────────────────
    let mut ext: Vec<usize> = regions
        .iter()
        .map(|region| {
            *region
                .iter()
                .max_by(|&&a, &&b| {
                    errors[a]
                        .abs()
                        .partial_cmp(&errors[b].abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap()
        })
        .collect();

    // ── Trim to exactly n_ext ─────────────────────────────────────────────────
    if ext.len() < n_ext {
        // Spec is likely infeasible for this tap count; fall back to uniform
        return (0..n_ext).map(|i| i * (n_grid - 1) / (n_ext - 1)).collect();
    }
    while ext.len() > n_ext {
        let e_first = errors[ext[0]].abs();
        let e_last = errors[*ext.last().unwrap()].abs();
        if e_first <= e_last {
            ext.remove(0);
        } else {
            ext.pop();
        }
    }
    ext
}

// ── Coefficient extraction ────────────────────────────────────────────────────

/// Recover the `m+1` cosine series coefficients from the converged extremals.
///
/// Evaluates H at the `m+1` DCT-I sample points ω_k = kπ/m (k = 0 … m)
/// using the barycentric formula, then inverts the DCT-I:
///
/// ```text
/// a[0]   = (1/m) [H₀/2 + H₁ + … + H_{m−1} + H_m/2]
/// a[n>0] = (2/m) [H₀/2 + Σ_{k=1}^{m−1} H_k cos(nkπ/m) + (−1)^n H_m/2]
/// ```
fn extract_cosine_coeffs(
    grid: &[Pt],
    ext: &[usize],
    mag: &[f64],
    delta: f64,
    m: usize,
) -> Vec<f64> {
    let n_ext = ext.len();
    let x_ext: Vec<f64> = ext.iter().map(|&i| grid[i].x()).collect();
    let v_ext: Vec<f64> = (0..n_ext)
        .map(|k| {
            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
            grid[ext[k]].desired - sign * delta / grid[ext[k]].weight
        })
        .collect();

    // Evaluate H at the m+1 DCT-I sample frequencies
    let h_vals: Vec<f64> = (0..=m)
        .map(|k| {
            let x = if m == 0 {
                1.0
            } else {
                (k as f64 * PI / m as f64).cos()
            };

            // Exact match with an extremal node?
            if let Some(j) = x_ext.iter().position(|&xe| (x - xe).abs() < 1e-13) {
                return v_ext[j];
            }

            // Barycentric formula
            let mut numer = 0.0_f64;
            let mut denom = 0.0_f64;
            for j in 0..n_ext {
                let sign = if j % 2 == 0 { 1.0 } else { -1.0 };
                let t = sign * mag[j] / (x - x_ext[j]);
                numer += t * v_ext[j];
                denom += t;
            }
            if denom.abs() < 1e-30 {
                v_ext[0]
            } else {
                numer / denom
            }
        })
        .collect();

    if m == 0 {
        return vec![h_vals[0]];
    }

    // Inverse DCT-I
    let mut a = vec![0.0_f64; m + 1];
    for n in 0..=m {
        let mut sum = 0.0_f64;
        for k in 0..=m {
            let gamma = if k == 0 || k == m { 0.5 } else { 1.0 };
            let angle = (n * k) as f64 * PI / m as f64;
            sum += gamma * h_vals[k] * angle.cos();
        }
        a[n] = if n == 0 || n == m {
            sum / m as f64
        } else {
            2.0 * sum / m as f64
        };
    }
    a
}
