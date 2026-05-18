//! Streaming Welch PSD + Short-Time Fourier Transform primitives.
//!
//! State-machine duals of `pwelch_psd` and `stft`: take a frame of new
//! samples, return the incremental analysis result. The state structs
//! own the ring buffer plus any running accumulators so callers only
//! pass the current input frame.
//!
//! Both share `SegmentBuffer`, which holds the samples between calls
//! and emits as many complete segments as fit. After draining each
//! segment, the buffer keeps the last `noverlap = win_len − hop`
//! samples so the next segment overlaps correctly.

use crate::convolution::next_power_of_two;
use crate::error::DspError;
use crate::fft::fft_raw;
use crate::welch::Sided;
use ndarray::{Array1, Array2};
use num_complex::Complex;
use rustlab_core::{CMatrix, CVector, RVector, C64};

/// Sliding-window segment producer shared by `pwelch_stream` and
/// `stft_stream`. Push frames in, drain complete `win_len`-long
/// segments out. After every drained segment the front of the buffer
/// is advanced by `hop` samples, leaving `noverlap` samples behind to
/// form the start of the next segment.
#[derive(Debug)]
pub(crate) struct SegmentBuffer {
    win_len: usize,
    hop: usize,
    buf: Vec<C64>,
}

impl SegmentBuffer {
    fn new(win_len: usize, noverlap: usize) -> Result<Self, DspError> {
        if win_len == 0 {
            return Err(DspError::InvalidParameter(
                "SegmentBuffer: win_len must be > 0".into(),
            ));
        }
        if noverlap >= win_len {
            return Err(DspError::InvalidParameter(format!(
                "SegmentBuffer: noverlap {noverlap} must be < win_len {win_len}"
            )));
        }
        Ok(Self {
            win_len,
            hop: win_len - noverlap,
            buf: Vec::with_capacity(2 * win_len),
        })
    }

    fn push_frame(&mut self, frame: &CVector) {
        self.buf.extend(frame.iter().copied());
    }

    /// Drain as many complete `win_len`-long segments as currently fit.
    /// Caller iterates over the returned slices. After this returns, the
    /// buffer is shifted forward by `n_segments · hop` samples.
    fn drain_segments(&mut self) -> Vec<Vec<C64>> {
        let mut out = Vec::new();
        let mut start = 0usize;
        while start + self.win_len <= self.buf.len() {
            out.push(self.buf[start..start + self.win_len].to_vec());
            start += self.hop;
        }
        if start > 0 {
            self.buf.drain(..start);
        }
        out
    }
}

// ─── pwelch_stream ───────────────────────────────────────────────────────────

/// Per-stream state for `pwelch_stream`.
#[derive(Debug)]
pub struct PwelchState {
    seg: SegmentBuffer,
    window: Vec<f64>,
    n_eff: usize,
    win_pow: f64,
    fs: f64,
    /// Two-sided accumulator of length `n_eff`.
    pxx_acc: Vec<f64>,
    n_segments: usize,
    /// `None` = cumulative running average; `Some(α)` = EMA with weight α.
    ema_alpha: Option<f64>,
    sided: Sided,
    /// Tracks whether *every* sample seen so far has zero imaginary part.
    all_real: bool,
}

/// Construct a streaming-pwelch state. `noverlap < window.len()`,
/// `nfft >= window.len()`. `ema_alpha`, when `Some`, switches the
/// running average to an exponential moving average; `None` gives the
/// cumulative form that converges to the batch `pwelch_psd`.
pub fn pwelch_stream_init(
    fs: f64,
    window: &RVector,
    noverlap: usize,
    nfft: usize,
    ema_alpha: Option<f64>,
    sided: Sided,
) -> Result<PwelchState, DspError> {
    let m = window.len();
    if m == 0 {
        return Err(DspError::InvalidParameter(
            "pwelch_stream_init: window is empty".into(),
        ));
    }
    if nfft < m {
        return Err(DspError::InvalidParameter(format!(
            "pwelch_stream_init: nfft {nfft} must be >= window length {m}"
        )));
    }
    if !(fs > 0.0) {
        return Err(DspError::InvalidParameter(format!(
            "pwelch_stream_init: fs {fs} must be > 0"
        )));
    }
    if let Some(a) = ema_alpha {
        if !(a > 0.0 && a <= 1.0) {
            return Err(DspError::InvalidParameter(format!(
                "pwelch_stream_init: ema_alpha {a} must be in (0, 1]"
            )));
        }
    }
    let win_pow: f64 = window.iter().map(|w| w * w).sum();
    if !(win_pow > 0.0) {
        return Err(DspError::InvalidParameter(
            "pwelch_stream_init: window has zero energy".into(),
        ));
    }
    let n_eff = next_power_of_two(nfft);
    Ok(PwelchState {
        seg: SegmentBuffer::new(m, noverlap)?,
        window: window.iter().copied().collect(),
        n_eff,
        win_pow,
        fs,
        pxx_acc: vec![0.0; n_eff],
        n_segments: 0,
        ema_alpha,
        sided,
        all_real: true,
    })
}

/// Push a frame of new samples into the streaming PSD state and return
/// the current PSD estimate. Returns an empty vector until the first
/// complete segment has been accumulated.
pub fn pwelch_stream(frame: &CVector, state: &mut PwelchState) -> RVector {
    if state.all_real {
        for c in frame.iter() {
            if c.im != 0.0 {
                state.all_real = false;
                break;
            }
        }
    }
    state.seg.push_frame(frame);
    let segs = state.seg.drain_segments();
    let mut fft_buf = vec![Complex::new(0.0, 0.0); state.n_eff];
    let scale = 1.0 / (state.fs * state.win_pow);
    for seg in &segs {
        for k in 0..state.window.len() {
            fft_buf[k] = seg[k] * state.window[k];
        }
        for k in state.window.len()..state.n_eff {
            fft_buf[k] = Complex::new(0.0, 0.0);
        }
        let spectrum = fft_raw(&fft_buf);
        match state.ema_alpha {
            Some(alpha) => {
                let one_minus = 1.0 - alpha;
                for k in 0..state.n_eff {
                    let per = spectrum[k].norm_sqr() * scale;
                    state.pxx_acc[k] = alpha * per + one_minus * state.pxx_acc[k];
                }
            }
            None => {
                let n_prev = state.n_segments as f64;
                let new_n = (state.n_segments + 1) as f64;
                for k in 0..state.n_eff {
                    let per = spectrum[k].norm_sqr() * scale;
                    state.pxx_acc[k] = (n_prev * state.pxx_acc[k] + per) / new_n;
                }
            }
        }
        state.n_segments += 1;
    }

    if state.n_segments == 0 {
        return Array1::zeros(0);
    }

    let onesided = match state.sided {
        Sided::OneSided => true,
        Sided::TwoSided => false,
        Sided::Auto => state.all_real,
    };
    if !onesided {
        return Array1::from_iter(state.pxx_acc.iter().copied());
    }
    let half = state.n_eff / 2;
    let mut out = vec![0.0; half + 1];
    out[0] = state.pxx_acc[0];
    for k in 1..half {
        out[k] = 2.0 * state.pxx_acc[k];
    }
    out[half] = state.pxx_acc[half];
    Array1::from_vec(out)
}

// ─── stft_stream ─────────────────────────────────────────────────────────────

/// Per-stream state for `stft_stream`.
#[derive(Debug)]
pub struct StftState {
    seg: SegmentBuffer,
    window: Vec<f64>,
    n_eff: usize,
    n_freqs: usize,
    sided: Sided,
    all_real: bool,
}

pub fn stft_stream_init(
    fs: f64,
    window: &RVector,
    noverlap: usize,
    nfft: usize,
    sided: Sided,
) -> Result<StftState, DspError> {
    let m = window.len();
    if m == 0 {
        return Err(DspError::InvalidParameter(
            "stft_stream_init: window is empty".into(),
        ));
    }
    if nfft < m {
        return Err(DspError::InvalidParameter(format!(
            "stft_stream_init: nfft {nfft} must be >= window length {m}"
        )));
    }
    if !(fs > 0.0) {
        return Err(DspError::InvalidParameter(format!(
            "stft_stream_init: fs {fs} must be > 0"
        )));
    }
    let n_eff = next_power_of_two(nfft);
    // Streaming requires a fixed row count, so we resolve `Auto` to
    // one-sided at init time. Pass `TwoSided` explicitly if you need
    // every bin.
    let onesided = !matches!(sided, Sided::TwoSided);
    let n_freqs = if onesided { n_eff / 2 + 1 } else { n_eff };
    Ok(StftState {
        seg: SegmentBuffer::new(m, noverlap)?,
        window: window.iter().copied().collect(),
        n_eff,
        n_freqs,
        sided,
        all_real: true,
    })
}

/// Push a frame of new samples into the streaming STFT state and
/// return any new spectrogram columns produced by the new samples.
/// When no segment boundary has been crossed, returns an
/// `n_freqs × 0` matrix so callers can always read `size(S, 1)`.
pub fn stft_stream(frame: &CVector, state: &mut StftState) -> CMatrix {
    if state.all_real {
        for c in frame.iter() {
            if c.im != 0.0 {
                state.all_real = false;
                break;
            }
        }
    }
    state.seg.push_frame(frame);
    let segs = state.seg.drain_segments();
    if segs.is_empty() {
        return Array2::<Complex<f64>>::zeros((state.n_freqs, 0));
    }
    let mut out = Array2::<Complex<f64>>::zeros((state.n_freqs, segs.len()));
    let mut fft_buf = vec![Complex::new(0.0, 0.0); state.n_eff];
    for (col, seg) in segs.iter().enumerate() {
        for k in 0..state.window.len() {
            fft_buf[k] = seg[k] * state.window[k];
        }
        for k in state.window.len()..state.n_eff {
            fft_buf[k] = Complex::new(0.0, 0.0);
        }
        let spectrum = fft_raw(&fft_buf);
        for row in 0..state.n_freqs {
            out[(row, col)] = spectrum[row];
        }
    }
    out
}

/// Reported one/two-sided convention of an `StftState`, for callers
/// that need to label the frequency axis.
pub fn stft_state_is_onesided(state: &StftState) -> bool {
    match state.sided {
        Sided::TwoSided => false,
        Sided::OneSided | Sided::Auto => true,
    }
}

/// Number of frequency bins (rows) this state emits per column.
pub fn stft_state_n_freqs(state: &StftState) -> usize {
    state.n_freqs
}
