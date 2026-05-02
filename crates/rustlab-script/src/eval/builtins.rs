use crate::error::ScriptError;
use crate::eval::value::{insert_commas, Value};
use ndarray::{Array1, Array2, Array3};
use num_complex::Complex;
use rand::Rng;
use rand_distr::{Distribution, Normal, Uniform};
use rustlab_core::{CMatrix, CVector, SparseMat, SparseVec, C64};
use rustlab_core::{OverflowMode, RoundMode};
use rustlab_dsp::convolution::convolve;
use rustlab_dsp::fixed::{qadd as fixed_qadd, qconv as fixed_qconv, qmul as fixed_qmul};
use rustlab_dsp::{
    butterworth_highpass, butterworth_lowpass, curl_2d, curl_3d, divergence_2d, divergence_3d, fft,
    fftfreq, fftshift, fir_bandpass, fir_bandpass_kaiser, fir_highpass, fir_highpass_kaiser,
    fir_lowpass, fir_lowpass_kaiser, fir_notch, firpm, firpmq, freqz, gradient_2d, gradient_3d,
    ifft, quantize_scalar, snr_db, upfirdn, IirFilter, QFmtSpec, WindowFunction,
};
use rustlab_plot::{
    colormap_rgb, compute_histogram, histogram_matrix, imagesc_terminal, plot_db, plot_histogram,
    push_xy_bar, push_xy_line, push_xy_scatter, push_xy_stem, render_figure_file,
    render_figure_terminal, render_heatmap_tui, render_image_tui, surf_terminal,
    sync_figure_outputs, HeatmapData, HeatmapKind, LineStyle, LiveFigure, LivePlot, SeriesColor,
    FIGURE,
};
use std::collections::HashMap;
use std::io::{Read as IoRead, Write as IoWrite};
use std::sync::{Arc, Mutex};

pub type BuiltinFn = fn(Vec<Value>) -> Result<Value, ScriptError>;
/// Nargout-aware builtin signature. The `nargout` argument is the number of
/// outputs the call site expects: `1` for `e = eig(A)`, `2` for
/// `[V, D] = eig(A)`, etc. Most builtins ignore it; only the few that
/// overload on caller arity (`eig`, `sort`, `find`, `svd` …) need it.
pub type BuiltinFnNargout = fn(Vec<Value>, usize) -> Result<Value, ScriptError>;

#[derive(Clone, Copy)]
enum BuiltinKind {
    Stateless(BuiltinFn),
    Nargout(BuiltinFnNargout),
}

pub struct BuiltinRegistry {
    map: HashMap<String, BuiltinKind>,
}

impl BuiltinRegistry {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut r = Self::new();
        // DSP
        r.register("fir_lowpass", builtin_fir_lowpass);
        r.register("fir_highpass", builtin_fir_highpass);
        r.register("fir_bandpass", builtin_fir_bandpass);
        r.register("butterworth_lowpass", builtin_butterworth_lowpass);
        r.register("butterworth_highpass", builtin_butterworth_highpass);
        r.register("convolve", builtin_convolve);
        r.register("filtfilt", builtin_filtfilt);
        r.register("upfirdn", builtin_upfirdn);
        r.register("window", builtin_window);
        // FFT
        r.register("fft", builtin_fft);
        r.register("ifft", builtin_ifft);
        r.register("fftshift", builtin_fftshift);
        r.register("fftfreq", builtin_fftfreq);
        r.register("spectrum", builtin_spectrum);
        // Kaiser FIR
        r.register("fir_lowpass_kaiser", builtin_fir_lowpass_kaiser);
        r.register("fir_highpass_kaiser", builtin_fir_highpass_kaiser);
        r.register("fir_bandpass_kaiser", builtin_fir_bandpass_kaiser);
        r.register("fir_notch", builtin_fir_notch);
        r.register("freqz", builtin_freqz);
        // Parks-McClellan optimal FIR
        r.register("firpm", builtin_firpm);
        r.register("firpmq", builtin_firpmq);
        // Fixed-point quantization
        r.register("qfmt", builtin_qfmt);
        r.register("quantize", builtin_quantize);
        r.register("qadd", builtin_qadd);
        r.register("qmul", builtin_qmul);
        r.register("qconv", builtin_qconv);
        r.register("snr", builtin_snr);
        // Math
        r.register("abs", builtin_abs);
        r.register("angle", builtin_angle);
        r.register("real", builtin_real);
        r.register("imag", builtin_imag);
        r.register("conj", builtin_conj);
        r.register("cos", builtin_cos);
        r.register("sin", builtin_sin);
        r.register("acos", builtin_acos);
        r.register("asin", builtin_asin);
        r.register("atan", builtin_atan);
        r.register("tanh", builtin_tanh);
        r.register("sqrt", builtin_sqrt);
        r.register("exp", builtin_exp);
        r.register("log", builtin_log);
        r.register("log10", builtin_log10);
        r.register("log2", builtin_log2);
        r.register("atan2", builtin_atan2);
        r.register("sinh", builtin_sinh);
        r.register("cosh", builtin_cosh);
        r.register("floor", builtin_floor);
        r.register("ceil", builtin_ceil);
        r.register("round", builtin_round);
        r.register("sign", builtin_sign);
        r.register("mod", builtin_mod);
        r.register("meshgrid", builtin_meshgrid);
        // Vector calculus
        r.register("gradient", builtin_gradient);
        r.register("divergence", builtin_divergence);
        r.register("curl", builtin_curl);
        r.register("gradient3", builtin_gradient3);
        r.register("divergence3", builtin_divergence3);
        r.register("curl3", builtin_curl3);
        // Array construction
        r.register("zeros", builtin_zeros);
        r.register("ones", builtin_ones);
        r.register("linspace", builtin_linspace);
        r.register("rand", builtin_rand);
        r.register("randn", builtin_randn);
        r.register("randi", builtin_randi);
        r.register("seed", builtin_seed);
        // Tensor3 (rank-3) constructors
        r.register("zeros3", builtin_zeros3);
        r.register("ones3", builtin_ones3);
        r.register("rand3", builtin_rand3);
        r.register("randn3", builtin_randn3);
        r.register("histogram", builtin_histogram);
        r.register("hist", builtin_histogram);
        r.register("mean", builtin_mean);
        r.register("median", builtin_median);
        r.register("std", builtin_std);
        r.register("min", builtin_min);
        r.register("max", builtin_max);
        r.register("sum", builtin_sum);
        r.register("prod", builtin_prod);
        r.register("cumsum", builtin_cumsum);
        r.register("argmin", builtin_argmin);
        r.register("argmax", builtin_argmax);
        r.register_nargout("sort", builtin_sort_nargout);
        r.register("trapz", builtin_trapz);
        r.register("len", builtin_len);
        r.register("length", builtin_len); // alias for len
        r.register("numel", builtin_numel);
        r.register("size", builtin_size);
        r.register("ndims", builtin_ndims);
        // I/O
        r.register("print", builtin_print);
        r.register("plot", builtin_plot);
        r.register("stem", builtin_stem);
        r.register("plotdb", builtin_plotdb);
        r.register("savefig", builtin_savefig);
        r.register("frame", builtin_frame);
        r.register("saveanim", builtin_saveanim);
        // Figure state control
        r.register("figure", builtin_figure);
        r.register("hold", builtin_hold);
        r.register("grid", builtin_grid);
        r.register("xlabel", builtin_xlabel);
        r.register("ylabel", builtin_ylabel);
        r.register("title", builtin_title);
        r.register("loglog", builtin_loglog);
        r.register("semilogx", builtin_semilogx);
        r.register("semilogy", builtin_semilogy);
        r.register("polar", builtin_polar);
        r.register("xlim", builtin_xlim);
        r.register("ylim", builtin_ylim);
        r.register("subplot", builtin_subplot);
        r.register("legend", builtin_legend);
        r.register("hline", builtin_hline);
        r.register("yline", builtin_hline); // common alias
        r.register("imagesc", builtin_imagesc);
        r.register("heatmap", builtin_heatmap);
        r.register("image", builtin_image);
        r.register("surf", builtin_surf);
        r.register("contour", builtin_contour);
        r.register("contourf", builtin_contourf);
        r.register("quiver", builtin_quiver);
        r.register("streamplot", builtin_streamplot);
        // Import / export
        r.register("save", builtin_save);
        r.register("load", builtin_load);
        r.register("whos", builtin_whos_file);
        // Matrix construction
        r.register("eye", builtin_eye);
        // Matrix operations
        r.register("transpose", builtin_transpose);
        r.register("diag", builtin_diag);
        r.register("trace", builtin_trace);
        r.register("reshape", builtin_reshape);
        r.register("repmat", builtin_repmat);
        r.register("horzcat", builtin_horzcat);
        r.register("vertcat", builtin_vertcat);
        r.register("cat", builtin_cat);
        r.register("permute", builtin_permute);
        r.register("squeeze", builtin_squeeze);
        // Linear algebra
        r.register("dot", builtin_dot);
        r.register("cross", builtin_cross);
        r.register("outer", builtin_outer);
        r.register("kron", builtin_kron);
        r.register("norm", builtin_norm);
        r.register("det", builtin_det);
        r.register("inv", builtin_inv);
        r.register("expm", builtin_expm);
        r.register("linsolve", builtin_linsolve);
        r.register_nargout("eig", builtin_eig_nargout);
        r.register("eigs", builtin_eigs);
        // Special functions
        r.register("laguerre", builtin_laguerre);
        r.register("legendre", builtin_legendre);
        // Number theory
        r.register("factor", builtin_factor);
        // Output
        r.register("disp", builtin_disp);
        r.register("fprintf", builtin_fprintf);
        r.register("sprintf", builtin_sprintf);
        r.register("commas", builtin_commas);
        r.register("error", builtin_error);
        // Aggregates
        r.register("all", builtin_all);
        r.register("any", builtin_any);
        // Matrix analysis
        r.register("rank", builtin_rank);
        r.register("roots", builtin_roots);
        // Transfer function (Phase 2)
        r.register("tf", builtin_tf);
        r.register("pole", builtin_pole);
        r.register("zero", builtin_zero);
        // State-space (Phase 3)
        r.register("ss", builtin_ss);
        r.register("ctrb", builtin_ctrb);
        r.register("obsv", builtin_obsv);
        // Frequency & time-domain analysis (Phase 4)
        r.register("bode", builtin_bode);
        r.register("step", builtin_step);
        r.register("margin", builtin_margin);
        // Optimal control (Phase 5)
        r.register("lqr", builtin_lqr);
        r.register("rlocus", builtin_rlocus);
        // Controls bootcamp
        r.register("logspace", builtin_logspace);
        r.register("lyap", builtin_lyap);
        r.register("gram", builtin_gram);
        r.register("care", builtin_care);
        r.register("dare", builtin_dare);
        r.register("place", builtin_place);
        r.register("freqresp", builtin_freqresp);
        r.register("svd", builtin_svd);
        // Struct construction
        r.register("struct", builtin_struct);
        // Type inspection
        r.register("isstruct", builtin_isstruct);
        r.register("fieldnames", builtin_fieldnames);
        r.register("isfield", builtin_isfield);
        r.register("rmfield", builtin_rmfield);
        // Cell / string arrays
        r.register("iscell", builtin_iscell);
        // ML / activation functions
        r.register("softmax", builtin_softmax);
        r.register("relu", builtin_relu);
        r.register("gelu", builtin_gelu);
        r.register("layernorm", builtin_layernorm);
        // New plot types
        r.register("bar", builtin_bar);
        r.register("scatter", builtin_scatter);

        // Streaming DSP
        r.register("state_init", builtin_state_init);
        r.register("filter_stream", builtin_filter_stream);

        // stdin/stdout audio I/O
        r.register("audio_in", builtin_audio_in);
        r.register("audio_out", builtin_audio_out);
        r.register("audio_read", builtin_audio_read);
        r.register("audio_write", builtin_audio_write);

        // Sparse
        r.register("sparse", builtin_sparse);
        r.register("sparsevec", builtin_sparsevec);
        r.register("speye", builtin_speye);
        r.register("spzeros", builtin_spzeros);
        r.register("nnz", builtin_nnz);
        r.register("issparse", builtin_issparse);
        r.register("full", builtin_full);
        r.register("nonzeros", builtin_nonzeros);
        r.register_nargout("find", builtin_find_nargout);
        r.register("spsolve", builtin_spsolve);
        r.register("spdiags", builtin_spdiags);
        r.register("sprand", builtin_sprand);
        r.register("laplacian_2d", builtin_laplacian_2d);
        r.register("laplacian_1d", builtin_laplacian_1d);
        r.register("laplacian_3d", builtin_laplacian_3d);
        r.register("laplacian_eps_2d", builtin_laplacian_eps_2d);
        r.register("ij2k", builtin_ij2k);
        r.register("k2ij", builtin_k2ij);
        r.register("ijk2k", builtin_ijk2k);
        r.register("k2ijk", builtin_k2ijk);

        // Geometry / shape rasterization masks
        r.register("rect_mask", builtin_rect_mask);
        r.register("disk_mask", builtin_disk_mask);
        r.register("polygon_mask", builtin_polygon_mask);

        // Live plotting
        r.register("figure_live", builtin_figure_live);
        r.register("plot_update", builtin_plot_update);
        r.register("plot_limits", builtin_plot_limits);
        r.register("plot_labels", builtin_plot_labels);
        r.register("figure_draw", builtin_figure_draw);
        r.register("figure_close", builtin_figure_close);
        r.register("mag2db", builtin_mag2db);
        r.register("sleep", builtin_sleep);

        r
    }

    pub fn register(&mut self, name: impl Into<String>, f: BuiltinFn) {
        self.map.insert(name.into(), BuiltinKind::Stateless(f));
    }

    /// Register a nargout-aware builtin. The function receives the number of
    /// outputs the call site expects and may dispatch on it (matlab-style
    /// overloading on caller arity).
    pub fn register_nargout(&mut self, name: impl Into<String>, f: BuiltinFnNargout) {
        self.map.insert(name.into(), BuiltinKind::Nargout(f));
    }

    pub fn call(&self, name: &str, args: Vec<Value>) -> Result<Value, ScriptError> {
        self.call_with_nargout(name, args, 1)
    }

    pub fn call_with_nargout(
        &self,
        name: &str,
        args: Vec<Value>,
        nargout: usize,
    ) -> Result<Value, ScriptError> {
        match self.map.get(name) {
            Some(BuiltinKind::Stateless(f)) => f(args),
            Some(BuiltinKind::Nargout(f)) => f(args, nargout),
            None => Err(ScriptError::undefined_fn(name.to_string())),
        }
    }
}

// ─── Helper macros / functions ─────────────────────────────────────────────

fn check_args(name: &str, args: &[Value], expected: usize) -> Result<(), ScriptError> {
    if args.len() != expected {
        Err(ScriptError::arg_count(
            name.to_string(),
            expected,
            args.len(),
        ))
    } else {
        Ok(())
    }
}

fn check_args_range(name: &str, args: &[Value], min: usize, max: usize) -> Result<(), ScriptError> {
    if args.len() < min || args.len() > max {
        Err(ScriptError::arg_count_range(
            name.to_string(),
            min,
            max,
            args.len(),
        ))
    } else {
        Ok(())
    }
}

fn parse_window(val: &Value) -> Result<WindowFunction, ScriptError> {
    let s = val.to_str().map_err(|e| ScriptError::type_err(e))?;
    WindowFunction::from_str(&s, None).map_err(ScriptError::Dsp)
}

fn cvector_to_value(v: CVector) -> Value {
    Value::Vector(v)
}

// ─── DSP builtins ──────────────────────────────────────────────────────────

fn builtin_fir_lowpass(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fir_lowpass", &args, 4)?;
    let num_taps = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let cutoff_hz = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let win = parse_window(&args[3])?;
    let filter = fir_lowpass(num_taps, cutoff_hz, sr, win)?;
    Ok(cvector_to_value(filter.coefficients))
}

fn builtin_fir_highpass(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fir_highpass", &args, 4)?;
    let num_taps = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let cutoff_hz = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let win = parse_window(&args[3])?;
    let filter = fir_highpass(num_taps, cutoff_hz, sr, win)?;
    Ok(cvector_to_value(filter.coefficients))
}

fn builtin_fir_bandpass(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fir_bandpass", &args, 5)?;
    let num_taps = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let low_hz = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let high_hz = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[3].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let win = parse_window(&args[4])?;
    let filter = fir_bandpass(num_taps, low_hz, high_hz, sr, win)?;
    Ok(cvector_to_value(filter.coefficients))
}

fn builtin_butterworth_lowpass(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("butterworth_lowpass", &args, 3)?;
    let order = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let cutoff_hz = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let filter = butterworth_lowpass(order, cutoff_hz, sr)?;
    // Return b coefficients as a complex vector for script use
    let coeffs: CVector = Array1::from_iter(filter.b.iter().map(|&x| Complex::new(x, 0.0)));
    Ok(Value::Vector(coeffs))
}

fn builtin_butterworth_highpass(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("butterworth_highpass", &args, 3)?;
    let order = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let cutoff_hz = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let filter = butterworth_highpass(order, cutoff_hz, sr)?;
    let coeffs: CVector = Array1::from_iter(filter.b.iter().map(|&x| Complex::new(x, 0.0)));
    Ok(Value::Vector(coeffs))
}

fn builtin_convolve(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("convolve", &args, 2)?;
    let x = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let h = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let result = convolve(&x, &h)?;
    Ok(Value::Vector(result))
}

/// filtfilt(b, a, x) — zero-phase forward-backward filter.
/// Applies filter(b,a) forward then backward so phase distortion cancels.
/// b and a are the numerator/denominator coefficients (a[0] must be 1).
fn builtin_filtfilt(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("filtfilt", &args, 3)?;
    let b_cv = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let a_cv = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let x_cv = args[2].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let b: Vec<f64> = b_cv.iter().map(|c| c.re).collect();
    let a: Vec<f64> = a_cv.iter().map(|c| c.re).collect();
    let x: Vec<f64> = x_cv.iter().map(|c| c.re).collect();
    if b.is_empty() || a.is_empty() {
        return Err(ScriptError::type_err(
            "filtfilt: b and a must be non-empty".to_string(),
        ));
    }
    let filt = IirFilter::new(b, a);
    let y = filt.filtfilt(&x);
    let result: CVector = Array1::from_iter(y.into_iter().map(|v| Complex::new(v, 0.0)));
    Ok(Value::Vector(result))
}

fn builtin_upfirdn(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() != 4 {
        return Err(ScriptError::runtime(format!(
            "upfirdn: expected 4 arguments (x, h, p, q), got {}",
            args.len()
        )));
    }
    let x = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let h_cv = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let h: Vec<f64> = h_cv.iter().map(|c| c.re).collect();
    let p = args[2].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let q = args[3].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let result = upfirdn(&x, &h, p, q)?;
    Ok(Value::Vector(result))
}

fn builtin_window(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("window", &args, 2)?;
    let win = parse_window(&args[0])?;
    let n = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let w = win.generate(n);
    // Convert RVector to CVector
    let cv: CVector = Array1::from_iter(w.iter().map(|&x| Complex::new(x, 0.0)));
    Ok(Value::Vector(cv))
}

// ─── Math builtins ─────────────────────────────────────────────────────────

fn builtin_abs(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("abs", &args, 1)?;
    match &args[0] {
        Value::Scalar(n) => Ok(Value::Scalar(n.abs())),
        Value::Complex(c) => Ok(Value::Scalar(c.norm())),
        Value::Vector(v) => {
            // Fast path: |re| is cheaper than sqrt(re²+im²) for real-only vectors.
            if is_real_vector(v) {
                Ok(Value::Vector(v.mapv(|c| Complex::new(c.re.abs(), 0.0))))
            } else {
                Ok(Value::Vector(v.mapv(|c| Complex::new(c.norm(), 0.0))))
            }
        }
        Value::Matrix(m) => {
            let result = m.mapv(|c| Complex::new(c.norm(), 0.0));
            Ok(Value::Matrix(result))
        }
        other => Err(ScriptError::type_err(format!(
            "abs: unsupported type {}",
            other
        ))),
    }
}

fn builtin_angle(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("angle", &args, 1)?;
    match &args[0] {
        Value::Scalar(n) => Ok(Value::Scalar(if *n >= 0.0 {
            0.0
        } else {
            std::f64::consts::PI
        })),
        Value::Complex(c) => Ok(Value::Scalar(c.arg())),
        Value::Vector(v) => {
            let result: CVector = Array1::from_iter(v.iter().map(|&c| Complex::new(c.arg(), 0.0)));
            Ok(Value::Vector(result))
        }
        other => Err(ScriptError::type_err(format!(
            "angle: unsupported type {}",
            other
        ))),
    }
}

fn builtin_real(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("real", &args, 1)?;
    match &args[0] {
        Value::Scalar(n) => Ok(Value::Scalar(*n)),
        Value::Complex(c) => Ok(Value::Scalar(c.re)),
        Value::Vector(v) => {
            let result: CVector = Array1::from_iter(v.iter().map(|&c| Complex::new(c.re, 0.0)));
            Ok(Value::Vector(result))
        }
        Value::Matrix(m) if m.nrows() == 1 && m.ncols() == 1 => Ok(Value::Scalar(m[[0, 0]].re)),
        Value::Matrix(m) => Ok(Value::Matrix(m.mapv(|c| Complex::new(c.re, 0.0)))),
        other => Err(ScriptError::type_err(format!(
            "real: unsupported type {}",
            other
        ))),
    }
}

fn builtin_imag(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("imag", &args, 1)?;
    match &args[0] {
        Value::Scalar(n) => Ok(Value::Scalar(if *n == 0.0 { 0.0 } else { 0.0 })),
        Value::Complex(c) => Ok(Value::Scalar(c.im)),
        Value::Vector(v) => {
            let result: CVector = Array1::from_iter(v.iter().map(|&c| Complex::new(c.im, 0.0)));
            Ok(Value::Vector(result))
        }
        Value::Matrix(m) if m.nrows() == 1 && m.ncols() == 1 => Ok(Value::Scalar(m[[0, 0]].im)),
        Value::Matrix(m) => Ok(Value::Matrix(m.mapv(|c| Complex::new(c.im, 0.0)))),
        other => Err(ScriptError::type_err(format!(
            "imag: unsupported type {}",
            other
        ))),
    }
}

fn builtin_conj(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("conj", &args, 1)?;
    match &args[0] {
        Value::Scalar(n) => Ok(Value::Scalar(*n)),
        Value::Complex(c) => Ok(Value::Complex(c.conj())),
        Value::Vector(v) => Ok(Value::Vector(v.mapv(|c| c.conj()))),
        Value::Matrix(m) => Ok(Value::Matrix(m.mapv(|c| c.conj()))),
        other => Err(ScriptError::type_err(format!(
            "conj: unsupported type {}",
            other.type_name()
        ))),
    }
}

fn apply_scalar_fn_to_value(
    name: &str,
    args: Vec<Value>,
    f: impl Fn(f64) -> f64,
    fc: impl Fn(Complex<f64>) -> Complex<f64>,
) -> Result<Value, ScriptError> {
    check_args(name, &args, 1)?;
    match &args[0] {
        Value::Scalar(n) => Ok(Value::Scalar(f(*n))),
        Value::Complex(c) => Ok(Value::Complex(fc(*c))),
        Value::Vector(v) => {
            // Fast path: avoid complex-number formula overhead for purely real vectors.
            if is_real_vector(v) {
                Ok(Value::Vector(v.mapv(|c| Complex::new(f(c.re), 0.0))))
            } else {
                Ok(Value::Vector(v.mapv(|c| fc(c))))
            }
        }
        Value::Matrix(m) => {
            if m.iter().all(|c| c.im == 0.0) {
                Ok(Value::Matrix(m.mapv(|c| Complex::new(f(c.re), 0.0))))
            } else {
                Ok(Value::Matrix(m.mapv(|c| fc(c))))
            }
        }
        other => Err(ScriptError::type_err(format!(
            "{}: unsupported type {}",
            name, other
        ))),
    }
}

fn builtin_cos(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("cos", args, f64::cos, |c: Complex<f64>| c.cos())
}

fn builtin_sin(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("sin", args, f64::sin, |c: Complex<f64>| c.sin())
}

fn builtin_acos(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("acos", args, f64::acos, |c: Complex<f64>| c.acos())
}

fn builtin_asin(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("asin", args, f64::asin, |c: Complex<f64>| c.asin())
}

fn builtin_atan(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("atan", args, f64::atan, |c: Complex<f64>| c.atan())
}

fn builtin_tanh(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("tanh", args, f64::tanh, |c: Complex<f64>| c.tanh())
}

fn builtin_sinh(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("sinh", args, f64::sinh, |c: Complex<f64>| c.sinh())
}

fn builtin_cosh(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("cosh", args, f64::cosh, |c: Complex<f64>| c.cosh())
}

// floor/ceil/round: apply to real and imaginary parts independently.
fn builtin_floor(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("floor", args, f64::floor, |c: Complex<f64>| {
        Complex::new(c.re.floor(), c.im.floor())
    })
}

fn builtin_ceil(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("ceil", args, f64::ceil, |c: Complex<f64>| {
        Complex::new(c.re.ceil(), c.im.ceil())
    })
}

fn builtin_round(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("round", args, f64::round, |c: Complex<f64>| {
        Complex::new(c.re.round(), c.im.round())
    })
}

// sign: real → -1/0/+1; complex → z/|z| (or 0 if z==0).
fn builtin_sign(args: Vec<Value>) -> Result<Value, ScriptError> {
    fn sign_real(x: f64) -> f64 {
        if x == 0.0 {
            0.0
        } else {
            x.signum()
        }
    }
    fn sign_complex(c: Complex<f64>) -> Complex<f64> {
        let m = c.norm();
        if m == 0.0 {
            Complex::new(0.0, 0.0)
        } else {
            c / m
        }
    }
    apply_scalar_fn_to_value("sign", args, sign_real, sign_complex)
}

// mod(a, m): a - m*floor(a/m), element-wise on real and imaginary parts.
fn builtin_mod(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("mod", &args, 2)?;
    fn mod_f64(a: f64, m: f64) -> f64 {
        a - m * (a / m).floor()
    }
    fn mod_c64(a: Complex<f64>, m: f64) -> Complex<f64> {
        Complex::new(mod_f64(a.re, m), mod_f64(a.im, m))
    }
    let m = match &args[1] {
        Value::Scalar(n) => *n,
        Value::Complex(c) if c.im == 0.0 => c.re,
        other => {
            return Err(ScriptError::type_err(format!(
                "mod: second argument must be a real scalar, got {}",
                other.type_name()
            )))
        }
    };
    match &args[0] {
        Value::Scalar(a) => Ok(Value::Scalar(mod_f64(*a, m))),
        Value::Complex(a) => Ok(Value::Complex(mod_c64(*a, m))),
        Value::Vector(v) => Ok(Value::Vector(v.mapv(|c| mod_c64(c, m)))),
        Value::Matrix(mx) => Ok(Value::Matrix(mx.mapv(|c| mod_c64(c, m)))),
        other => Err(ScriptError::type_err(format!(
            "mod: unsupported type {}",
            other.type_name()
        ))),
    }
}

fn builtin_sqrt(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("sqrt", args, f64::sqrt, |c: Complex<f64>| c.sqrt())
}

fn builtin_exp(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("exp", args, f64::exp, |c: Complex<f64>| c.exp())
}

fn builtin_log(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("log", args, f64::ln, |c: Complex<f64>| c.ln())
}

fn builtin_log10(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("log10", args, f64::log10, |c: Complex<f64>| {
        c.ln() / f64::ln(10.0)
    })
}

fn builtin_log2(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("log2", args, f64::log2, |c: Complex<f64>| {
        c.ln() / f64::ln(2.0)
    })
}

// ─── atan2(y, x) ──────────────────────────────────────────────────────────────

/// Element-wise four-quadrant arctangent: atan2(y, x) → angle in radians.
/// Both arguments may be scalar, vector, or matrix; shapes must match (or one scalar).
fn builtin_atan2(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("atan2", &args, 2)?;

    /// Extract real part of a C64, ignoring imaginary (atan2 is real-valued).
    fn re(c: C64) -> f64 {
        c.re
    }

    match (&args[0], &args[1]) {
        // scalar × scalar
        (Value::Scalar(y), Value::Scalar(x)) => Ok(Value::Scalar(y.atan2(*x))),

        // scalar × vector
        (Value::Scalar(y), Value::Vector(xv)) => {
            let v = Array1::from_iter(xv.iter().map(|&xc| Complex::new(y.atan2(re(xc)), 0.0)));
            Ok(Value::Vector(v))
        }
        (Value::Vector(yv), Value::Scalar(x)) => {
            let v = Array1::from_iter(yv.iter().map(|&yc| Complex::new(re(yc).atan2(*x), 0.0)));
            Ok(Value::Vector(v))
        }

        // vector × vector
        (Value::Vector(yv), Value::Vector(xv)) => {
            if yv.len() != xv.len() {
                return Err(ScriptError::type_err(format!(
                    "atan2: vector lengths must match ({} vs {})",
                    yv.len(),
                    xv.len()
                )));
            }
            let v = Array1::from_iter(
                yv.iter()
                    .zip(xv.iter())
                    .map(|(&yc, &xc)| Complex::new(re(yc).atan2(re(xc)), 0.0)),
            );
            Ok(Value::Vector(v))
        }

        // scalar × matrix
        (Value::Scalar(y), Value::Matrix(xm)) => {
            let m = xm.mapv(|xc| Complex::new(y.atan2(re(xc)), 0.0));
            Ok(Value::Matrix(m))
        }
        (Value::Matrix(ym), Value::Scalar(x)) => {
            let m = ym.mapv(|yc| Complex::new(re(yc).atan2(*x), 0.0));
            Ok(Value::Matrix(m))
        }

        // matrix × matrix
        (Value::Matrix(ym), Value::Matrix(xm)) => {
            if ym.shape() != xm.shape() {
                return Err(ScriptError::type_err(format!(
                    "atan2: matrix shapes must match ({}×{} vs {}×{})",
                    ym.nrows(),
                    ym.ncols(),
                    xm.nrows(),
                    xm.ncols()
                )));
            }
            let m = Array2::from_shape_fn(ym.raw_dim(), |(i, j)| {
                Complex::new(re(ym[[i, j]]).atan2(re(xm[[i, j]])), 0.0)
            });
            Ok(Value::Matrix(m))
        }

        (y, x) => Err(ScriptError::type_err(format!(
            "atan2: unsupported types {} and {}",
            y.type_name(),
            x.type_name()
        ))),
    }
}

// ─── meshgrid(x, y) ───────────────────────────────────────────────────────────

/// `[X, Y] = meshgrid(x, y)`
///
/// Given row vector x (length m) and row vector y (length n), return two n×m matrices:
///   X[i, j] = x[j]   (x varies along columns)
///   Y[i, j] = y[i]   (y varies along rows)
///
/// Uses 'xy' indexing: x varies along columns, y varies along rows.
fn builtin_meshgrid(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("meshgrid", &args, 2)?;

    let xv = args[0]
        .to_cvector()
        .map_err(|e| ScriptError::type_err(format!("meshgrid: x: {}", e)))?;
    let yv = args[1]
        .to_cvector()
        .map_err(|e| ScriptError::type_err(format!("meshgrid: y: {}", e)))?;

    let (m, n) = (xv.len(), yv.len()); // m cols, n rows

    let x_mat = Array2::from_shape_fn((n, m), |(_, j)| xv[j]);
    let y_mat = Array2::from_shape_fn((n, m), |(i, _)| yv[i]);

    Ok(Value::Tuple(vec![
        Value::Matrix(x_mat),
        Value::Matrix(y_mat),
    ]))
}

// ─── Geometry / shape rasterization masks ────────────────────────────────────

fn mask_xy<'a>(
    name: &str,
    args: &'a [Value],
) -> Result<(&'a CMatrix, &'a CMatrix), ScriptError> {
    let x = match &args[0] {
        Value::Matrix(m) => m,
        other => {
            return Err(ScriptError::type_err(format!(
                "{name}: X must be a matrix, got {}",
                other.type_name()
            )));
        }
    };
    let y = match &args[1] {
        Value::Matrix(m) => m,
        other => {
            return Err(ScriptError::type_err(format!(
                "{name}: Y must be a matrix, got {}",
                other.type_name()
            )));
        }
    };
    Ok((x, y))
}

/// `rect_mask(X, Y, x0, y0, w, h)` — axis-aligned rectangle mask, inclusive
/// on all four sides. Returns a real-valued matrix the same shape as X/Y
/// with `1.0` inside the rectangle and `0.0` outside.
fn builtin_rect_mask(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("rect_mask", &args, 6)?;
    let (x, y) = mask_xy("rect_mask", &args)?;
    let x0 = args[2]
        .to_scalar()
        .map_err(|e| ScriptError::type_err(format!("rect_mask: x0: {e}")))?;
    let y0 = args[3]
        .to_scalar()
        .map_err(|e| ScriptError::type_err(format!("rect_mask: y0: {e}")))?;
    let w = args[4]
        .to_scalar()
        .map_err(|e| ScriptError::type_err(format!("rect_mask: w: {e}")))?;
    let h = args[5]
        .to_scalar()
        .map_err(|e| ScriptError::type_err(format!("rect_mask: h: {e}")))?;
    let m = rustlab_dsp::rect_mask(x, y, x0, y0, w, h)
        .map_err(|e| ScriptError::type_err(e.to_string()))?;
    Ok(Value::Matrix(m))
}

/// `disk_mask(X, Y, xc, yc, r)` — closed-disk mask. Returns a real-valued
/// matrix the same shape as X/Y with `1.0` inside the disk and `0.0` outside.
fn builtin_disk_mask(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("disk_mask", &args, 5)?;
    let (x, y) = mask_xy("disk_mask", &args)?;
    let xc = args[2]
        .to_scalar()
        .map_err(|e| ScriptError::type_err(format!("disk_mask: xc: {e}")))?;
    let yc = args[3]
        .to_scalar()
        .map_err(|e| ScriptError::type_err(format!("disk_mask: yc: {e}")))?;
    let r = args[4]
        .to_scalar()
        .map_err(|e| ScriptError::type_err(format!("disk_mask: r: {e}")))?;
    let m = rustlab_dsp::disk_mask(x, y, xc, yc, r)
        .map_err(|e| ScriptError::type_err(e.to_string()))?;
    Ok(Value::Matrix(m))
}

/// `polygon_mask(X, Y, verts)` — polygon mask via even-odd ray casting.
/// `verts` is an N×2 matrix where each row is `[x, y]`. Returns a
/// real-valued matrix the same shape as X/Y with `1.0` inside the polygon
/// and `0.0` outside. Polygons with fewer than 3 vertices, or with all
/// vertices collinear, return an all-zero mask.
fn builtin_polygon_mask(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("polygon_mask", &args, 3)?;
    let (x, y) = mask_xy("polygon_mask", &args)?;
    let verts = match &args[2] {
        Value::Matrix(m) => m,
        other => {
            return Err(ScriptError::type_err(format!(
                "polygon_mask: verts must be an N×2 matrix, got {}",
                other.type_name()
            )));
        }
    };
    let m = rustlab_dsp::polygon_mask(x, y, verts)
        .map_err(|e| ScriptError::type_err(e.to_string()))?;
    Ok(Value::Matrix(m))
}

// ─── Vector calculus on uniform 2-D grids ────────────────────────────────────

fn unpack_dxdy(args: &[Value], name: &str, start: usize) -> Result<(f64, f64), ScriptError> {
    if args.len() <= start {
        return Ok((1.0, 1.0));
    }
    if args.len() == start + 2 {
        let dx = args[start]
            .to_scalar()
            .map_err(|e| ScriptError::type_err(format!("{name}: dx: {e}")))?;
        let dy = args[start + 1]
            .to_scalar()
            .map_err(|e| ScriptError::type_err(format!("{name}: dy: {e}")))?;
        Ok((dx, dy))
    } else {
        Err(ScriptError::type_err(format!(
            "{name}: expected dx and dy together (or neither), got {} extra args",
            args.len() - start
        )))
    }
}

/// `[Fx, Fy] = gradient(F)` or `gradient(F, dx, dy)`.
fn builtin_gradient(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("gradient", &args, 1, 3)?;
    let f = to_cmatrix_arg(&args[0], "gradient", "F")?;
    let (dx, dy) = unpack_dxdy(&args, "gradient", 1)?;
    let (fx, fy) = gradient_2d(&f, dx, dy)?;
    Ok(Value::Tuple(vec![Value::Matrix(fx), Value::Matrix(fy)]))
}

/// `D = divergence(Fx, Fy)` or `divergence(Fx, Fy, dx, dy)`.
fn builtin_divergence(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("divergence", &args, 2, 4)?;
    let fx = to_cmatrix_arg(&args[0], "divergence", "Fx")?;
    let fy = to_cmatrix_arg(&args[1], "divergence", "Fy")?;
    let (dx, dy) = unpack_dxdy(&args, "divergence", 2)?;
    let d = divergence_2d(&fx, &fy, dx, dy)?;
    Ok(Value::Matrix(d))
}

/// `Cz = curl(Fx, Fy)` or `curl(Fx, Fy, dx, dy)` — z-component of ∇×F.
fn builtin_curl(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("curl", &args, 2, 4)?;
    let fx = to_cmatrix_arg(&args[0], "curl", "Fx")?;
    let fy = to_cmatrix_arg(&args[1], "curl", "Fy")?;
    let (dx, dy) = unpack_dxdy(&args, "curl", 2)?;
    let c = curl_2d(&fx, &fy, dx, dy)?;
    Ok(Value::Matrix(c))
}

// ─── Vector calculus on uniform 3-D grids ────────────────────────────────────

fn unpack_dxdydz(args: &[Value], name: &str, start: usize) -> Result<(f64, f64, f64), ScriptError> {
    if args.len() <= start {
        return Ok((1.0, 1.0, 1.0));
    }
    if args.len() == start + 3 {
        let dx = args[start]
            .to_scalar()
            .map_err(|e| ScriptError::type_err(format!("{name}: dx: {e}")))?;
        let dy = args[start + 1]
            .to_scalar()
            .map_err(|e| ScriptError::type_err(format!("{name}: dy: {e}")))?;
        let dz = args[start + 2]
            .to_scalar()
            .map_err(|e| ScriptError::type_err(format!("{name}: dz: {e}")))?;
        Ok((dx, dy, dz))
    } else {
        Err(ScriptError::type_err(format!(
            "{name}: expected dx, dy and dz together (or none), got {} extra args",
            args.len() - start
        )))
    }
}

fn to_ctensor3_arg(
    val: &Value,
    fn_name: &str,
    arg_name: &str,
) -> Result<rustlab_core::CTensor3, ScriptError> {
    match val {
        Value::Tensor3(t) => Ok(t.clone()),
        other => Err(ScriptError::type_err(format!(
            "{}: {} must be a tensor3, got {}",
            fn_name,
            arg_name,
            other.type_name()
        ))),
    }
}

/// `[Fx, Fy, Fz] = gradient3(F)` or `gradient3(F, dx, dy, dz)`.
fn builtin_gradient3(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("gradient3", &args, 1, 4)?;
    let f = to_ctensor3_arg(&args[0], "gradient3", "F")?;
    let (dx, dy, dz) = unpack_dxdydz(&args, "gradient3", 1)?;
    let (fx, fy, fz) = gradient_3d(&f, dx, dy, dz)?;
    Ok(Value::Tuple(vec![
        Value::Tensor3(fx),
        Value::Tensor3(fy),
        Value::Tensor3(fz),
    ]))
}

/// `D = divergence3(Fx, Fy, Fz)` or `divergence3(Fx, Fy, Fz, dx, dy, dz)`.
fn builtin_divergence3(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("divergence3", &args, 3, 6)?;
    let fx = to_ctensor3_arg(&args[0], "divergence3", "Fx")?;
    let fy = to_ctensor3_arg(&args[1], "divergence3", "Fy")?;
    let fz = to_ctensor3_arg(&args[2], "divergence3", "Fz")?;
    let (dx, dy, dz) = unpack_dxdydz(&args, "divergence3", 3)?;
    let d = divergence_3d(&fx, &fy, &fz, dx, dy, dz)?;
    Ok(Value::Tensor3(d))
}

/// `[Cx, Cy, Cz] = curl3(Fx, Fy, Fz)` or `curl3(Fx, Fy, Fz, dx, dy, dz)`.
fn builtin_curl3(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("curl3", &args, 3, 6)?;
    let fx = to_ctensor3_arg(&args[0], "curl3", "Fx")?;
    let fy = to_ctensor3_arg(&args[1], "curl3", "Fy")?;
    let fz = to_ctensor3_arg(&args[2], "curl3", "Fz")?;
    let (dx, dy, dz) = unpack_dxdydz(&args, "curl3", 3)?;
    let (cx, cy, cz) = curl_3d(&fx, &fy, &fz, dx, dy, dz)?;
    Ok(Value::Tuple(vec![
        Value::Tensor3(cx),
        Value::Tensor3(cy),
        Value::Tensor3(cz),
    ]))
}

// ─── Array construction ────────────────────────────────────────────────────

/// Unpack size arguments for array constructors like zeros, ones, rand, randn.
/// Accepts: f(n), f(m,n), or f([m,n]) (vector from size()).
/// Returns (Some(m), n) for matrix or (None, n) for vector.
fn unpack_size_args(args: &[Value], name: &str) -> Result<(Option<usize>, usize), ScriptError> {
    if args.len() == 2 {
        let m = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
        let n = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
        Ok((Some(m), n))
    } else if let Value::Vector(v) = &args[0] {
        if v.len() == 2 {
            let m = v[0].re.round() as usize;
            let n = v[1].re.round() as usize;
            Ok((Some(m), n))
        } else {
            Err(ScriptError::type_err(format!(
                "{name}: expected scalar or 2-element vector, got {}-element vector",
                v.len()
            )))
        }
    } else {
        let n = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
        Ok((None, n))
    }
}

fn builtin_zeros(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("zeros", &args, 1, 2)?;
    let (m, n) = unpack_size_args(&args, "zeros")?;
    if let Some(m) = m {
        Ok(Value::Matrix(Array2::zeros((m, n))))
    } else {
        Ok(Value::Vector(Array1::zeros(n)))
    }
}

fn builtin_ones(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("ones", &args, 1, 2)?;
    let (m, n) = unpack_size_args(&args, "ones")?;
    if let Some(m) = m {
        Ok(Value::Matrix(Array2::from_elem(
            (m, n),
            Complex::new(1.0, 0.0),
        )))
    } else {
        Ok(Value::Vector(Array1::from_elem(n, Complex::new(1.0, 0.0))))
    }
}

fn builtin_linspace(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("linspace", &args, 3)?;
    let start = args[0].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let stop = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let n = args[2].to_usize().map_err(|e| ScriptError::type_err(e))?;
    if n == 0 {
        return Ok(Value::Vector(Array1::zeros(0)));
    }
    if n == 1 {
        // Octave / MATLAB convention: linspace(a, b, 1) returns [b],
        // not [a] (numpy's choice). rustlab follows Octave for
        // ergonomic parity with curriculum scripts.
        return Ok(Value::Vector(Array1::from_vec(vec![Complex::new(
            stop, 0.0,
        )])));
    }
    let step = (stop - start) / (n - 1) as f64;
    let v: CVector = Array1::from_iter((0..n).map(|i| Complex::new(start + step * i as f64, 0.0)));
    Ok(Value::Vector(v))
}

fn builtin_rand(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("rand", &args, 1, 2)?;
    let dist = Uniform::new(0.0_f64, 1.0);
    let (m, n) = unpack_size_args(&args, "rand")?;
    crate::eval::rng::with_rng(|rng| {
        if let Some(m) = m {
            let data: Vec<C64> = (0..m * n)
                .map(|_| Complex::new(dist.sample(rng), 0.0))
                .collect();
            Ok(Value::Matrix(Array2::from_shape_vec((m, n), data).unwrap()))
        } else {
            Ok(Value::Vector(Array1::from_iter(
                (0..n).map(|_| Complex::new(dist.sample(rng), 0.0)),
            )))
        }
    })
}

fn builtin_randn(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("randn", &args, 1, 2)?;
    let dist =
        Normal::new(0.0_f64, 1.0).map_err(|e| ScriptError::type_err(format!("randn: {e}")))?;
    let (m, n) = unpack_size_args(&args, "randn")?;
    crate::eval::rng::with_rng(|rng| {
        if let Some(m) = m {
            let data: Vec<C64> = (0..m * n)
                .map(|_| Complex::new(dist.sample(rng), 0.0))
                .collect();
            Ok(Value::Matrix(Array2::from_shape_vec((m, n), data).unwrap()))
        } else {
            Ok(Value::Vector(Array1::from_iter(
                (0..n).map(|_| Complex::new(dist.sample(rng), 0.0)),
            )))
        }
    })
}

/// Unpack (m, n, p) from either 3 scalar args or one 3-element vector.
fn unpack_tensor3_shape(args: &[Value], name: &str) -> Result<(usize, usize, usize), ScriptError> {
    match args.len() {
        3 => {
            let m = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
            let n = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
            let p = args[2].to_usize().map_err(|e| ScriptError::type_err(e))?;
            Ok((m, n, p))
        }
        1 => {
            if let Value::Vector(v) = &args[0] {
                if v.len() == 3 {
                    return Ok((
                        v[0].re.round() as usize,
                        v[1].re.round() as usize,
                        v[2].re.round() as usize,
                    ));
                }
            }
            Err(ScriptError::type_err(format!(
                "{name}: expected (m, n, p) or a 3-element vector"
            )))
        }
        n => Err(ScriptError::type_err(format!(
            "{name}: expected 1 or 3 arguments, got {n}"
        ))),
    }
}

fn builtin_zeros3(args: Vec<Value>) -> Result<Value, ScriptError> {
    let (m, n, p) = unpack_tensor3_shape(&args, "zeros3")?;
    Ok(Value::Tensor3(Array3::<C64>::zeros((m, n, p))))
}

fn builtin_ones3(args: Vec<Value>) -> Result<Value, ScriptError> {
    let (m, n, p) = unpack_tensor3_shape(&args, "ones3")?;
    Ok(Value::Tensor3(Array3::<C64>::from_elem(
        (m, n, p),
        Complex::new(1.0, 0.0),
    )))
}

fn builtin_rand3(args: Vec<Value>) -> Result<Value, ScriptError> {
    let (m, n, p) = unpack_tensor3_shape(&args, "rand3")?;
    let dist = Uniform::new(0.0_f64, 1.0);
    let data: Vec<C64> = crate::eval::rng::with_rng(|rng| {
        (0..m * n * p)
            .map(|_| Complex::new(dist.sample(rng), 0.0))
            .collect()
    });
    Ok(Value::Tensor3(
        Array3::from_shape_vec((m, n, p), data).unwrap(),
    ))
}

fn builtin_randn3(args: Vec<Value>) -> Result<Value, ScriptError> {
    let (m, n, p) = unpack_tensor3_shape(&args, "randn3")?;
    let dist =
        Normal::new(0.0_f64, 1.0).map_err(|e| ScriptError::type_err(format!("randn3: {e}")))?;
    let data: Vec<C64> = crate::eval::rng::with_rng(|rng| {
        (0..m * n * p)
            .map(|_| Complex::new(dist.sample(rng), 0.0))
            .collect()
    });
    Ok(Value::Tensor3(
        Array3::from_shape_vec((m, n, p), data).unwrap(),
    ))
}

fn builtin_randi(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() || args.len() > 2 {
        return Err(ScriptError::type_err(
            "randi: expected randi(imax) or randi(imax, n) or randi([lo,hi], n)".to_string(),
        ));
    }
    // First arg: scalar imax → range [1, imax], or 2-element vector [lo, hi]
    let (lo, hi) = match &args[0] {
        Value::Vector(v) if v.len() >= 2 => (v[0].re as i64, v[1].re as i64),
        Value::Vector(v) if v.len() == 1 => (1i64, v[0].re as i64),
        _ => {
            let imax = args[0].to_scalar().map_err(|e| ScriptError::type_err(e))? as i64;
            (1i64, imax)
        }
    };
    if lo > hi {
        return Err(ScriptError::type_err(format!(
            "randi: lo ({lo}) must be <= hi ({hi})"
        )));
    }
    crate::eval::rng::with_rng(|rng| {
        if args.len() == 1 {
            // Return a single scalar integer
            Ok(Value::Scalar(rng.gen_range(lo..=hi) as f64))
        } else {
            let n = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
            let v: CVector =
                Array1::from_iter((0..n).map(|_| Complex::new(rng.gen_range(lo..=hi) as f64, 0.0)));
            Ok(Value::Vector(v))
        }
    })
}

/// `seed(N)` — re-seed the shared RNG with a deterministic 64-bit value, so
/// every subsequent `rand` / `randn` / `randi` / `rand3` / `randn3` /
/// `sprand` call produces a reproducible sequence. Useful for notebooks
/// that commit rendered SVG/MD output to git: a `seed(N)` line at the top
/// of a notebook makes re-renders bit-stable.
///
/// `seed()` with no arguments re-seeds from OS entropy — restores the
/// non-deterministic behaviour that was the default before any seed call.
fn builtin_seed(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() {
        crate::eval::rng::seed_rng_from_entropy();
        return Ok(Value::None);
    }
    if args.len() != 1 {
        return Err(ScriptError::type_err(
            "seed: expected seed() or seed(N) where N is a non-negative integer".to_string(),
        ));
    }
    let s = args[0].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    if !s.is_finite() || s < 0.0 || s.fract() != 0.0 {
        return Err(ScriptError::type_err(format!(
            "seed: expected a non-negative integer, got {s}"
        )));
    }
    crate::eval::rng::seed_rng(s as u64);
    Ok(Value::None)
}

fn builtin_histogram(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() || args.len() > 2 {
        return Err(ScriptError::type_err(
            "histogram: expected histogram(v) or histogram(v, n_bins)".to_string(),
        ));
    }
    let data = to_real_vector(&args[0])?;
    let n_bins = if args.len() == 2 {
        args[1].to_usize().map_err(|e| ScriptError::type_err(e))?
    } else {
        10
    };
    plot_histogram(&data, n_bins, "Histogram").map_err(|e| ScriptError::type_err(e.to_string()))?;
    let (centers, counts, _) = compute_histogram(&data, n_bins);
    Ok(Value::Matrix(histogram_matrix(&centers, &counts)))
}

/// Detect the matlab-style empty-matrix placeholder used in `min(M, [], dim)`
/// and `max(M, [], dim)` to disambiguate the axis form from the 2-scalar
/// elementwise comparison form.
fn is_empty_matrix_placeholder(v: &Value) -> bool {
    match v {
        Value::Matrix(m) => m.is_empty(),
        Value::Vector(arr) => arr.is_empty(),
        _ => false,
    }
}

/// Reduce an `M×N` matrix's columns or rows by `cmp` (returning the running
/// extremum), used by both `min` and `max` axis forms.
fn matrix_axis_extremum(
    m: &CMatrix,
    dim: usize,
    init: f64,
    pick: fn(f64, f64) -> f64,
) -> Result<Value, ScriptError> {
    let nrows = m.nrows();
    let ncols = m.ncols();
    match dim {
        1 => {
            let mut data = vec![init; ncols];
            for ((_, j), x) in m.indexed_iter() {
                data[j] = pick(data[j], x.re);
            }
            let cdata: Vec<C64> = data.into_iter().map(|r| Complex::new(r, 0.0)).collect();
            Ok(Value::Matrix(
                Array2::from_shape_vec((1, ncols), cdata)
                    .map_err(|e| ScriptError::runtime(e.to_string()))?,
            ))
        }
        2 => {
            let mut data = vec![init; nrows];
            for ((i, _), x) in m.indexed_iter() {
                data[i] = pick(data[i], x.re);
            }
            let cdata: Vec<C64> = data.into_iter().map(|r| Complex::new(r, 0.0)).collect();
            Ok(Value::Matrix(
                Array2::from_shape_vec((nrows, 1), cdata)
                    .map_err(|e| ScriptError::runtime(e.to_string()))?,
            ))
        }
        _ => unreachable!(),
    }
}

fn builtin_min(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("min", &args, 1, 3)?;
    // 3-arg axis form: min(M, [], dim) — matlab convention. The empty
    // placeholder disambiguates from the 2-scalar elementwise form.
    if args.len() == 3 {
        if !is_empty_matrix_placeholder(&args[1]) {
            return Err(ScriptError::type_err(
                "min: 3-arg form is min(M, [], dim); 2nd argument must be the empty matrix []".to_string(),
            ));
        }
        let dim = args[2]
            .to_scalar()
            .map_err(|e| ScriptError::type_err(format!("min: dim: {}", e)))? as i64;
        if dim != 1 && dim != 2 {
            return Err(ScriptError::type_err(format!(
                "min: dim must be 1 or 2, got {}",
                dim
            )));
        }
        return match &args[0] {
            Value::Matrix(m) if !m.is_empty() => {
                matrix_axis_extremum(m, dim as usize, f64::INFINITY, f64::min)
            }
            Value::Vector(_) | Value::Scalar(_) => Ok(args[0].clone()),
            other => Err(ScriptError::type_err(format!(
                "min: argument must be a non-empty matrix, got {}",
                other.type_name()
            ))),
        };
    }
    // Two-scalar form `min(a, b)` retained — only fires when both args are
    // genuinely scalar. Matrix-input axis reductions take precedence below.
    if args.len() == 2
        && matches!(args[0], Value::Scalar(_))
        && matches!(args[1], Value::Scalar(_))
    {
        let a = args[0].to_scalar().unwrap();
        let b = args[1].to_scalar().unwrap();
        return Ok(Value::Scalar(a.min(b)));
    }
    match &args[0] {
        Value::Vector(v) if !v.is_empty() => {
            let m = v.iter().map(|c| c.re).fold(f64::INFINITY, f64::min);
            Ok(Value::Scalar(m))
        }
        Value::Matrix(m) if !m.is_empty() => {
            let nrows = m.nrows();
            let ncols = m.ncols();
            // 1-D-shaped matrix → flat reduction → scalar.
            if nrows == 1 || ncols == 1 {
                let v = m.iter().map(|c| c.re).fold(f64::INFINITY, f64::min);
                return Ok(Value::Scalar(v));
            }
            // 2-D matrix → row vector of column mins (default dim 1).
            matrix_axis_extremum(m, 1, f64::INFINITY, f64::min)
        }
        Value::Scalar(s) => Ok(Value::Scalar(*s)),
        _ => Err(ScriptError::type_err(
            "min: argument must be a non-empty vector, matrix, or scalar".to_string(),
        )),
    }
}

fn builtin_max(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("max", &args, 1, 3)?;
    if args.len() == 3 {
        if !is_empty_matrix_placeholder(&args[1]) {
            return Err(ScriptError::type_err(
                "max: 3-arg form is max(M, [], dim); 2nd argument must be the empty matrix []".to_string(),
            ));
        }
        let dim = args[2]
            .to_scalar()
            .map_err(|e| ScriptError::type_err(format!("max: dim: {}", e)))? as i64;
        if dim != 1 && dim != 2 {
            return Err(ScriptError::type_err(format!(
                "max: dim must be 1 or 2, got {}",
                dim
            )));
        }
        return match &args[0] {
            Value::Matrix(m) if !m.is_empty() => {
                matrix_axis_extremum(m, dim as usize, f64::NEG_INFINITY, f64::max)
            }
            Value::Vector(_) | Value::Scalar(_) => Ok(args[0].clone()),
            other => Err(ScriptError::type_err(format!(
                "max: argument must be a non-empty matrix, got {}",
                other.type_name()
            ))),
        };
    }
    if args.len() == 2
        && matches!(args[0], Value::Scalar(_))
        && matches!(args[1], Value::Scalar(_))
    {
        let a = args[0].to_scalar().unwrap();
        let b = args[1].to_scalar().unwrap();
        return Ok(Value::Scalar(a.max(b)));
    }
    match &args[0] {
        Value::Vector(v) if !v.is_empty() => {
            let m = v.iter().map(|c| c.re).fold(f64::NEG_INFINITY, f64::max);
            Ok(Value::Scalar(m))
        }
        Value::Matrix(m) if !m.is_empty() => {
            let nrows = m.nrows();
            let ncols = m.ncols();
            if nrows == 1 || ncols == 1 {
                let v = m.iter().map(|c| c.re).fold(f64::NEG_INFINITY, f64::max);
                return Ok(Value::Scalar(v));
            }
            matrix_axis_extremum(m, 1, f64::NEG_INFINITY, f64::max)
        }
        Value::Scalar(s) => Ok(Value::Scalar(*s)),
        _ => Err(ScriptError::type_err(
            "max: argument must be a non-empty vector, matrix, or scalar".to_string(),
        )),
    }
}

/// sleep(seconds) — pause execution for the given duration
fn builtin_sleep(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("sleep", &args, 1)?;
    let secs = match &args[0] {
        Value::Scalar(s) => *s,
        _ => {
            return Err(ScriptError::type_err(
                "sleep: argument must be a scalar (seconds)".to_string(),
            ))
        }
    };
    if secs < 0.0 {
        return Err(ScriptError::type_err(
            "sleep: duration must be non-negative".to_string(),
        ));
    }
    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    Ok(Value::Scalar(0.0))
}

fn builtin_error(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("error", &args, 1)?;
    let msg = match &args[0] {
        Value::Str(s) => s.clone(),
        other => format!("{}", other),
    };
    Err(ScriptError::runtime(msg))
}

/// mean(v) — average of all elements of a vector / 1-D-shaped matrix → scalar.
/// mean(M) — average along dim 1 (columns) → row matrix `1×ncols`.
/// mean(M, 1) / mean(M, 2) — explicit axis.
fn builtin_mean(args: Vec<Value>) -> Result<Value, ScriptError> {
    dispatch_matrix_reduction(
        "mean",
        &args,
        |elems| {
            if elems.is_empty() {
                return Err(ScriptError::type_err(
                    "mean: argument must be a non-empty vector, matrix, or scalar".to_string(),
                ));
            }
            let s: C64 = elems.iter().copied().sum();
            Ok(complex_to_value(s / elems.len() as f64))
        },
        |m, dim| {
            matrix_axis_collect_then(m, dim, |slice| {
                slice.iter().copied().sum::<C64>() / slice.len() as f64
            })
        },
    )
}

/// Median of a slice of complex values, comparing by real part.
fn median_of_real_slice(vals: &[C64]) -> f64 {
    let mut reals: Vec<f64> = vals.iter().map(|c| c.re).collect();
    reals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = reals.len();
    if n % 2 == 1 {
        reals[n / 2]
    } else {
        (reals[n / 2 - 1] + reals[n / 2]) / 2.0
    }
}

/// Sample standard deviation of a slice of complex values (real part-aware,
/// dividing by `n - 1`). Returns 0 for length-1 slices.
fn std_of_slice(vals: &[C64]) -> f64 {
    if vals.len() <= 1 {
        return 0.0;
    }
    let n = vals.len() as f64;
    let mean: C64 = vals.iter().copied().sum::<C64>() / n;
    let variance: f64 =
        vals.iter().map(|&x| (x - mean).norm_sqr()).sum::<f64>() / (n - 1.0);
    variance.sqrt()
}

/// median(v) — middle value of a vector / 1-D-shaped matrix → scalar.
/// median(M) — column medians → row matrix `1×ncols`.
/// median(M, 1) / median(M, 2) — explicit axis.
fn builtin_median(args: Vec<Value>) -> Result<Value, ScriptError> {
    dispatch_matrix_reduction(
        "median",
        &args,
        |elems| {
            if elems.is_empty() {
                return Err(ScriptError::type_err(
                    "median: argument must be a non-empty vector, matrix, or scalar".to_string(),
                ));
            }
            Ok(Value::Scalar(median_of_real_slice(elems)))
        },
        |m, dim| {
            matrix_axis_collect_then(m, dim, |slice| {
                Complex::new(median_of_real_slice(slice), 0.0)
            })
        },
    )
}

/// std(v) — sample standard deviation of a vector / 1-D-shaped matrix → scalar.
/// std(M) — column std → row matrix `1×ncols`.
/// std(M, 1) / std(M, 2) — explicit axis.
fn builtin_std(args: Vec<Value>) -> Result<Value, ScriptError> {
    dispatch_matrix_reduction(
        "std",
        &args,
        // std_of_slice handles empty (returns 0.0) and length-1 cases.
        |elems| Ok(Value::Scalar(std_of_slice(elems))),
        |m, dim| {
            matrix_axis_collect_then(m, dim, |slice| Complex::new(std_of_slice(slice), 0.0))
        },
    )
}

/// Parse the optional `dim` argument (1 or 2) used by matrix-axis reducers.
fn parse_reduction_dim(name: &str, args: &[Value]) -> Result<Option<usize>, ScriptError> {
    if args.len() < 2 {
        return Ok(None);
    }
    let d = args[1]
        .to_scalar()
        .map_err(|e| ScriptError::type_err(format!("{}: dim: {}", name, e)))?
        as i64;
    if d != 1 && d != 2 {
        return Err(ScriptError::type_err(format!(
            "{}: dim must be 1 or 2, got {}",
            name, d
        )));
    }
    Ok(Some(d as usize))
}

/// Wrap a complex sum/reduction result, collapsing imaginary noise to zero
/// when it's negligible.
fn complex_to_value(s: C64) -> Value {
    if s.im.abs() < 1e-12 {
        Value::Scalar(s.re)
    } else {
        Value::Complex(s)
    }
}

/// For each column (`dim=1`) or row (`dim=2`) of `m`, collect that
/// dimension's elements into a `Vec<C64>`, apply `f`, and place the
/// result in the corresponding cell of a `1×ncols` (`dim=1`) or
/// `nrows×1` (`dim=2`) output matrix. Used by every matrix-axis reducer
/// (`sum`, `mean`, `prod`, `median`, `std`) so the column/row iteration
/// + output-shape construction live in exactly one place.
fn matrix_axis_collect_then(
    m: &CMatrix,
    dim: usize,
    f: impl Fn(&[C64]) -> C64,
) -> CMatrix {
    let nrows = m.nrows();
    let ncols = m.ncols();
    match dim {
        1 => {
            let data: Vec<C64> = (0..ncols)
                .map(|j| {
                    let col: Vec<C64> = (0..nrows).map(|i| m[[i, j]]).collect();
                    f(&col)
                })
                .collect();
            Array2::from_shape_vec((1, ncols), data).expect("dim=1 shape is 1×ncols")
        }
        2 => {
            let data: Vec<C64> = (0..nrows)
                .map(|i| {
                    let row: Vec<C64> = (0..ncols).map(|j| m[[i, j]]).collect();
                    f(&row)
                })
                .collect();
            Array2::from_shape_vec((nrows, 1), data).expect("dim=2 shape is nrows×1")
        }
        _ => unreachable!("dim is always 1 or 2 by construction (parse_reduction_dim)"),
    }
}

/// Common dispatch envelope for matrix-axis reducers. Handles:
///
/// - `Value::Vector`: `dim=1` is identity (vector behaves like a 1×N row,
///   so reducing along the singleton row dim yields the same vector);
///   default and `dim=2` apply `flat_reduce` to all elements.
/// - `Value::Matrix`: empty matrix returns `Scalar(0.0)`; 1-D-shaped matrix
///   (`1×N` or `N×1`) is treated as a vector and `flat_reduce` is applied
///   over all elements; general `M×N` matrix dispatches to `axis_reduce`
///   along the chosen `dim` (default 1, the octave first-non-singleton-dim
///   rule for matrices with `M > 1`).
/// - `Value::Scalar` / `Value::Complex`: cloned through.
///
/// `flat_reduce` returns a `Value` directly because different reducers map
/// the flat-input case to different shapes (scalar for sum/mean/prod/median,
/// scalar for std, etc.). Returning `Result` lets a reducer reject empty
/// input cleanly (e.g. mean errors on empty per current behavior).
fn dispatch_matrix_reduction<FlatF, AxisF>(
    name: &str,
    args: &[Value],
    flat_reduce: FlatF,
    axis_reduce: AxisF,
) -> Result<Value, ScriptError>
where
    FlatF: Fn(&[C64]) -> Result<Value, ScriptError>,
    AxisF: Fn(&CMatrix, usize) -> CMatrix,
{
    check_args_range(name, args, 1, 2)?;
    let dim_arg = parse_reduction_dim(name, args)?;
    match &args[0] {
        Value::Vector(v) => match dim_arg {
            Some(1) => Ok(args[0].clone()),
            _ => {
                let elements: Vec<C64> = v.iter().copied().collect();
                flat_reduce(&elements)
            }
        },
        Value::Matrix(m) => {
            let nrows = m.nrows();
            let ncols = m.ncols();
            if nrows == 0 || ncols == 0 {
                return Ok(Value::Scalar(0.0));
            }
            if nrows == 1 || ncols == 1 {
                let elements: Vec<C64> = m.iter().copied().collect();
                return flat_reduce(&elements);
            }
            let dim = dim_arg.unwrap_or(1);
            Ok(Value::Matrix(axis_reduce(m, dim)))
        }
        Value::Scalar(s) => Ok(Value::Scalar(*s)),
        Value::Complex(c) => Ok(Value::Complex(*c)),
        other => Err(ScriptError::type_err(format!(
            "{}: unsupported type {}",
            name,
            other.type_name()
        ))),
    }
}

/// sum(v) — sum of all elements of a vector / 1-D-shaped matrix → scalar.
/// sum(M) — sum along dim 1 (columns) → row matrix `1×ncols` (octave default).
/// sum(M, 1) — explicit column sums.
/// sum(M, 2) — row sums → column matrix `nrows×1`.
fn builtin_sum(args: Vec<Value>) -> Result<Value, ScriptError> {
    dispatch_matrix_reduction(
        "sum",
        &args,
        |elems| Ok(complex_to_value(elems.iter().copied().sum())),
        |m, dim| matrix_axis_collect_then(m, dim, |slice| slice.iter().copied().sum()),
    )
}

/// prod(v) — product of all elements of a vector / 1-D-shaped matrix → scalar.
/// prod(M) — product along dim 1 (columns) → row matrix `1×ncols`.
/// prod(M, 1) / prod(M, 2) — explicit axis.
fn builtin_prod(args: Vec<Value>) -> Result<Value, ScriptError> {
    let one = Complex::new(1.0, 0.0);
    dispatch_matrix_reduction(
        "prod",
        &args,
        |elems| {
            if elems.is_empty() {
                return Err(ScriptError::type_err(
                    "prod: argument must be a non-empty vector, matrix, or scalar".to_string(),
                ));
            }
            Ok(complex_to_value(
                elems.iter().copied().fold(one, |acc, x| acc * x),
            ))
        },
        |m, dim| {
            matrix_axis_collect_then(m, dim, |slice| {
                slice.iter().copied().fold(one, |acc, x| acc * x)
            })
        },
    )
}

/// cumsum(v) — cumulative sum of a vector. Returns a vector of the same length.
/// cumsum(v) — running total of a vector / 1-D-shaped matrix → same-shape result.
/// cumsum(M) — running total along dim 1 (each column independently).
/// cumsum(M, 1) / cumsum(M, 2) — explicit axis.
fn builtin_cumsum(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("cumsum", &args, 1, 2)?;
    let dim_arg = parse_reduction_dim("cumsum", &args)?;
    match &args[0] {
        Value::Vector(v) => {
            // Vector behaves like a 1×N row matrix. dim=1 is a no-op (each
            // column has length 1, so cumsum is the identity).
            match dim_arg {
                Some(1) => Ok(args[0].clone()),
                _ => {
                    let mut acc = Complex::new(0.0, 0.0);
                    let result: CVector = Array1::from_iter(v.iter().map(|&x| {
                        acc += x;
                        acc
                    }));
                    Ok(Value::Vector(result))
                }
            }
        }
        Value::Matrix(m) => {
            let nrows = m.nrows();
            let ncols = m.ncols();
            if nrows == 0 || ncols == 0 {
                return Ok(Value::Matrix(m.clone()));
            }
            // 1-D-shaped matrix (column or row vector form): treat as a vector
            // and accumulate along the long axis. Preserve the input shape.
            if nrows == 1 || ncols == 1 {
                let mut acc = Complex::new(0.0, 0.0);
                let mut data: Vec<C64> = Vec::with_capacity(nrows * ncols);
                for &x in m.iter() {
                    acc += x;
                    data.push(acc);
                }
                return Ok(Value::Matrix(
                    Array2::from_shape_vec((nrows, ncols), data)
                        .map_err(|e| ScriptError::runtime(e.to_string()))?,
                ));
            }
            let dim = dim_arg.unwrap_or(1);
            let mut out = m.clone();
            match dim {
                1 => {
                    // Cumulative along columns (top to bottom).
                    for j in 0..ncols {
                        let mut acc = Complex::new(0.0, 0.0);
                        for i in 0..nrows {
                            acc += m[[i, j]];
                            out[[i, j]] = acc;
                        }
                    }
                }
                2 => {
                    // Cumulative along rows (left to right).
                    for i in 0..nrows {
                        let mut acc = Complex::new(0.0, 0.0);
                        for j in 0..ncols {
                            acc += m[[i, j]];
                            out[[i, j]] = acc;
                        }
                    }
                }
                _ => unreachable!(),
            }
            Ok(Value::Matrix(out))
        }
        Value::Scalar(n) => Ok(Value::Scalar(*n)),
        Value::Complex(c) => Ok(Value::Complex(*c)),
        other => Err(ScriptError::type_err(format!(
            "cumsum: unsupported type {}",
            other.type_name()
        ))),
    }
}

/// argmin(v) — 1-based index of the minimum element (by real part).
/// Per-column or per-row 1-based argmin/argmax for a 2-D matrix.
fn matrix_axis_argmin_argmax(
    m: &CMatrix,
    dim: usize,
    pick_better: fn(f64, f64) -> bool,
) -> Result<Value, ScriptError> {
    let nrows = m.nrows();
    let ncols = m.ncols();
    match dim {
        1 => {
            // Per-column index → row vector of length ncols.
            let mut best_val = vec![f64::NAN; ncols];
            let mut best_idx = vec![0usize; ncols];
            for ((i, j), x) in m.indexed_iter() {
                if best_val[j].is_nan() || pick_better(x.re, best_val[j]) {
                    best_val[j] = x.re;
                    best_idx[j] = i;
                }
            }
            let cdata: Vec<C64> = best_idx
                .into_iter()
                .map(|k| Complex::new((k + 1) as f64, 0.0))
                .collect();
            Ok(Value::Matrix(
                Array2::from_shape_vec((1, ncols), cdata)
                    .map_err(|e| ScriptError::runtime(e.to_string()))?,
            ))
        }
        2 => {
            // Per-row index → column vector of length nrows.
            let mut best_val = vec![f64::NAN; nrows];
            let mut best_idx = vec![0usize; nrows];
            for ((i, j), x) in m.indexed_iter() {
                if best_val[i].is_nan() || pick_better(x.re, best_val[i]) {
                    best_val[i] = x.re;
                    best_idx[i] = j;
                }
            }
            let cdata: Vec<C64> = best_idx
                .into_iter()
                .map(|k| Complex::new((k + 1) as f64, 0.0))
                .collect();
            Ok(Value::Matrix(
                Array2::from_shape_vec((nrows, 1), cdata)
                    .map_err(|e| ScriptError::runtime(e.to_string()))?,
            ))
        }
        _ => unreachable!(),
    }
}

fn builtin_argmin(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("argmin", &args, 1, 2)?;
    let dim_arg = parse_reduction_dim("argmin", &args)?;
    match &args[0] {
        Value::Vector(v) if !v.is_empty() => {
            // Vector behaves like a 1×N row matrix. dim=1 is a no-op (each
            // column has length 1, so the argmin is always row 1).
            match dim_arg {
                Some(1) => {
                    let n = v.len();
                    let cdata: Vec<C64> =
                        (0..n).map(|_| Complex::new(1.0, 0.0)).collect();
                    Ok(Value::Vector(Array1::from_vec(cdata)))
                }
                _ => {
                    let idx = v
                        .iter()
                        .enumerate()
                        .min_by(|(_, a), (_, b)| a.re.partial_cmp(&b.re).unwrap())
                        .map(|(i, _)| i)
                        .unwrap();
                    Ok(Value::Scalar((idx + 1) as f64))
                }
            }
        }
        Value::Matrix(m) if !m.is_empty() => {
            let nrows = m.nrows();
            let ncols = m.ncols();
            // 1-D-shaped: treat as a vector, return a scalar index.
            if nrows == 1 || ncols == 1 {
                let idx = m
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| a.re.partial_cmp(&b.re).unwrap())
                    .map(|(i, _)| i)
                    .unwrap();
                return Ok(Value::Scalar((idx + 1) as f64));
            }
            let dim = dim_arg.unwrap_or(1);
            matrix_axis_argmin_argmax(m, dim, |new, cur| new < cur)
        }
        Value::Scalar(_) => Ok(Value::Scalar(1.0)),
        _ => Err(ScriptError::type_err(
            "argmin: argument must be a non-empty vector, scalar, or matrix".to_string(),
        )),
    }
}

/// argmax(v) — 1-based index of the maximum element (by real part).
fn builtin_argmax(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("argmax", &args, 1, 2)?;
    let dim_arg = parse_reduction_dim("argmax", &args)?;
    match &args[0] {
        Value::Vector(v) if !v.is_empty() => match dim_arg {
            Some(1) => {
                let n = v.len();
                let cdata: Vec<C64> = (0..n).map(|_| Complex::new(1.0, 0.0)).collect();
                Ok(Value::Vector(Array1::from_vec(cdata)))
            }
            _ => {
                let idx = v
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.re.partial_cmp(&b.re).unwrap())
                    .map(|(i, _)| i)
                    .unwrap();
                Ok(Value::Scalar((idx + 1) as f64))
            }
        },
        Value::Matrix(m) if !m.is_empty() => {
            let nrows = m.nrows();
            let ncols = m.ncols();
            if nrows == 1 || ncols == 1 {
                let idx = m
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.re.partial_cmp(&b.re).unwrap())
                    .map(|(i, _)| i)
                    .unwrap();
                return Ok(Value::Scalar((idx + 1) as f64));
            }
            let dim = dim_arg.unwrap_or(1);
            matrix_axis_argmin_argmax(m, dim, |new, cur| new > cur)
        }
        Value::Scalar(_) => Ok(Value::Scalar(1.0)),
        _ => Err(ScriptError::type_err(
            "argmax: argument must be a non-empty vector, scalar, or matrix".to_string(),
        )),
    }
}

/// sort(v) — sort a vector ascending by real part; preserves imaginary components.
/// Nargout-aware dispatch for `sort`:
///   nargout == 1: returns the sorted output, same shape as input.
///   nargout >= 2: returns `[sorted, idx]` where `idx` is the 1-based
///                 permutation that produces `sorted` from the input —
///                 `sorted(k) == input(idx(k))` for vectors/1-D matrices.
fn builtin_sort_nargout(args: Vec<Value>, nargout: usize) -> Result<Value, ScriptError> {
    if nargout < 2 {
        return builtin_sort(args);
    }
    check_args_range("sort", &args, 1, 2)?;
    let descending = if args.len() == 2 {
        match &args[1] {
            Value::Str(s) => match s.as_str() {
                "ascend" => false,
                "descend" => true,
                other => {
                    return Err(ScriptError::type_err(format!(
                        "sort: direction must be \"ascend\" or \"descend\", got \"{}\"",
                        other
                    )))
                }
            },
            other => {
                return Err(ScriptError::type_err(format!(
                    "sort: 2nd argument must be a direction string, got {}",
                    other.type_name()
                )))
            }
        }
    } else {
        false
    };

    /// Sort a flat slice of complex values by real part, returning the sorted
    /// values plus the 1-based permutation indices.
    fn sort_with_indices(elements: &[C64], descending: bool) -> (Vec<C64>, Vec<f64>) {
        let mut paired: Vec<(usize, C64)> = elements.iter().copied().enumerate().collect();
        paired.sort_by(|(_, a), (_, b)| {
            let ord = a
                .re
                .partial_cmp(&b.re)
                .unwrap_or(std::cmp::Ordering::Equal);
            if descending {
                ord.reverse()
            } else {
                ord
            }
        });
        let sorted: Vec<C64> = paired.iter().map(|(_, v)| *v).collect();
        let indices: Vec<f64> = paired.iter().map(|(i, _)| (*i + 1) as f64).collect();
        (sorted, indices)
    }

    let (sorted, indices, original): (Value, Value, &Value) = match &args[0] {
        Value::Vector(v) => {
            let elements: Vec<C64> = v.iter().copied().collect();
            let (sorted, idx) = sort_with_indices(&elements, descending);
            (
                Value::Vector(Array1::from_vec(sorted)),
                Value::Vector(Array1::from_iter(
                    idx.into_iter().map(|f| Complex::new(f, 0.0)),
                )),
                &args[0],
            )
        }
        Value::Matrix(m) if m.nrows() == 1 || m.ncols() == 1 => {
            let nrows = m.nrows();
            let ncols = m.ncols();
            let elements: Vec<C64> = m.iter().copied().collect();
            let (sorted, idx) = sort_with_indices(&elements, descending);
            let s_mat = Array2::from_shape_fn((nrows, ncols), |(i, j)| sorted[i * ncols + j]);
            let i_mat = Array2::from_shape_fn((nrows, ncols), |(i, j)| {
                Complex::new(idx[i * ncols + j], 0.0)
            });
            (Value::Matrix(s_mat), Value::Matrix(i_mat), &args[0])
        }
        Value::Scalar(_) => {
            return Ok(Value::Tuple(vec![args[0].clone(), Value::Scalar(1.0)]));
        }
        _ => {
            return Err(ScriptError::type_err(
                "sort: argument must be a vector, scalar, or 1-D-shaped matrix"
                    .to_string(),
            ))
        }
    };
    let _ = original;
    Ok(Value::Tuple(vec![sorted, indices]))
}

fn builtin_sort(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("sort", &args, 1, 2)?;

    // Optional 2nd argument: "ascend" (default) or "descend" — matches the
    // octave/matlab string-flag form. The numeric-dim form `sort(M, dim)` is
    // not yet supported (matrix-axis reductions are a separate plan item).
    let descending = if args.len() == 2 {
        match &args[1] {
            Value::Str(s) => match s.as_str() {
                "ascend" => false,
                "descend" => true,
                other => {
                    return Err(ScriptError::type_err(format!(
                        "sort: direction must be \"ascend\" or \"descend\", got \"{}\"",
                        other
                    )))
                }
            },
            other => {
                return Err(ScriptError::type_err(format!(
                    "sort: 2nd argument must be a direction string \"ascend\" or \"descend\", got {}",
                    other.type_name()
                )))
            }
        }
    } else {
        false
    };

    let cmp_real = |a: &f64, b: &f64| {
        let ord = a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal);
        if descending { ord.reverse() } else { ord }
    };
    let cmp_cx = |a: &C64, b: &C64| {
        let ord = a.re.partial_cmp(&b.re).unwrap_or(std::cmp::Ordering::Equal);
        if descending { ord.reverse() } else { ord }
    };

    match &args[0] {
        Value::Vector(v) => {
            if is_real_vector(v) {
                let mut reals: Vec<f64> = v.iter().map(|c| c.re).collect();
                reals.sort_unstable_by(cmp_real);
                Ok(Value::Vector(Array1::from_iter(
                    reals.into_iter().map(|r| Complex::new(r, 0.0)),
                )))
            } else {
                let mut sorted: Vec<C64> = v.iter().copied().collect();
                sorted.sort_by(cmp_cx);
                Ok(Value::Vector(Array1::from_vec(sorted)))
            }
        }
        // Octave/matlab compat: a 1-D-shaped matrix (column vector or single
        // row) is treated as a vector by sort. The output keeps the same
        // shape so downstream code that depends on column-vs-row continues
        // to work.
        Value::Matrix(m) if m.nrows() == 1 || m.ncols() == 1 => {
            let nrows = m.nrows();
            let ncols = m.ncols();
            let mut elements: Vec<C64> = m.iter().copied().collect();
            let any_nonzero_im = elements.iter().any(|c| c.im.abs() > 1e-12);
            if !any_nonzero_im {
                let mut reals: Vec<f64> = elements.iter().map(|c| c.re).collect();
                reals.sort_unstable_by(cmp_real);
                elements = reals.into_iter().map(|r| Complex::new(r, 0.0)).collect();
            } else {
                elements.sort_by(cmp_cx);
            }
            let result = Array2::from_shape_fn((nrows, ncols), |(i, j)| {
                elements[i * ncols + j]
            });
            Ok(Value::Matrix(result))
        }
        Value::Scalar(_) => Ok(args[0].clone()),
        _ => Err(ScriptError::type_err(
            "sort: argument must be a vector, scalar, or 1-D-shaped matrix \
             (column vector or single row)"
                .to_string(),
        )),
    }
}

/// trapz(v) — trapezoidal integration with unit spacing.
/// trapz(x, v) — trapezoidal integration with x coordinates.
fn builtin_trapz(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("trapz", &args, 1, 2)?;
    let (x_opt, v) = if args.len() == 2 {
        let x = match &args[0] {
            Value::Vector(v) => v.iter().map(|c| c.re).collect::<Vec<f64>>(),
            other => {
                return Err(ScriptError::type_err(format!(
                    "trapz: x must be a vector, got {}",
                    other.type_name()
                )))
            }
        };
        let v = match &args[1] {
            Value::Vector(v) => v.clone(),
            other => {
                return Err(ScriptError::type_err(format!(
                    "trapz: v must be a vector, got {}",
                    other.type_name()
                )))
            }
        };
        (Some(x), v)
    } else {
        let v = match &args[0] {
            Value::Vector(v) => v.clone(),
            other => {
                return Err(ScriptError::type_err(format!(
                    "trapz: argument must be a vector, got {}",
                    other.type_name()
                )))
            }
        };
        (None, v)
    };
    if v.len() < 2 {
        return Ok(Value::Scalar(0.0));
    }
    let s: C64 = (0..v.len() - 1)
        .map(|i| {
            let dx = match &x_opt {
                Some(x) => x[i + 1] - x[i],
                None => 1.0,
            };
            (v[i] + v[i + 1]) * 0.5 * dx
        })
        .sum();
    if s.im.abs() < 1e-12 {
        Ok(Value::Scalar(s.re))
    } else {
        Ok(Value::Complex(s))
    }
}

fn builtin_len(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("len", &args, 1)?;
    match &args[0] {
        Value::Vector(v) => Ok(Value::Scalar(v.len() as f64)),
        // Octave/matlab convention: length(M) = max(size(M)). For column or
        // row vectors this gives the natural length; for general matrices
        // it gives the longest dimension. Use numel(M) for total element
        // count and size(M) for full shape information.
        Value::Matrix(m) => Ok(Value::Scalar(m.nrows().max(m.ncols()) as f64)),
        Value::Str(s) => Ok(Value::Scalar(s.len() as f64)),
        Value::Tuple(t) => Ok(Value::Scalar(t.len() as f64)),
        Value::StringArray(v) => Ok(Value::Scalar(v.len() as f64)),
        other => Err(ScriptError::type_err(format!(
            "len: unsupported type {}",
            other
        ))),
    }
}

fn builtin_numel(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("numel", &args, 1)?;
    let n = match &args[0] {
        Value::Vector(v) => v.len(),
        Value::Matrix(m) => m.nrows() * m.ncols(),
        Value::Tensor3(t) => t.shape().iter().product::<usize>(),
        Value::Scalar(_) | Value::Complex(_) => 1,
        Value::StringArray(v) => v.len(),
        other => {
            return Err(ScriptError::type_err(format!(
                "numel: unsupported type {}",
                other
            )))
        }
    };
    Ok(Value::Scalar(n as f64))
}

fn builtin_size(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("size", &args, 1, 2)?;
    // Tensor3 carries 3 dimensions; everything else is treated as (rows, cols).
    if let Value::Tensor3(t) = &args[0] {
        let s = t.shape();
        let (m, n, p) = (s[0], s[1], s[2]);
        if args.len() == 2 {
            let dim = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
            return match dim {
                1 => Ok(Value::Scalar(m as f64)),
                2 => Ok(Value::Scalar(n as f64)),
                3 => Ok(Value::Scalar(p as f64)),
                _ => Err(ScriptError::type_err(format!(
                    "size: dim must be 1, 2, or 3 for tensor3, got {}",
                    dim
                ))),
            };
        }
        return Ok(Value::Vector(Array1::from_vec(vec![
            Complex::new(m as f64, 0.0),
            Complex::new(n as f64, 0.0),
            Complex::new(p as f64, 0.0),
        ])));
    }
    let (nrows, ncols) = match &args[0] {
        Value::Vector(v) => (1usize, v.len()),
        Value::Matrix(m) => (m.nrows(), m.ncols()),
        Value::Scalar(_) | Value::Complex(_) => (1, 1),
        Value::SparseVector(sv) => (1, sv.len),
        Value::SparseMatrix(sm) => (sm.rows, sm.cols),
        Value::StringArray(v) => (1, v.len()),
        other => {
            return Err(ScriptError::type_err(format!(
                "size: unsupported type {}",
                other.type_name()
            )))
        }
    };
    if args.len() == 2 {
        let dim = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
        match dim {
            1 => Ok(Value::Scalar(nrows as f64)),
            2 => Ok(Value::Scalar(ncols as f64)),
            _ => Err(ScriptError::type_err(format!(
                "size: dim must be 1 or 2, got {}",
                dim
            ))),
        }
    } else {
        Ok(Value::Vector(Array1::from_vec(vec![
            Complex::new(nrows as f64, 0.0),
            Complex::new(ncols as f64, 0.0),
        ])))
    }
}

/// ndims(A) — number of dimensions. Scalars/vectors/matrices report 2 (Octave
/// convention: even a scalar has ndims=2, conceptually 1×1). Tensor3 reports 3.
fn builtin_ndims(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("ndims", &args, 1)?;
    let n = match &args[0] {
        Value::Tensor3(_) => 3,
        Value::Scalar(_)
        | Value::Complex(_)
        | Value::Vector(_)
        | Value::Matrix(_)
        | Value::SparseVector(_)
        | Value::SparseMatrix(_)
        | Value::StringArray(_)
        | Value::Bool(_)
        | Value::Str(_) => 2,
        other => {
            return Err(ScriptError::type_err(format!(
                "ndims: unsupported type {}",
                other.type_name()
            )))
        }
    };
    Ok(Value::Scalar(n as f64))
}

// ─── FFT builtins ──────────────────────────────────────────────────────────

fn builtin_fft(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fft", &args, 1)?;
    let v = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let result = fft(&v).map_err(ScriptError::Dsp)?;
    Ok(Value::Vector(result))
}

fn builtin_ifft(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("ifft", &args, 1)?;
    let v = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let result = ifft(&v).map_err(ScriptError::Dsp)?;
    Ok(Value::Vector(result))
}

fn builtin_fftshift(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fftshift", &args, 1)?;
    let v = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    Ok(Value::Vector(fftshift(&v)))
}

fn builtin_fftfreq(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fftfreq", &args, 2)?;
    let n = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let freqs = fftfreq(n, sr);
    let cv: CVector = Array1::from_iter(freqs.iter().map(|&f| Complex::new(f, 0.0)));
    Ok(Value::Vector(cv))
}

fn builtin_spectrum(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("spectrum", &args, 2)?;
    let x = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let n = x.len();
    if n == 0 {
        return Err(ScriptError::type_err(
            "spectrum: input vector is empty".to_string(),
        ));
    }
    // DC-centered spectrum via fftshift
    let xs = fftshift(&x);
    // DC-centered frequency axis: same rotation as fftshift
    let raw_freqs: Vec<f64> = fftfreq(n, sr).to_vec();
    let split = (n + 1) / 2;
    let shifted_freqs: Vec<f64> = raw_freqs[split..]
        .iter()
        .chain(raw_freqs[..split].iter())
        .copied()
        .collect();
    // Pack into 2×n matrix (row 0 = Hz axis, row 1 = complex spectrum)
    use ndarray::Array2;
    let mut m = Array2::zeros((2, n));
    for i in 0..n {
        m[(0, i)] = Complex::new(shifted_freqs[i], 0.0);
        m[(1, i)] = xs[i];
    }
    Ok(Value::Matrix(m))
}

// ─── Kaiser FIR builtins ───────────────────────────────────────────────────

fn builtin_fir_lowpass_kaiser(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fir_lowpass_kaiser", &args, 4)?;
    let cutoff = args[0].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let tbw = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let attn = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[3].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let filter = fir_lowpass_kaiser(cutoff, tbw, attn, sr)?;
    Ok(cvector_to_value(filter.coefficients))
}

fn builtin_fir_highpass_kaiser(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fir_highpass_kaiser", &args, 4)?;
    let cutoff = args[0].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let tbw = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let attn = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[3].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let filter = fir_highpass_kaiser(cutoff, tbw, attn, sr)?;
    Ok(cvector_to_value(filter.coefficients))
}

fn builtin_fir_bandpass_kaiser(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fir_bandpass_kaiser", &args, 5)?;
    let low = args[0].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let high = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let tbw = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let attn = args[3].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[4].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let filter = fir_bandpass_kaiser(low, high, tbw, attn, sr)?;
    Ok(cvector_to_value(filter.coefficients))
}

fn builtin_fir_notch(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fir_notch", &args, 5)?;
    let center = args[0].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let bw = args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let taps = args[3].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let win = parse_window(&args[4])?;
    let filter = fir_notch(center, bw, sr, taps, win)?;
    Ok(cvector_to_value(filter.coefficients))
}

fn builtin_freqz(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("freqz", &args, 3)?;
    let h = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let n = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let sr = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let (freqs, h_out) = freqz(&h, n, sr).map_err(ScriptError::Dsp)?;
    // Return as 2×n matrix: row 0 = freq axis (real), row 1 = complex response
    use ndarray::Array2;
    let mut mat: ndarray::Array2<rustlab_core::C64> = Array2::zeros((2, n));
    for k in 0..n {
        mat[[0, k]] = Complex::new(freqs[k], 0.0);
        mat[[1, k]] = h_out[k];
    }
    Ok(Value::Matrix(mat))
}

// ─── Parks-McClellan FIR builtins ─────────────────────────────────────────

/// firpm(n_taps, bands, desired)  or  firpm(n_taps, bands, desired, weights)
///
/// bands and desired are vectors of normalized frequencies in [0,1] (1 = Nyquist).
/// weights is an optional vector with one value per band pair.
fn builtin_firpm(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() < 3 || args.len() > 4 {
        return Err(ScriptError::arg_count("firpm".into(), 3, args.len()));
    }
    let n_taps = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let bands = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let desired = args[2].to_cvector().map_err(|e| ScriptError::type_err(e))?;

    let bands_f: Vec<f64> = bands.iter().map(|c| c.re).collect();
    let desired_f: Vec<f64> = desired.iter().map(|c| c.re).collect();

    let weights_f: Vec<f64> = if args.len() == 4 {
        let w = args[3].to_cvector().map_err(|e| ScriptError::type_err(e))?;
        w.iter().map(|c| c.re).collect()
    } else {
        vec![]
    };

    let filter = firpm(n_taps, &bands_f, &desired_f, &weights_f).map_err(ScriptError::Dsp)?;
    Ok(cvector_to_value(filter.coefficients))
}

/// firpmq(n_taps, bands, desired [, weights [, bits [, n_iter]]])
///
/// Design an integer-coefficient equiripple FIR filter.
/// Defaults: weights = uniform, bits = 16, n_iter = 8.
///
/// Returns integer-valued coefficients (e.g. 127.0, -512.0).
/// Divide by (2^(bits-1) - 1) to normalize to unit gain for freqz.
fn builtin_firpmq(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() < 3 || args.len() > 6 {
        return Err(ScriptError::runtime(
            "firpmq: expected 3–6 arguments: (n_taps, bands, desired [, weights [, bits [, n_iter]]])".into()
        ));
    }
    let n_taps = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let bands = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let desired = args[2].to_cvector().map_err(|e| ScriptError::type_err(e))?;

    let bands_f: Vec<f64> = bands.iter().map(|c| c.re).collect();
    let desired_f: Vec<f64> = desired.iter().map(|c| c.re).collect();

    let weights_f: Vec<f64> = if args.len() >= 4 {
        let w = args[3].to_cvector().map_err(|e| ScriptError::type_err(e))?;
        w.iter().map(|c| c.re).collect()
    } else {
        vec![]
    };

    let bits = if args.len() >= 5 {
        args[4].to_usize().map_err(|e| ScriptError::type_err(e))? as u32
    } else {
        16
    };

    let n_iter = if args.len() >= 6 {
        args[5].to_usize().map_err(|e| ScriptError::type_err(e))?
    } else {
        8
    };

    let filter =
        firpmq(n_taps, &bands_f, &desired_f, &weights_f, bits, n_iter).map_err(ScriptError::Dsp)?;
    Ok(cvector_to_value(filter.coefficients))
}

// ─── Fixed-point quantization builtins ────────────────────────────────────

/// Parse a round-mode string, returning a ScriptError on failure.
fn parse_round_mode(s: &str) -> Result<RoundMode, ScriptError> {
    RoundMode::from_str(s).ok_or_else(|| {
        ScriptError::runtime(format!(
            "unknown rounding mode '{s}'; valid: floor, ceil, zero, round, round_even"
        ))
    })
}

/// Parse an overflow-mode string.
fn parse_overflow_mode(s: &str) -> Result<OverflowMode, ScriptError> {
    OverflowMode::from_str(s).ok_or_else(|| {
        ScriptError::runtime(format!(
            "unknown overflow mode '{s}'; valid: saturate, wrap"
        ))
    })
}

/// qfmt(word_bits, frac_bits)
/// qfmt(word_bits, frac_bits, round_mode)
/// qfmt(word_bits, frac_bits, round_mode, overflow_mode)
fn builtin_qfmt(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() < 2 || args.len() > 4 {
        return Err(ScriptError::arg_count("qfmt".into(), 2, args.len()));
    }
    let word = args[0].to_usize().map_err(|e| ScriptError::type_err(e))? as u8;
    let frac = args[1].to_usize().map_err(|e| ScriptError::type_err(e))? as u8;
    let round = if args.len() >= 3 {
        parse_round_mode(&args[2].to_str().map_err(|e| ScriptError::type_err(e))?)?
    } else {
        RoundMode::Floor
    };
    let overflow = if args.len() == 4 {
        parse_overflow_mode(&args[3].to_str().map_err(|e| ScriptError::type_err(e))?)?
    } else {
        OverflowMode::Saturate
    };
    let spec = QFmtSpec::new(word, frac, round, overflow).map_err(ScriptError::Dsp)?;
    Ok(Value::QFmt(spec))
}

/// quantize(x, fmt)  — snap every element of x to the Q grid defined by fmt.
/// Works on scalars, complex, vectors, and matrices (real/imag quantized independently).
fn builtin_quantize(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("quantize", &args, 2)?;
    let spec = args[1].to_qfmt().map_err(|e| ScriptError::type_err(e))?;

    match &args[0] {
        Value::Scalar(n) => Ok(Value::Scalar(quantize_scalar(*n, &spec))),
        Value::Complex(c) => Ok(Value::Complex(Complex::new(
            quantize_scalar(c.re, &spec),
            quantize_scalar(c.im, &spec),
        ))),
        Value::Vector(v) => {
            let re: Vec<f64> = v.iter().map(|c| quantize_scalar(c.re, &spec)).collect();
            let im: Vec<f64> = v.iter().map(|c| quantize_scalar(c.im, &spec)).collect();
            Ok(Value::Vector(Array1::from_iter(
                re.iter().zip(im.iter()).map(|(&r, &i)| Complex::new(r, i)),
            )))
        }
        Value::Matrix(m) => {
            let rows = m.nrows();
            let cols = m.ncols();
            let data: Vec<_> = m
                .iter()
                .map(|&c| Complex::new(quantize_scalar(c.re, &spec), quantize_scalar(c.im, &spec)))
                .collect();
            Ok(Value::Matrix(
                Array2::from_shape_vec((rows, cols), data)
                    .map_err(|e| ScriptError::runtime(e.to_string()))?,
            ))
        }
        other => Err(ScriptError::type_err(format!(
            "quantize: cannot quantize {}",
            other.type_name()
        ))),
    }
}

/// Extract a real f64 vector from a Value (scalar broadcast, vector, or real matrix row).
fn to_real_vec(v: &Value, name: &str) -> Result<Vec<f64>, ScriptError> {
    match v {
        Value::Scalar(n) => Ok(vec![*n]),
        Value::Vector(v) => Ok(v.iter().map(|c| c.re).collect()),
        other => Err(ScriptError::type_err(format!(
            "{name}: expected real scalar or vector, got {}",
            other.type_name()
        ))),
    }
}

/// qadd(a, b, fmt)  — element-wise add then quantize to fmt.
fn builtin_qadd(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("qadd", &args, 3)?;
    let a = to_real_vec(&args[0], "qadd")?;
    let b = to_real_vec(&args[1], "qadd")?;
    let spec = args[2].to_qfmt().map_err(|e| ScriptError::type_err(e))?;
    let y = fixed_qadd(&a, &b, &spec).map_err(ScriptError::Dsp)?;
    Ok(cvector_to_value(Array1::from_iter(
        y.iter().map(|&v| Complex::new(v, 0.0)),
    )))
}

/// qmul(a, b, fmt)  — element-wise multiply then quantize to fmt.
fn builtin_qmul(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("qmul", &args, 3)?;
    let a = to_real_vec(&args[0], "qmul")?;
    let b = to_real_vec(&args[1], "qmul")?;
    let spec = args[2].to_qfmt().map_err(|e| ScriptError::type_err(e))?;
    let y = fixed_qmul(&a, &b, &spec).map_err(ScriptError::Dsp)?;
    Ok(cvector_to_value(Array1::from_iter(
        y.iter().map(|&v| Complex::new(v, 0.0)),
    )))
}

/// qconv(x, h, fmt)  — fixed-point FIR convolution, output quantized to fmt.
fn builtin_qconv(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("qconv", &args, 3)?;
    let x = to_real_vec(&args[0], "qconv")?;
    let h = to_real_vec(&args[1], "qconv")?;
    let spec = args[2].to_qfmt().map_err(|e| ScriptError::type_err(e))?;
    let y = fixed_qconv(&x, &h, &spec);
    Ok(cvector_to_value(Array1::from_iter(
        y.iter().map(|&v| Complex::new(v, 0.0)),
    )))
}

/// snr(x_ref, x_quantized)  — signal-to-noise ratio in dB.
fn builtin_snr(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("snr", &args, 2)?;
    let x_ref = to_real_vec(&args[0], "snr")?;
    let x_q = to_real_vec(&args[1], "snr")?;
    let db = snr_db(&x_ref, &x_q).map_err(ScriptError::Dsp)?;
    Ok(Value::Scalar(db))
}

// ─── I/O builtins ──────────────────────────────────────────────────────────

fn builtin_print(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("print", &args, 0, 16)?;
    let mut out = String::new();
    for (i, v) in args.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{}", v));
    }
    super::output::script_println(&out);
    Ok(Value::None)
}

/// Check if a CVector is "real" (all imaginary parts < 1e-10)
fn is_real_vector(v: &CVector) -> bool {
    v.iter().all(|c| c.im.abs() < 1e-10)
}

// ─── Plot options helper ────────────────────────────────────────────────────

struct PlotOpts {
    color: Option<SeriesColor>,
    label: Option<String>,
    style: LineStyle,
}

impl Default for PlotOpts {
    fn default() -> Self {
        Self {
            color: None,
            label: None,
            style: LineStyle::Solid,
        }
    }
}

/// Parse trailing key-value string pairs from args slice.
/// Returns (opts, number_of_args_consumed).
fn parse_plot_opts(args: &[Value]) -> PlotOpts {
    let mut opts = PlotOpts::default();
    let mut i = 0;
    while i + 1 < args.len() {
        if let (Ok(k), Ok(v)) = (args[i].to_str(), args[i + 1].to_str()) {
            match k.to_lowercase().as_str() {
                "color" | "colour" => {
                    opts.color = SeriesColor::parse(&v);
                    i += 2;
                }
                "label" => {
                    opts.label = Some(v);
                    i += 2;
                }
                "style" => {
                    opts.style = if v.to_lowercase() == "dashed" {
                        LineStyle::Dashed
                    } else {
                        LineStyle::Solid
                    };
                    i += 2;
                }
                _ => break,
            }
        } else {
            break;
        }
    }
    opts
}

// ─── plot builtin ──────────────────────────────────────────────────────────

fn builtin_plot(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() {
        return Err(ScriptError::arg_count("plot".to_string(), 1, 0));
    }
    let mut args = args;
    flatten_column_matrix_args(&mut args);

    // Determine if first two args are both data (x, y) or single data + options
    let (x_opt, y_val, opts_start) = match (&args[0], args.get(1)) {
        (Value::Vector(_) | Value::Matrix(_), Some(Value::Vector(_) | Value::Matrix(_))) => {
            // plot(x, y, ...) or plot(x, M, ...)
            (Some(&args[0]), &args[1], 2)
        }
        _ => {
            // plot(v, ...) or plot(M, ...)
            (None, &args[0], 1)
        }
    };

    let opts = parse_plot_opts(&args[opts_start..]);
    let label = opts.label.as_deref().unwrap_or("").to_string();

    // Title: check if last remaining string arg is not a key-value pair
    let title = {
        let rem = &args[opts_start..];
        if rem.len() == 1 {
            if let Ok(s) = rem[0].to_str() {
                s
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    };

    match y_val {
        Value::Matrix(m) => {
            // Each column is a series
            let x_data: Vec<f64> = if let Some(Value::Vector(xv)) = x_opt {
                xv.iter().map(|c| c.re).collect()
            } else {
                (0..m.nrows()).map(|i| i as f64).collect()
            };
            let ncols = m.ncols();
            for col in 0..ncols {
                let y_data: Vec<f64> = m.column(col).iter().map(|c| c.re).collect();
                let col_label = if label.is_empty() {
                    format!("col{}", col + 1)
                } else {
                    label.clone()
                };
                let col_color = opts.color; // all columns same color if specified, else cycle
                push_xy_line(
                    x_data.clone(),
                    y_data,
                    &col_label,
                    &title,
                    col_color,
                    opts.style.clone(),
                );
            }
            render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))
        }
        Value::Vector(v) => {
            let x_data: Vec<f64> = if let Some(Value::Vector(xv)) = x_opt {
                xv.iter().map(|c| c.re).collect()
            } else {
                (0..v.len()).map(|i| i as f64).collect()
            };
            if is_real_vector(v) {
                let y_data: Vec<f64> = v.iter().map(|c| c.re).collect();
                let lbl = if label.is_empty() {
                    "value"
                } else {
                    label.as_str()
                };
                push_xy_line(x_data, y_data, lbl, &title, opts.color, opts.style);
                render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))
            } else {
                // Complex: push magnitude + real
                FIGURE.with(|fig| {
                    let mut fig = fig.borrow_mut();
                    if !fig.hold {
                        fig.current_mut().series.clear();
                    }
                    let sp = fig.current_mut();
                    if !title.is_empty() && sp.title.is_empty() {
                        sp.title = title.clone();
                    }
                    let mag_color = opts.color.unwrap_or(SeriesColor::Blue);
                    sp.series.push(rustlab_plot::Series {
                        label: "magnitude".to_string(),
                        x_data: x_data.clone(),
                        y_data: v.iter().map(|c| c.norm()).collect(),
                        color: mag_color,
                        style: opts.style.clone(),
                        kind: rustlab_plot::PlotKind::Line,
                    });
                    sp.series.push(rustlab_plot::Series {
                        label: "real".to_string(),
                        x_data,
                        y_data: v.iter().map(|c| c.re).collect(),
                        color: SeriesColor::Green,
                        style: opts.style.clone(),
                        kind: rustlab_plot::PlotKind::Line,
                    });
                });
                render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))
            }
        }
        Value::Scalar(n) => {
            let x_data = vec![0.0f64];
            let y_data = vec![*n];
            push_xy_line(x_data, y_data, "value", &title, opts.color, opts.style);
            render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))
        }
        other => Err(ScriptError::type_err(format!(
            "plot: cannot plot {}",
            other
        ))),
    }?;
    sync_figure_outputs();
    Ok(Value::None)
}

fn builtin_stem(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() {
        return Err(ScriptError::arg_count("stem".to_string(), 1, 0));
    }
    let mut args = args;
    flatten_column_matrix_args(&mut args);

    let (x_opt, y_val, opts_start) = match (&args[0], args.get(1)) {
        (Value::Vector(_), Some(Value::Vector(_))) => (Some(&args[0]), &args[1], 2),
        _ => (None, &args[0], 1),
    };

    let opts = parse_plot_opts(&args[opts_start..]);
    let label = opts.label.as_deref().unwrap_or("stem").to_string();
    let title = {
        let rem = &args[opts_start..];
        if rem.len() == 1 {
            if let Ok(s) = rem[0].to_str() {
                s
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    };

    match y_val {
        Value::Vector(v) => {
            let x_data: Vec<f64> = if let Some(Value::Vector(xv)) = x_opt {
                xv.iter().map(|c| c.re).collect()
            } else {
                (0..v.len()).map(|i| i as f64).collect()
            };
            let y_data: Vec<f64> = v.iter().map(|c| c.re).collect();
            push_xy_stem(x_data, y_data, &label, &title, opts.color);
        }
        Value::Scalar(n) => {
            push_xy_stem(vec![0.0], vec![*n], &label, &title, opts.color);
        }
        other => {
            return Err(ScriptError::type_err(format!(
                "stem: cannot plot {}",
                other
            )))
        }
    }
    render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();
    Ok(Value::None)
}

fn builtin_plotdb(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("plotdb", &args, 1, 2)?;
    let title = if args.len() == 2 {
        args[1].to_str().map_err(|e| ScriptError::type_err(e))?
    } else {
        "Frequency Response".to_string()
    };
    let (freqs, h) = extract_freq_response(&args[0])?;
    plot_db(&freqs, &h, &title).map_err(|e| ScriptError::runtime(e.to_string()))?;
    Ok(Value::None)
}

fn builtin_savefig(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("savefig", &args, 1)?;
    let path = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;
    render_figure_file(&path).map_err(|e| ScriptError::runtime(e.to_string()))?;
    Ok(Value::None)
}

/// frame() — snapshot the current figure into the animation frame buffer
/// and clear trace data while preserving subplot layout, axes, titles,
/// limits, hold, and grid. Pair with saveanim() to flush.
fn builtin_frame(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("frame", &args, 0)?;
    rustlab_plot::push_frame();
    rustlab_plot::clear_figure_traces();
    Ok(Value::None)
}

/// saveanim(path) / saveanim(path, fps) — flush the animation frame buffer
/// to disk. The path extension picks the format: `.html` / `.htm` →
/// self-contained Plotly animation; `.gif` → animated GIF (per-frame
/// NeuQuant palette). Buffer is cleared on success. `fps` defaults to 10.
fn builtin_saveanim(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("saveanim", &args, 1, 2)?;
    let path = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;
    let fps = if args.len() == 2 {
        match &args[1] {
            Value::Scalar(n) => *n,
            other => {
                return Err(ScriptError::type_err(format!(
                    "saveanim: fps must be a scalar, got {}",
                    other
                )));
            }
        }
    } else {
        10.0
    };
    let lower = path.to_lowercase();
    let format = if lower.ends_with(".html") || lower.ends_with(".htm") {
        rustlab_plot::NotebookAnimationFormat::Html
    } else if lower.ends_with(".gif") {
        rustlab_plot::NotebookAnimationFormat::Gif
    } else {
        return Err(ScriptError::runtime(
            "saveanim: supported output formats are .html / .htm (Plotly) and .gif"
                .to_string(),
        ));
    };
    if rustlab_plot::frames_len() == 0 {
        return Err(ScriptError::runtime(
            "saveanim: no frames captured (call frame() at least once)".to_string(),
        ));
    }
    // Notebook mode: capture for the renderer instead of writing the user
    // path (which the notebook executor has CWD'd to the source dir, not
    // the gallery output dir). The renderer writes the per-format file and
    // emits the right embed for each output format.
    if rustlab_plot::plot_context() == rustlab_plot::PlotContext::Notebook {
        rustlab_plot::push_notebook_animation_snapshot(fps, format);
        return Ok(Value::None);
    }
    match format {
        rustlab_plot::NotebookAnimationFormat::Html => {
            rustlab_plot::render_animation_html(&path, fps)
                .map_err(|e| ScriptError::runtime(e.to_string()))?;
        }
        rustlab_plot::NotebookAnimationFormat::Gif => {
            rustlab_plot::render_animation_gif(&path, fps)
                .map_err(|e| ScriptError::runtime(e.to_string()))?;
        }
    }
    Ok(Value::None)
}

// ─── Figure state builtins ─────────────────────────────────────────────────

/// figure()           — create a new figure, return its numeric handle.
/// figure(N)          — switch to figure N (create if it doesn't exist).
/// figure("file.html") — create a new figure in HTML output mode.
fn builtin_figure(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("figure", &args, 0, 1)?;

    if args.len() == 1 {
        // Numeric arg → switch to existing figure (or create it)
        if let Value::Scalar(n) = &args[0] {
            let id = *n as u32;
            rustlab_plot::figure_switch(id).map_err(|e| ScriptError::runtime(e.to_string()))?;
            return Ok(Value::Scalar(id as f64));
        }
        // String arg → new HTML figure
        let path = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;
        let id = rustlab_plot::figure_new_html(&path);
        eprintln!("HTML figure active: {}", path);
        return Ok(Value::Scalar(id as f64));
    }

    // No args → new TUI/viewer figure
    let id = rustlab_plot::figure_new();
    Ok(Value::Scalar(id as f64))
}

/// hold("on"|1) / hold("off"|0) — set hold on/off.
fn builtin_hold(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("hold", &args, 1)?;
    let on = match &args[0] {
        Value::Scalar(n) => *n != 0.0,
        _ => {
            let s = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;
            s.to_lowercase() == "on" || s == "1"
        }
    };
    FIGURE.with(|fig| fig.borrow_mut().hold = on);
    sync_figure_outputs();
    Ok(Value::None)
}

/// hline(y) / hline(y, "color") / hline(y, "color", "label")
/// Draw a horizontal reference line at y value(s).
fn builtin_hline(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() || args.len() > 3 {
        return Err(ScriptError::type_err(
            "hline: expected hline(y), hline(y, color), or hline(y, color, label)".to_string(),
        ));
    }
    let y_vals: Vec<f64> = match &args[0] {
        Value::Scalar(n) => vec![*n],
        Value::Vector(v) => v.iter().map(|c| c.re).collect(),
        other => {
            return Err(ScriptError::type_err(format!(
                "hline: expected scalar or vector, got {}",
                other
            )))
        }
    };
    let color = if args.len() >= 2 {
        let s = args[1].to_str().map_err(|e| ScriptError::type_err(e))?;
        SeriesColor::parse(&s)
    } else {
        None
    };
    let label = if args.len() >= 3 {
        args[2].to_str().map_err(|e| ScriptError::type_err(e))?
    } else {
        String::new()
    };
    // Use a wide x-range; the renderers clip to the subplot's xlim
    let x_span = FIGURE.with(|fig| {
        let fig = fig.borrow();
        let sp = fig.current();
        if sp.series.is_empty() {
            (-1e6_f64, 1e6_f64)
        } else {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for s in &sp.series {
                for &x in &s.x_data {
                    if x < lo {
                        lo = x;
                    }
                    if x > hi {
                        hi = x;
                    }
                }
            }
            let margin = (hi - lo).abs() * 0.1;
            (lo - margin, hi + margin)
        }
    });
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            fig.current_mut().series.clear();
            fig.current_mut().title.clear();
        }
        for &y in &y_vals {
            let c = color.unwrap_or_else(|| fig.next_color());
            let lbl = if label.is_empty() {
                format!("y={}", y)
            } else {
                label.clone()
            };
            let sp = fig.current_mut();
            sp.series.push(rustlab_plot::Series {
                label: lbl,
                x_data: vec![x_span.0, x_span.1],
                y_data: vec![y, y],
                color: c,
                style: LineStyle::Dashed,
                kind: rustlab_plot::PlotKind::Line,
            });
        }
    });
    render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();
    Ok(Value::None)
}

/// grid("on"|1) / grid("off"|0) — enable/disable grid on current subplot.
fn builtin_grid(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("grid", &args, 1)?;
    let on = match &args[0] {
        Value::Scalar(n) => *n != 0.0,
        _ => {
            let s = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;
            s.to_lowercase() == "on" || s == "1"
        }
    };
    FIGURE.with(|fig| fig.borrow_mut().current_mut().grid = on);
    sync_figure_outputs();
    Ok(Value::None)
}

/// xlabel("text") — set x-axis label on current subplot.
fn builtin_xlabel(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("xlabel", &args, 1)?;
    let label = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;
    FIGURE.with(|fig| fig.borrow_mut().current_mut().xlabel = label);
    sync_figure_outputs();
    Ok(Value::None)
}

/// ylabel("text") — set y-axis label on current subplot.
fn builtin_ylabel(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("ylabel", &args, 1)?;
    let label = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;
    FIGURE.with(|fig| fig.borrow_mut().current_mut().ylabel = label);
    sync_figure_outputs();
    Ok(Value::None)
}

// ── Log-axis and polar plots (em_requests §2.7 — pretransform shims) ──

fn vector_arg<'a>(args: &'a [Value], idx: usize, name: &str, role: &str) -> Result<&'a CVector, ScriptError> {
    match args.get(idx) {
        Some(Value::Vector(v)) => Ok(v),
        Some(other) => Err(ScriptError::type_err(format!(
            "{name}: {role} must be a vector, got {}",
            other.type_name()
        ))),
        None => Err(ScriptError::type_err(format!(
            "{name}: missing {role} argument"
        ))),
    }
}

/// Element-wise transform of a real-valued `Vector` via a closure.
/// Returns a new `Value::Vector` with imag part zeroed (the inputs are
/// asserted real-positive elsewhere; the transform is real-valued).
fn vector_transform(
    name: &str,
    role: &str,
    v: &CVector,
    f: impl Fn(f64) -> f64,
) -> Result<CVector, ScriptError> {
    let out: CVector = v
        .iter()
        .map(|c| {
            if c.im.abs() > 1e-12 {
                Err(ScriptError::type_err(format!(
                    "{name}: {role} must be real-valued (got nontrivial imag part)"
                )))
            } else {
                Ok(Complex::new(f(c.re), 0.0))
            }
        })
        .collect::<Result<Vec<_>, _>>()?
        .into();
    Ok(out)
}

fn assert_strictly_positive(name: &str, role: &str, v: &CVector) -> Result<(), ScriptError> {
    for c in v.iter() {
        if c.re <= 0.0 || !c.re.is_finite() {
            return Err(ScriptError::type_err(format!(
                "{name}: {role} must be strictly positive (got {})",
                c.re
            )));
        }
    }
    Ok(())
}

/// `loglog(x, y [, opts])` — log-log plot. Pre-transforms both axes
/// via `log10` and labels the axes with the `log10(...)` convention.
/// `x` and `y` must be strictly positive.
fn builtin_loglog(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("loglog", &args, 2, 8)?;
    let x = vector_arg(&args, 0, "loglog", "x")?;
    let y = vector_arg(&args, 1, "loglog", "y")?;
    assert_strictly_positive("loglog", "x", x)?;
    assert_strictly_positive("loglog", "y", y)?;
    let lx = vector_transform("loglog", "x", x, |v| v.log10())?;
    let ly = vector_transform("loglog", "y", y, |v| v.log10())?;

    let mut new_args: Vec<Value> = vec![Value::Vector(lx), Value::Vector(ly)];
    new_args.extend(args.into_iter().skip(2));
    let r = builtin_plot(new_args)?;

    FIGURE.with(|fig| {
        let mut f = fig.borrow_mut();
        let cur = f.current_mut();
        if cur.xlabel.is_empty() {
            cur.xlabel = "log10(x)".into();
        }
        if cur.ylabel.is_empty() {
            cur.ylabel = "log10(y)".into();
        }
    });
    sync_figure_outputs();
    Ok(r)
}

/// `semilogx(x, y [, opts])` — log-x, linear-y plot.
fn builtin_semilogx(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("semilogx", &args, 2, 8)?;
    let x = vector_arg(&args, 0, "semilogx", "x")?;
    let y = vector_arg(&args, 1, "semilogx", "y")?;
    assert_strictly_positive("semilogx", "x", x)?;
    let lx = vector_transform("semilogx", "x", x, |v| v.log10())?;

    let mut new_args: Vec<Value> = vec![Value::Vector(lx), Value::Vector(y.clone())];
    new_args.extend(args.into_iter().skip(2));
    let r = builtin_plot(new_args)?;

    FIGURE.with(|fig| {
        let mut f = fig.borrow_mut();
        let cur = f.current_mut();
        if cur.xlabel.is_empty() {
            cur.xlabel = "log10(x)".into();
        }
    });
    sync_figure_outputs();
    Ok(r)
}

/// `semilogy(x, y [, opts])` — linear-x, log-y plot.
fn builtin_semilogy(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("semilogy", &args, 2, 8)?;
    let x = vector_arg(&args, 0, "semilogy", "x")?;
    let y = vector_arg(&args, 1, "semilogy", "y")?;
    assert_strictly_positive("semilogy", "y", y)?;
    let ly = vector_transform("semilogy", "y", y, |v| v.log10())?;

    let mut new_args: Vec<Value> = vec![Value::Vector(x.clone()), Value::Vector(ly)];
    new_args.extend(args.into_iter().skip(2));
    let r = builtin_plot(new_args)?;

    FIGURE.with(|fig| {
        let mut f = fig.borrow_mut();
        let cur = f.current_mut();
        if cur.ylabel.is_empty() {
            cur.ylabel = "log10(y)".into();
        }
    });
    sync_figure_outputs();
    Ok(r)
}

/// `polar(theta, r [, opts])` — polar plot via Cartesian pre-transform:
/// plots `(r·cos(θ), r·sin(θ))` and labels the axes accordingly.
/// `theta` is in radians.
fn builtin_polar(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("polar", &args, 2, 8)?;
    let theta = vector_arg(&args, 0, "polar", "theta")?;
    let r = vector_arg(&args, 1, "polar", "r")?;
    if theta.len() != r.len() {
        return Err(ScriptError::type_err(format!(
            "polar: theta and r must be the same length (got {} vs {})",
            theta.len(),
            r.len()
        )));
    }
    let xs: CVector = theta
        .iter()
        .zip(r.iter())
        .map(|(t, ri)| {
            if t.im.abs() > 1e-12 || ri.im.abs() > 1e-12 {
                Err(ScriptError::type_err(
                    "polar: theta and r must be real-valued".into(),
                ))
            } else {
                Ok(Complex::new(ri.re * t.re.cos(), 0.0))
            }
        })
        .collect::<Result<Vec<_>, _>>()?
        .into();
    let ys: CVector = theta
        .iter()
        .zip(r.iter())
        .map(|(t, ri)| Complex::new(ri.re * t.re.sin(), 0.0))
        .collect::<Vec<_>>()
        .into();

    let mut new_args: Vec<Value> = vec![Value::Vector(xs), Value::Vector(ys)];
    new_args.extend(args.into_iter().skip(2));
    let res = builtin_plot(new_args)?;

    FIGURE.with(|fig| {
        let mut f = fig.borrow_mut();
        let cur = f.current_mut();
        if cur.xlabel.is_empty() {
            cur.xlabel = "r·cos(θ)".into();
        }
        if cur.ylabel.is_empty() {
            cur.ylabel = "r·sin(θ)".into();
        }
    });
    sync_figure_outputs();
    Ok(res)
}

/// title("text") — set title on current subplot.
fn builtin_title(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("title", &args, 1)?;
    let t = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;
    FIGURE.with(|fig| fig.borrow_mut().current_mut().title = t);
    sync_figure_outputs();
    Ok(Value::None)
}

/// xlim([lo, hi]) — set x-axis bounds on current subplot.
fn builtin_xlim(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("xlim", &args, 1)?;
    let v = match &args[0] {
        Value::Vector(v) if v.len() >= 2 => v.clone(),
        _ => {
            return Err(ScriptError::type_err(
                "xlim: expected [lo, hi] vector".to_string(),
            ))
        }
    };
    FIGURE.with(|fig| fig.borrow_mut().current_mut().xlim = (Some(v[0].re), Some(v[1].re)));
    sync_figure_outputs();
    Ok(Value::None)
}

/// ylim([lo, hi]) — set y-axis bounds on current subplot.
fn builtin_ylim(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("ylim", &args, 1)?;
    let v = match &args[0] {
        Value::Vector(v) if v.len() >= 2 => v.clone(),
        _ => {
            return Err(ScriptError::type_err(
                "ylim: expected [lo, hi] vector".to_string(),
            ))
        }
    };
    FIGURE.with(|fig| fig.borrow_mut().current_mut().ylim = (Some(v[0].re), Some(v[1].re)));
    sync_figure_outputs();
    Ok(Value::None)
}

/// subplot(rows, cols, idx) — switch to subplot panel (1-based index).
fn builtin_subplot(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("subplot", &args, 3)?;
    let rows = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let cols = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let idx = args[2].to_usize().map_err(|e| ScriptError::type_err(e))?;
    FIGURE.with(|fig| fig.borrow_mut().set_subplot(rows, cols, idx));
    sync_figure_outputs();
    Ok(Value::None)
}

/// legend("s1", "s2", ...) — retroactively label series in current subplot.
fn builtin_legend(args: Vec<Value>) -> Result<Value, ScriptError> {
    // legend() — enable legend using series labels already set via plot(..., "label", "name")
    // legend("l1", "l2", ...) — override series labels in order
    if !args.is_empty() {
        let labels: Vec<String> = args
            .iter()
            .map(|a| a.to_str().unwrap_or_default())
            .collect();
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            let sp = fig.current_mut();
            for (i, label) in labels.iter().enumerate() {
                if i < sp.series.len() {
                    sp.series[i].label = label.clone();
                }
            }
        });
    }
    sync_figure_outputs();
    Ok(Value::None)
}

/// imagesc(M) / imagesc(M, colormap) — display matrix as heatmap.
fn builtin_imagesc(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("imagesc", &args, 1, 2)?;
    let colormap = if args.len() == 2 {
        args[1].to_str().map_err(|e| ScriptError::type_err(e))?
    } else {
        "viridis".to_string()
    };
    let matrix = match &args[0] {
        Value::Matrix(m) => m.clone(),
        Value::Vector(v) => {
            // Treat as column vector matrix
            let n = v.len();
            ndarray::Array2::from_shape_fn((n, 1), |(i, _)| v[i])
        }
        other => {
            return Err(ScriptError::type_err(format!(
                "imagesc: expected matrix, got {}",
                other
            )))
        }
    };
    imagesc_terminal(&matrix, "", &colormap).map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();
    Ok(Value::None)
}

fn builtin_heatmap(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("heatmap", &args, 1, 5)?;

    let labeled = matches!(&args[0], Value::StringArray(_));
    let (xlabels, ylabels, matrix_idx) = if labeled {
        if args.len() < 3 {
            return Err(ScriptError::type_err(
                "heatmap: labeled form requires heatmap(xlabels, ylabels, M[, title[, colormap]])"
                    .to_string(),
            ));
        }
        let xl = match &args[0] {
            Value::StringArray(v) => v.clone(),
            _ => unreachable!(),
        };
        let yl = match &args[1] {
            Value::StringArray(v) => v.clone(),
            other => {
                return Err(ScriptError::type_err(format!(
                    "heatmap: ylabels must be a string array, got {}",
                    other
                )))
            }
        };
        (Some(xl), Some(yl), 2usize)
    } else {
        if args.len() > 2 {
            return Err(ScriptError::type_err(
                "heatmap: unlabeled form is heatmap(M) or heatmap(M, title); \
                 use heatmap(xlabels, ylabels, M, ...) for categorical labels"
                    .to_string(),
            ));
        }
        (None, None, 0usize)
    };

    let matrix: CMatrix = match &args[matrix_idx] {
        Value::Matrix(m) => m.clone(),
        Value::Vector(v) => {
            let n = v.len();
            Array2::from_shape_fn((n, 1), |(i, _)| v[i])
        }
        other => {
            return Err(ScriptError::type_err(format!(
                "heatmap: expected matrix, got {}",
                other
            )))
        }
    };
    let (nrows, ncols) = (matrix.nrows(), matrix.ncols());
    if nrows == 0 || ncols == 0 {
        return Err(ScriptError::type_err(
            "heatmap: matrix must be non-empty".to_string(),
        ));
    }

    if let Some(ref xl) = xlabels {
        if xl.len() != ncols {
            return Err(ScriptError::type_err(format!(
                "heatmap: xlabels length ({}) must match column count ({})",
                xl.len(),
                ncols
            )));
        }
    }
    if let Some(ref yl) = ylabels {
        if yl.len() != nrows {
            return Err(ScriptError::type_err(format!(
                "heatmap: ylabels length ({}) must match row count ({})",
                yl.len(),
                nrows
            )));
        }
    }

    let title = if args.len() > matrix_idx + 1 {
        args[matrix_idx + 1]
            .to_str()
            .map_err(ScriptError::type_err)?
    } else {
        String::new()
    };
    let colormap = if args.len() > matrix_idx + 2 {
        args[matrix_idx + 2]
            .to_str()
            .map_err(ScriptError::type_err)?
    } else {
        "viridis".to_string()
    };

    let vals: Vec<f64> = matrix.iter().map(|c| c.norm()).collect();
    let min_v = vals.iter().copied().fold(f64::INFINITY, f64::min);
    let max_v = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = (max_v - min_v).max(1e-12);
    let z: Vec<Vec<f64>> = (0..nrows)
        .map(|r| (0..ncols).map(|c| vals[r * ncols + c]).collect())
        .collect();

    let title_for_state = title.clone();
    let colormap_for_state = colormap.clone();
    let xlabels_for_state = xlabels.clone();
    let ylabels_for_state = ylabels.clone();
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
        }
        let sp = fig.current_mut();
        if !title_for_state.is_empty() && sp.title.is_empty() {
            sp.title = title_for_state;
        }
        sp.heatmap = Some(HeatmapData {
            z,
            colorscale: colormap_for_state,
            kind: HeatmapKind::Heatmap,
            x_labels: xlabels_for_state,
            y_labels: ylabels_for_state,
            ..Default::default()
        });
    });

    render_heatmap_tui(&vals, nrows, ncols, min_v, range, &title, &colormap)
        .map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();
    Ok(Value::None)
}

fn builtin_image(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("image", &args, 1, 3)?;

    fn to_matrix(v: &Value, name: &str) -> Result<CMatrix, ScriptError> {
        match v {
            Value::Matrix(m) => Ok(m.clone()),
            Value::Vector(vec) => {
                let n = vec.len();
                Ok(Array2::from_shape_fn((n, 1), |(i, _)| vec[i]))
            }
            other => Err(ScriptError::type_err(format!(
                "image: {} must be a matrix, got {}",
                name, other
            ))),
        }
    }

    fn check_real(name: &str, m: &CMatrix) -> Result<(), ScriptError> {
        for v in m.iter() {
            if v.im.abs() > 1e-9 {
                return Err(ScriptError::type_err(format!(
                    "image: {} channel must be a real matrix (got non-zero imag part)",
                    name
                )));
            }
        }
        Ok(())
    }

    enum Mode {
        Grayscale,
        Colormap(String),
        Rgb,
    }
    let mode = match args.len() {
        1 => Mode::Grayscale,
        2 => match &args[1] {
            Value::Str(s) => Mode::Colormap(s.clone()),
            other => {
                return Err(ScriptError::type_err(format!(
                    "image: 2nd arg must be a colormap name, got {}",
                    other
                )))
            }
        },
        3 => Mode::Rgb,
        _ => unreachable!(),
    };

    let m0 = to_matrix(&args[0], "matrix")?;
    let (nrows, ncols) = (m0.nrows(), m0.ncols());
    if nrows == 0 || ncols == 0 {
        return Err(ScriptError::type_err(
            "image: matrix must be non-empty".to_string(),
        ));
    }

    let mut rgba = Vec::with_capacity(nrows * ncols * 4);
    match &mode {
        Mode::Grayscale => {
            for r in 0..nrows {
                for c in 0..ncols {
                    let v = m0[[r, c]].norm().clamp(0.0, 255.0) as u8;
                    rgba.extend_from_slice(&[v, v, v, 255]);
                }
            }
        }
        Mode::Colormap(name) => {
            for r in 0..nrows {
                for c in 0..ncols {
                    let v = m0[[r, c]].norm().clamp(0.0, 255.0);
                    let t = v / 255.0;
                    let (rr, gg, bb) = colormap_rgb(t, name);
                    rgba.extend_from_slice(&[rr, gg, bb, 255]);
                }
            }
        }
        Mode::Rgb => {
            let mg = to_matrix(&args[1], "G channel")?;
            let mb = to_matrix(&args[2], "B channel")?;
            if mg.nrows() != nrows
                || mg.ncols() != ncols
                || mb.nrows() != nrows
                || mb.ncols() != ncols
            {
                return Err(ScriptError::type_err(
                    "image: R, G, B matrices must share the same shape".to_string(),
                ));
            }
            check_real("R", &m0)?;
            check_real("G", &mg)?;
            check_real("B", &mb)?;
            for r in 0..nrows {
                for c in 0..ncols {
                    let rr = m0[[r, c]].re.clamp(0.0, 255.0) as u8;
                    let gg = mg[[r, c]].re.clamp(0.0, 255.0) as u8;
                    let bb = mb[[r, c]].re.clamp(0.0, 255.0) as u8;
                    rgba.extend_from_slice(&[rr, gg, bb, 255]);
                }
            }
        }
    }

    let rgba_for_state = rgba.clone();
    FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            let sp = fig.current_mut();
            sp.series.clear();
            sp.title.clear();
        }
        let sp = fig.current_mut();
        sp.heatmap = Some(HeatmapData {
            kind: HeatmapKind::ImageRgba,
            rgba: Some(rgba_for_state),
            rgba_width: ncols as u32,
            rgba_height: nrows as u32,
            ..Default::default()
        });
    });

    render_image_tui(&rgba, ncols, nrows, "")
        .map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();
    Ok(Value::None)
}

// ─── Contour plots ───────────────────────────────────────────────────────────

/// Convert a Value::Matrix scalar field to `Vec<Vec<f64>>` of magnitudes.
fn z_matrix_to_rows(m: &CMatrix) -> Vec<Vec<f64>> {
    let (nrows, ncols) = (m.nrows(), m.ncols());
    (0..nrows)
        .map(|r| (0..ncols).map(|c| m[[r, c]].norm()).collect())
        .collect()
}

/// Parse the trailing `(modifier_or_color | "title")` arguments shared by
/// `contour` and `contourf`. Returns `(levels, title, color)` where exactly
/// one of `levels` or `nlevels` may be provided. `color` is only meaningful
/// for line contours; `contourf` ignores it.
fn parse_contour_modifiers(
    name: &str,
    rest: &[Value],
) -> Result<
    (
        Option<usize>,    // explicit nlevels (scalar)
        Option<Vec<f64>>, // explicit levels (vector)
        Option<String>,   // title
        Option<rustlab_plot::SeriesColor>,
    ),
    ScriptError,
> {
    let mut nlevels = None;
    let mut levels = None;
    let mut title = None;
    let mut color = None;

    for v in rest {
        match v {
            Value::Scalar(n) => {
                if nlevels.is_some() || levels.is_some() {
                    return Err(ScriptError::type_err(format!(
                        "{name}: nlevels / levels specified more than once"
                    )));
                }
                let n_int = n.round() as i64;
                if n_int < 1 {
                    return Err(ScriptError::type_err(format!(
                        "{name}: nlevels must be >= 1, got {n}"
                    )));
                }
                nlevels = Some(n_int as usize);
            }
            Value::Vector(vec) => {
                if nlevels.is_some() || levels.is_some() {
                    return Err(ScriptError::type_err(format!(
                        "{name}: nlevels / levels specified more than once"
                    )));
                }
                let mut lv: Vec<f64> = vec.iter().map(|c| c.re).collect();
                lv.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                levels = Some(lv);
            }
            Value::Str(s) => {
                if let Some(c) = rustlab_plot::SeriesColor::parse(s) {
                    if color.is_some() {
                        return Err(ScriptError::type_err(format!(
                            "{name}: colour specified more than once"
                        )));
                    }
                    color = Some(c);
                } else {
                    if title.is_some() {
                        return Err(ScriptError::type_err(format!(
                            "{name}: title specified more than once"
                        )));
                    }
                    title = Some(s.clone());
                }
            }
            other => {
                return Err(ScriptError::type_err(format!(
                    "{name}: unexpected modifier of type {}",
                    other.type_name()
                )));
            }
        }
    }
    Ok((nlevels, levels, title, color))
}

/// Extract the `(X, Y, Z)` triple from the first 1 or 3 arguments. The
/// remaining arguments (if any) are returned as a modifier slice.
fn parse_xyz<'a>(
    name: &str,
    args: &'a [Value],
) -> Result<(Vec<f64>, Vec<f64>, CMatrix, &'a [Value]), ScriptError> {
    if args.is_empty() {
        return Err(ScriptError::type_err(format!(
            "{name}: expected at least one argument (Z)"
        )));
    }
    // First, see whether args[0..3] is an (X, Y, Z) triple. We require
    // args[0] and args[1] to NOT be matrices that represent Z (they should
    // be vectors or meshgrid matrices) and args[2] to be a Matrix.
    if args.len() >= 3 {
        if let Value::Matrix(m) = &args[2] {
            let z = m.clone();
            let x = axis_from_value(&args[0], name, "X", z.ncols(), false)?;
            let y = axis_from_value(&args[1], name, "Y", z.nrows(), true)?;
            return Ok((x, y, z, &args[3..]));
        }
    }
    // Fall back to the 1-arg form: args[0] is Z, X and Y default to indices.
    match &args[0] {
        Value::Matrix(m) => {
            let z = m.clone();
            let (nrows, ncols) = (z.nrows(), z.ncols());
            let x: Vec<f64> = (0..ncols).map(|i| (i + 1) as f64).collect();
            let y: Vec<f64> = (0..nrows).map(|i| (i + 1) as f64).collect();
            Ok((x, y, z, &args[1..]))
        }
        other => Err(ScriptError::type_err(format!(
            "{}: expected a matrix Z, got {}",
            name,
            other.type_name()
        ))),
    }
}

fn warn_terminal_contour_once() {
    use std::sync::Once;
    static WARN: Once = Once::new();
    WARN.call_once(|| {
        eprintln!(
            "rustlab: contour/contourf are not rendered to the terminal — \
             use savefig(\"plot.svg\" or \"plot.html\") to view them."
        );
    });
}

fn push_contour_state(
    name: &str,
    x: Vec<f64>,
    y: Vec<f64>,
    z_rows: Vec<Vec<f64>>,
    nlevels: Option<usize>,
    levels: Option<Vec<f64>>,
    title: Option<String>,
    filled: bool,
    line_color: Option<rustlab_plot::SeriesColor>,
    colorscale: String,
) -> Result<(), ScriptError> {
    if z_rows.is_empty() || z_rows[0].is_empty() {
        return Err(ScriptError::type_err(format!("{name}: Z is empty")));
    }
    if z_rows.len() < 2 || z_rows[0].len() < 2 {
        return Err(ScriptError::type_err(format!(
            "{name}: Z must be at least 2x2"
        )));
    }
    let resolved_levels = if let Some(lv) = levels {
        if lv.is_empty() {
            return Err(ScriptError::type_err(format!(
                "{name}: explicit levels vector must be non-empty"
            )));
        }
        lv
    } else {
        let n = nlevels.unwrap_or(10);
        rustlab_plot::contour::auto_levels(&z_rows, n)
    };

    rustlab_plot::FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            // Replace mode: clear any prior contours on this subplot. Leave
            // the heatmap and series alone (same convention as imagesc).
            fig.current_mut().contours.clear();
        }
        if let Some(t) = &title {
            fig.current_mut().title = t.clone();
        }
        fig.current_mut().contours.push(rustlab_plot::ContourData {
            z: z_rows,
            x,
            y,
            levels: resolved_levels,
            filled,
            line_color,
            colorscale,
        });
    });

    if rustlab_plot::plot_context() == rustlab_plot::PlotContext::Terminal {
        warn_terminal_contour_once();
    }
    sync_figure_outputs();
    Ok(())
}

/// `contour(Z [, ...])` or `contour(X, Y, Z [, ...])` — line contour plot.
fn builtin_contour(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("contour", &args, 1, 5)?;
    let (x, y, z_mat, rest) = parse_xyz("contour", &args)?;
    let (nlevels, levels, title, color) = parse_contour_modifiers("contour", rest)?;
    let z_rows = z_matrix_to_rows(&z_mat);
    push_contour_state(
        "contour",
        x,
        y,
        z_rows,
        nlevels,
        levels,
        title,
        false,
        color,
        "viridis".to_string(),
    )?;
    Ok(Value::None)
}

/// `contourf(Z [, ...])` or `contourf(X, Y, Z [, ...])` — filled contour plot.
fn builtin_contourf(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("contourf", &args, 1, 5)?;
    let (x, y, z_mat, rest) = parse_xyz("contourf", &args)?;
    let (nlevels, levels, title, _color) = parse_contour_modifiers("contourf", rest)?;
    let z_rows = z_matrix_to_rows(&z_mat);
    // For contourf the optional string is reused as a colormap name when
    // not consumed as title or colour.
    let mut colorscale = "viridis".to_string();
    // If the user supplied a string that didn't match a colour or title,
    // parse_contour_modifiers will have classified it as a title. We accept
    // that — to override the colormap users can pass the next layer (e.g.
    // explicit imagesc styling).
    // (Discoverable pattern: contourf(X, Y, Z, 12) for 12 bands.)
    let _ = &mut colorscale;
    push_contour_state(
        "contourf", x, y, z_rows, nlevels, levels, title, true, None, colorscale,
    )?;
    Ok(Value::None)
}

// ─── Quiver and streamline plots ─────────────────────────────────────────────

/// Convert a `CMatrix` to `Vec<Vec<f64>>` by taking the real part of each
/// complex entry. Used for vector-field components where imaginary content is
/// not meaningful (same convention as `axis_from_value`).
fn vector_field_matrix_to_rows(m: &CMatrix) -> Vec<Vec<f64>> {
    let (nrows, ncols) = (m.nrows(), m.ncols());
    (0..nrows)
        .map(|r| (0..ncols).map(|c| m[[r, c]].re).collect())
        .collect()
}

/// Extract `(X, Y, U, V)` from the first 2 or 4 arguments. For the 2-arg
/// shortcut `quiver(U, V)` we synthesise X and Y from grid indices.
fn parse_xyuv<'a>(
    name: &str,
    args: &'a [Value],
) -> Result<
    (
        Vec<f64>,
        Vec<f64>,
        Vec<Vec<f64>>,
        Vec<Vec<f64>>,
        &'a [Value],
    ),
    ScriptError,
> {
    if args.len() >= 4 {
        if let (Value::Matrix(um), Value::Matrix(vm)) = (&args[2], &args[3]) {
            if um.shape() != vm.shape() {
                return Err(ScriptError::type_err(format!(
                    "{name}: U and V must have the same shape"
                )));
            }
            let (nrows, ncols) = (um.nrows(), um.ncols());
            let x = axis_from_value(&args[0], name, "X", ncols, false)?;
            let y = axis_from_value(&args[1], name, "Y", nrows, true)?;
            let u = vector_field_matrix_to_rows(um);
            let v = vector_field_matrix_to_rows(vm);
            return Ok((x, y, u, v, &args[4..]));
        }
    }
    if args.len() >= 2 {
        if let (Value::Matrix(um), Value::Matrix(vm)) = (&args[0], &args[1]) {
            if um.shape() != vm.shape() {
                return Err(ScriptError::type_err(format!(
                    "{name}: U and V must have the same shape"
                )));
            }
            let (nrows, ncols) = (um.nrows(), um.ncols());
            let (x, y) = rustlab_plot::quiver::default_xy(nrows, ncols);
            let u = vector_field_matrix_to_rows(um);
            let v = vector_field_matrix_to_rows(vm);
            return Ok((x, y, u, v, &args[2..]));
        }
    }
    Err(ScriptError::type_err(format!(
        "{name}: expected (X, Y, U, V) or (U, V) with matrix arguments"
    )))
}

fn warn_terminal_quiver_once() {
    use std::sync::Once;
    static WARN: Once = Once::new();
    WARN.call_once(|| {
        eprintln!(
            "rustlab: quiver is not rendered to the terminal — \
             use savefig(\"plot.svg\" or \"plot.html\") to view it."
        );
    });
}

fn warn_terminal_streamplot_once() {
    use std::sync::Once;
    static WARN: Once = Once::new();
    WARN.call_once(|| {
        eprintln!(
            "rustlab: streamplot is not rendered to the terminal — \
             use savefig(\"plot.svg\" or \"plot.html\") to view it."
        );
    });
}

/// `quiver(X, Y, U, V [, scale | "title"])` or `quiver(U, V [, ...])`.
///
/// Draws an arrow glyph at each grid point. Arrows are auto-scaled so the
/// longest one equals the nearest-neighbour cell distance; an explicit
/// `scale` multiplier is applied on top. NaN entries in `U` or `V` skip
/// that cell. Honours `hold on` to overlay on an existing `imagesc` or
/// `contour`.
fn builtin_quiver(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("quiver", &args, 2, 6)?;
    let (x, y, u, v, rest) = parse_xyuv("quiver", &args)?;
    let mut scale = 1.0f64;
    let mut title: Option<String> = None;
    let mut color: Option<rustlab_plot::SeriesColor> = None;
    for arg in rest {
        match arg {
            Value::Scalar(n) => {
                if !n.is_finite() || *n <= 0.0 {
                    return Err(ScriptError::type_err(
                        "quiver: scale must be a positive finite number".into(),
                    ));
                }
                scale = *n;
            }
            Value::Str(s) => {
                if let Some(c) = rustlab_plot::SeriesColor::parse(s) {
                    color = Some(c);
                } else {
                    title = Some(s.clone());
                }
            }
            other => {
                return Err(ScriptError::type_err(format!(
                    "quiver: unexpected argument of type {}",
                    other.type_name()
                )));
            }
        }
    }

    rustlab_plot::FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            fig.current_mut().quivers.clear();
        }
        if let Some(t) = &title {
            fig.current_mut().title = t.clone();
        }
        fig.current_mut().quivers.push(rustlab_plot::QuiverData {
            x,
            y,
            u,
            v,
            scale,
            color,
            title: title.clone(),
        });
    });

    if rustlab_plot::plot_context() == rustlab_plot::PlotContext::Terminal {
        warn_terminal_quiver_once();
    }
    sync_figure_outputs();
    Ok(Value::None)
}

/// `streamplot(X, Y, U, V [, density | seeds | "title"])`.
///
/// Integrates streamlines from seed points via RK4 forward and backward
/// until they leave the domain or hit a NaN / near-zero sample. When no
/// explicit `seeds` matrix is supplied, places seeds on a grid sized by
/// `density` (default 1.0 → ≈ one seed per grid cell). Honours `hold on`
/// for overlay on `imagesc` / `contour`.
fn builtin_streamplot(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("streamplot", &args, 2, 7)?;
    let (x, y, u, v, rest) = parse_xyuv("streamplot", &args)?;
    let mut density = 1.0f64;
    let mut title: Option<String> = None;
    let mut color: Option<rustlab_plot::SeriesColor> = None;
    let mut seeds: Option<Vec<(f64, f64)>> = None;
    for arg in rest {
        match arg {
            Value::Scalar(n) => {
                if !n.is_finite() || *n <= 0.0 {
                    return Err(ScriptError::type_err(
                        "streamplot: density must be a positive finite number".into(),
                    ));
                }
                density = *n;
            }
            Value::Str(s) => {
                if let Some(c) = rustlab_plot::SeriesColor::parse(s) {
                    color = Some(c);
                } else {
                    title = Some(s.clone());
                }
            }
            Value::Matrix(m) => {
                if m.ncols() != 2 {
                    return Err(ScriptError::type_err(format!(
                        "streamplot: seeds matrix must be N×2 (got {}×{})",
                        m.nrows(),
                        m.ncols()
                    )));
                }
                let pts: Vec<(f64, f64)> = (0..m.nrows())
                    .map(|i| (m[[i, 0]].re, m[[i, 1]].re))
                    .collect();
                seeds = Some(pts);
            }
            other => {
                return Err(ScriptError::type_err(format!(
                    "streamplot: unexpected argument of type {}",
                    other.type_name()
                )));
            }
        }
    }

    rustlab_plot::FIGURE.with(|fig| {
        let mut fig = fig.borrow_mut();
        if !fig.hold {
            fig.current_mut().streamlines.clear();
        }
        if let Some(t) = &title {
            fig.current_mut().title = t.clone();
        }
        fig.current_mut()
            .streamlines
            .push(rustlab_plot::StreamlineData {
                x,
                y,
                u,
                v,
                density,
                seeds,
                color,
                title: title.clone(),
            });
    });

    if rustlab_plot::plot_context() == rustlab_plot::PlotContext::Terminal {
        warn_terminal_streamplot_once();
    }
    sync_figure_outputs();
    Ok(Value::None)
}

/// `surf(Z)` / `surf(X, Y, Z)` / `surf(X, Y, Z, "colormap")` — 3D surface plot.
///
/// Z is an nrows×ncols matrix. X and Y may be either:
///   - 1-D vectors: length must match ncols (X) and nrows (Y), OR
///   - 2-D matrices from `meshgrid(...)`: we take row 0 for X and column 0 for Y.
///
/// Under the viewer (`viewer on` or `--plot viewer`), renders an interactive
/// 3D surface with mouse drag to rotate, scroll to zoom, right-drag to pan.
/// Under HTML output, emits a Plotly 3D surface. Terminal/SVG/PNG fall back
/// to static renders.
fn builtin_surf(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("surf", &args, 1, 4)?;

    let (x, y, z_mat, colormap) = match args.len() {
        1 => {
            let z = match &args[0] {
                Value::Matrix(m) => m.clone(),
                other => {
                    return Err(ScriptError::type_err(format!(
                        "surf: expected matrix Z, got {}",
                        other.type_name()
                    )))
                }
            };
            let (nrows, ncols) = (z.nrows(), z.ncols());
            let x: Vec<f64> = (0..ncols).map(|i| (i + 1) as f64).collect();
            let y: Vec<f64> = (0..nrows).map(|i| (i + 1) as f64).collect();
            (x, y, z, "viridis".to_string())
        }
        n @ (3 | 4) => {
            let z = match &args[2] {
                Value::Matrix(m) => m.clone(),
                other => {
                    return Err(ScriptError::type_err(format!(
                        "surf: expected matrix Z as third argument, got {}",
                        other.type_name()
                    )))
                }
            };
            let x = axis_from_value(&args[0], "surf", "X", z.ncols(), false)?;
            let y = axis_from_value(&args[1], "surf", "Y", z.nrows(), true)?;
            let cmap = if n == 4 {
                args[3].to_str().map_err(|e| ScriptError::type_err(e))?
            } else {
                "viridis".to_string()
            };
            (x, y, z, cmap)
        }
        _ => {
            return Err(ScriptError::runtime(format!(
                "surf: expected 1, 3, or 4 arguments, got {}",
                args.len()
            )));
        }
    };

    let nrows = z_mat.nrows();
    let ncols = z_mat.ncols();
    if nrows == 0 || ncols == 0 {
        return Err(ScriptError::type_err("surf: Z is empty".to_string()));
    }
    // Convert Z matrix (C64) to Vec<Vec<f64>> using magnitudes so complex
    // inputs degrade sensibly (matches imagesc's convention).
    let z_rows: Vec<Vec<f64>> = (0..nrows)
        .map(|r| (0..ncols).map(|c| z_mat[[r, c]].norm()).collect())
        .collect();

    surf_terminal(z_rows, x, y, "", &colormap).map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();
    Ok(Value::None)
}

/// Normalize a meshgrid-style axis argument to a 1-D Vec<f64> of `expected` length.
/// Accepts a 1-D vector, or a 2-D matrix from `meshgrid(...)`:
///   - `is_y = false` (X axis): take row 0 (length = ncols).
///   - `is_y = true`  (Y axis): take column 0 (length = nrows).
fn axis_from_value(
    val: &Value,
    fn_name: &str,
    arg_name: &str,
    expected: usize,
    is_y: bool,
) -> Result<Vec<f64>, ScriptError> {
    match val {
        Value::Vector(v) => {
            if v.len() != expected {
                return Err(ScriptError::type_err(format!(
                    "{}: {} length ({}) must match Z dimension ({})",
                    fn_name,
                    arg_name,
                    v.len(),
                    expected
                )));
            }
            Ok(v.iter().map(|c| c.re).collect())
        }
        Value::Matrix(m) => {
            let axis: Vec<f64> = if is_y {
                (0..m.nrows()).map(|i| m[[i, 0]].re).collect()
            } else {
                (0..m.ncols()).map(|j| m[[0, j]].re).collect()
            };
            if axis.len() != expected {
                return Err(ScriptError::type_err(format!(
                    "{}: {} dimension ({}) must match Z ({})",
                    fn_name,
                    arg_name,
                    axis.len(),
                    expected
                )));
            }
            Ok(axis)
        }
        other => Err(ScriptError::type_err(format!(
            "{}: {} must be a vector or matrix, got {}",
            fn_name,
            arg_name,
            other.type_name()
        ))),
    }
}

/// Extract (freqs: RVector, H: CVector) from a 2×n Matrix (as returned by freqz).
fn extract_freq_response(val: &Value) -> Result<(rustlab_core::RVector, CVector), ScriptError> {
    match val {
        Value::Matrix(m) => {
            if m.nrows() < 2 {
                return Err(ScriptError::type_err(
                    "plotdb: expected a 2×n matrix from freqz".to_string(),
                ));
            }
            let freqs = ndarray::Array1::from_iter(m.row(0).iter().map(|c| c.re));
            let h = ndarray::Array1::from_iter(m.row(1).iter().copied());
            Ok((freqs, h))
        }
        other => Err(ScriptError::type_err(format!(
            "plotdb: expected matrix from freqz, got {other}"
        ))),
    }
}

/// Extract the real part of any numeric Value as an RVector.
/// Coerce a Value to CMatrix: Matrix passes through, Scalar becomes 1×1, Vector becomes n×1.
fn to_cmatrix_arg(val: &Value, fn_name: &str, arg_name: &str) -> Result<CMatrix, ScriptError> {
    match val {
        Value::Matrix(m) => Ok(m.clone()),
        Value::Scalar(n) => Ok(Array2::from_elem((1, 1), Complex::new(*n, 0.0))),
        Value::Complex(c) => Ok(Array2::from_elem((1, 1), *c)),
        Value::Vector(v) => {
            let m = Array2::from_shape_fn((v.len(), 1), |(i, _)| v[i]);
            Ok(m)
        }
        other => Err(ScriptError::type_err(format!(
            "{}: {} must be a matrix or vector, got {}",
            fn_name,
            arg_name,
            other.type_name()
        ))),
    }
}

fn to_real_vector(val: &Value) -> Result<rustlab_core::RVector, ScriptError> {
    match val {
        Value::Vector(v) => Ok(ndarray::Array1::from_iter(v.iter().map(|c| c.re))),
        Value::Scalar(n) => Ok(ndarray::Array1::from_vec(vec![*n])),
        Value::Matrix(m) if m.ncols() == 1 => {
            Ok(ndarray::Array1::from_iter(m.column(0).iter().map(|c| c.re)))
        }
        other => Err(ScriptError::type_err(format!(
            "cannot plot value of type {other}"
        ))),
    }
}

// Nx1 column matrices are 1D data in user-facing terms. Rewrite such args to
// Vector so plot/stem/bar can treat them uniformly with row vectors. Leaves
// multi-column matrices alone (they remain grouped series).
fn flatten_column_matrix_args(args: &mut [Value]) {
    for a in args.iter_mut() {
        if let Value::Matrix(m) = a {
            if m.ncols() == 1 {
                let v: Vec<Complex<f64>> = m.column(0).iter().copied().collect();
                *a = Value::Vector(ndarray::Array1::from_vec(v));
            }
        }
    }
}

// ─── Save / Load / whos builtins ──────────────────────────────────────────

// ── NPY helpers ────────────────────────────────────────────────────────────

/// Flatten a Value into (data, shape) for NPY serialisation.
fn value_to_c64_array(val: &Value) -> Result<(Vec<Complex<f64>>, Vec<usize>), String> {
    match val {
        Value::Scalar(n) => Ok((vec![Complex::new(*n, 0.0)], vec![1])),
        Value::Complex(c) => Ok((vec![*c], vec![1])),
        Value::Vector(v) => Ok((v.iter().copied().collect(), vec![v.len()])),
        Value::Matrix(m) => {
            // ndarray Array2 is row-major (C order) — iter() gives row-major order
            let data: Vec<Complex<f64>> = m.iter().copied().collect();
            Ok((data, vec![m.nrows(), m.ncols()]))
        }
        Value::Tensor3(t) => {
            let s = t.shape();
            let data: Vec<Complex<f64>> = t.iter().copied().collect();
            Ok((data, vec![s[0], s[1], s[2]]))
        }
        other => Err(format!("save: cannot serialise {} to NPY", other)),
    }
}

/// Build the raw bytes of an NPY v1.0 file.
fn build_npy_bytes(data: &[Complex<f64>], shape: &[usize]) -> Vec<u8> {
    let real_only = data.iter().all(|c| c.im.abs() < 1e-12);
    let descr = if real_only { "<f8" } else { "<c16" };

    let shape_str = match shape {
        [n] => format!("({n},)"),
        [r, c] => format!("({r}, {c})"),
        other => {
            let parts: Vec<String> = other.iter().map(|d| d.to_string()).collect();
            format!("({})", parts.join(", "))
        }
    };
    let raw = format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': {shape_str}, }}");

    // Total = 10 (prefix) + header_len; must be divisible by 64.
    let needed = 10 + raw.len() + 1; // +1 for the trailing '\n'
    let padded = ((needed + 63) / 64) * 64;
    let header = format!("{}{}\n", raw, " ".repeat(padded - needed));
    let hlen = header.len() as u16;

    let mut out = Vec::with_capacity(padded + data.len() * if real_only { 8 } else { 16 });
    out.extend_from_slice(b"\x93NUMPY");
    out.push(1);
    out.push(0);
    out.extend_from_slice(&hlen.to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    if real_only {
        for c in data {
            out.extend_from_slice(&c.re.to_le_bytes());
        }
    } else {
        for c in data {
            out.extend_from_slice(&c.re.to_le_bytes());
            out.extend_from_slice(&c.im.to_le_bytes());
        }
    }
    out
}

/// Parse the shape tuple from an NPY header string.
fn parse_npy_shape(header: &str) -> Result<Vec<usize>, String> {
    let key = header
        .find("'shape':")
        .or_else(|| header.find("\"shape\":"))
        .ok_or_else(|| "NPY header missing 'shape' field".to_string())?;
    let after = &header[key..];
    let open = after
        .find('(')
        .ok_or_else(|| "NPY header: bad shape (no '(')".to_string())?;
    let close = after
        .find(')')
        .ok_or_else(|| "NPY header: bad shape (no ')')".to_string())?;
    let inner = after[open + 1..close].trim();
    if inner.is_empty() {
        return Ok(vec![]); // 0-d array
    }
    inner
        .split(',')
        .filter_map(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.parse::<usize>())
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("NPY shape parse error: {e}"))
}

/// Reconstruct a Value from a flat array + shape.
fn array_to_value(values: Vec<Complex<f64>>, shape: &[usize]) -> Result<Value, String> {
    match shape {
        [] | [1] => {
            let c = *values.first().ok_or("NPY: empty array")?;
            if c.im.abs() < 1e-12 {
                Ok(Value::Scalar(c.re))
            } else {
                Ok(Value::Complex(c))
            }
        }
        [_n] => Ok(Value::Vector(Array1::from_vec(values))),
        [nrows, ncols] => {
            let mat =
                Array2::from_shape_vec((*nrows, *ncols), values).map_err(|e| e.to_string())?;
            Ok(Value::Matrix(mat))
        }
        [m, n, p] => {
            let t = Array3::from_shape_vec((*m, *n, *p), values).map_err(|e| e.to_string())?;
            Ok(Value::Tensor3(t))
        }
        other => Err(format!("NPY: unsupported shape rank {}", other.len())),
    }
}

/// Parse an in-memory NPY byte buffer into a Value.
fn parse_npy_bytes(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() < 10 || &bytes[0..6] != b"\x93NUMPY" {
        return Err("not a valid NPY file".to_string());
    }
    let hlen = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    let hend = 10 + hlen;
    if bytes.len() < hend {
        return Err("NPY file truncated in header".to_string());
    }
    let header = std::str::from_utf8(&bytes[10..hend]).map_err(|e| e.to_string())?;
    let is_c16 = header.contains("<c16") || header.contains(">c16");
    let is_f8 = header.contains("<f8") || header.contains(">f8");
    let shape = parse_npy_shape(header)?;
    let data = &bytes[hend..];

    if is_c16 {
        if data.len() % 16 != 0 {
            return Err("NPY complex128: data length is not a multiple of 16".to_string());
        }
        let values: Vec<Complex<f64>> = (0..data.len() / 16)
            .map(|i| {
                let re = f64::from_le_bytes(data[i * 16..i * 16 + 8].try_into().unwrap());
                let im = f64::from_le_bytes(data[i * 16 + 8..i * 16 + 16].try_into().unwrap());
                Complex::new(re, im)
            })
            .collect();
        array_to_value(values, &shape)
    } else if is_f8 {
        if data.len() % 8 != 0 {
            return Err("NPY float64: data length is not a multiple of 8".to_string());
        }
        let values: Vec<Complex<f64>> = (0..data.len() / 8)
            .map(|i| {
                let f = f64::from_le_bytes(data[i * 8..i * 8 + 8].try_into().unwrap());
                Complex::new(f, 0.0)
            })
            .collect();
        array_to_value(values, &shape)
    } else {
        Err(format!(
            "unsupported NPY dtype (only <f8 and <c16 are supported): {}",
            header.chars().take(100).collect::<String>()
        ))
    }
}

// ── CSV helpers ────────────────────────────────────────────────────────────

fn fmt_csv_cell(c: Complex<f64>) -> String {
    if c.im.abs() < 1e-12 {
        format!("{}", c.re)
    } else if c.im >= 0.0 {
        format!("{}+{}i", c.re, c.im)
    } else {
        format!("{}{}i", c.re, c.im) // im already negative
    }
}

/// Parse a single CSV cell as a real or complex number.
fn parse_csv_cell(s: &str) -> Result<Complex<f64>, String> {
    let s = s.trim();
    // No imaginary suffix → plain real
    if !s.ends_with('i') && !s.ends_with('j') {
        return s
            .parse::<f64>()
            .map(|f| Complex::new(f, 0.0))
            .map_err(|_| format!("cannot parse '{}' as a number", s));
    }
    // Strip 'i'/'j' suffix and find the split between re and im parts.
    let body = &s[..s.len() - 1];
    let bytes = body.as_bytes();
    // Scan right-to-left for + or - that is not the very first character
    let split = (1..bytes.len())
        .rev()
        .find(|&i| bytes[i] == b'+' || bytes[i] == b'-');
    if let Some(i) = split {
        let re: f64 = body[..i]
            .parse()
            .map_err(|_| format!("invalid real part in '{}'", s))?;
        let im: f64 = match &body[i..] {
            "+" => 1.0,
            "-" => -1.0,
            t => t
                .parse()
                .map_err(|_| format!("invalid imaginary part in '{}'", s))?,
        };
        Ok(Complex::new(re, im))
    } else {
        // Pure imaginary: body is e.g. "2.5" or "-2.5"
        let im: f64 = match body {
            "" | "+" => 1.0,
            "-" => -1.0,
            t => t
                .parse()
                .map_err(|_| format!("cannot parse imaginary '{}' in '{}'", t, s))?,
        };
        Ok(Complex::new(0.0, im))
    }
}

fn write_csv(path: &str, val: &Value) -> Result<(), String> {
    use std::io::Write;
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut w = std::io::BufWriter::new(file);
    match val {
        Value::Scalar(n) => writeln!(w, "{n}").map_err(|e| e.to_string())?,
        Value::Complex(c) => writeln!(w, "{}", fmt_csv_cell(*c)).map_err(|e| e.to_string())?,
        Value::Vector(v) => {
            for c in v.iter() {
                writeln!(w, "{}", fmt_csv_cell(*c)).map_err(|e| e.to_string())?;
            }
        }
        Value::Matrix(m) => {
            for r in 0..m.nrows() {
                for ci in 0..m.ncols() {
                    if ci > 0 {
                        write!(w, ",").map_err(|e| e.to_string())?;
                    }
                    write!(w, "{}", fmt_csv_cell(m[[r, ci]])).map_err(|e| e.to_string())?;
                }
                writeln!(w).map_err(|e| e.to_string())?;
            }
        }
        other => return Err(format!("save: cannot serialise {} to CSV", other)),
    }
    Ok(())
}

fn load_csv(path: &str) -> Result<Value, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return Ok(Value::Vector(Array1::zeros(0)));
    }
    let mut rows: Vec<Vec<Complex<f64>>> = Vec::with_capacity(lines.len());
    for line in &lines {
        let cells: Result<Vec<_>, _> = line.split(',').map(parse_csv_cell).collect();
        rows.push(cells.map_err(|e| format!("CSV parse error: {e}"))?);
    }
    let ncols = rows[0].len();
    if rows.iter().any(|r| r.len() != ncols) {
        return Err("CSV load: rows have inconsistent column counts".to_string());
    }
    match (rows.len(), ncols) {
        (1, 1) => {
            let c = rows[0][0];
            if c.im.abs() < 1e-12 {
                Ok(Value::Scalar(c.re))
            } else {
                Ok(Value::Complex(c))
            }
        }
        (_, 1) => {
            // Column vector
            Ok(Value::Vector(Array1::from_vec(
                rows.into_iter().map(|r| r[0]).collect(),
            )))
        }
        (1, _) => {
            // Row vector
            Ok(Value::Vector(Array1::from_vec(
                rows.into_iter().next().unwrap(),
            )))
        }
        (nrows, ncols) => {
            let flat: Vec<Complex<f64>> = rows.into_iter().flatten().collect();
            let mat = Array2::from_shape_vec((nrows, ncols), flat).map_err(|e| e.to_string())?;
            Ok(Value::Matrix(mat))
        }
    }
}

// ── NPZ helpers ────────────────────────────────────────────────────────────

fn save_npz(path: &str, pairs: &[Value]) -> Result<(), String> {
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    for chunk in pairs.chunks(2) {
        let name = chunk[0].to_str().map_err(|e| format!("save NPZ: {e}"))?;
        let (data, shape) = value_to_c64_array(&chunk[1])?;
        let npy = build_npy_bytes(&data, &shape);
        zip.start_file(format!("{name}.npy"), opts)
            .map_err(|e| e.to_string())?;
        zip.write_all(&npy).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Load all variables from an NPZ file. Returns (var_name, value) pairs in zip order.
pub fn load_all_from_npz(path: &str) -> Result<Vec<(String, Value)>, String> {
    use std::io::Read;
    use zip::ZipArchive;

    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let entry_name = entry.name().to_string();
        let var_name = entry_name
            .strip_suffix(".npy")
            .unwrap_or(&entry_name)
            .to_string();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        result.push((var_name, parse_npy_bytes(&buf)?));
    }
    Ok(result)
}

fn load_from_npz(path: &str, name: &str) -> Result<Value, String> {
    use std::io::Read;
    use zip::ZipArchive;

    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|e| e.to_string())?;
    let entry_name = format!("{name}.npy");
    let mut entry = zip
        .by_name(&entry_name)
        .map_err(|_| format!("'{}' not found in {}", name, path))?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    parse_npy_bytes(&buf)
}

// ── Builtins ───────────────────────────────────────────────────────────────

fn builtin_save(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() < 2 {
        return Err(ScriptError::type_err(
            "save: usage:\n  save(\"file.npy\", x)\n  save(\"file.csv\", x)\n  save(\"file.toml\", s)\n  save(\"file.npz\", \"name1\", x1, \"name2\", x2, ...)".to_string()
        ));
    }
    let path = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;

    if path.ends_with(".npz") {
        let pairs = &args[1..];
        if pairs.is_empty() || pairs.len() % 2 != 0 {
            return Err(ScriptError::type_err(
                "save: NPZ requires alternating name/value pairs after the filename".to_string(),
            ));
        }
        save_npz(&path, pairs).map_err(|e| ScriptError::runtime(e))?;
    } else if path.ends_with(".toml") {
        if args.len() != 2 {
            return Err(ScriptError::type_err(
                "save: TOML format takes exactly one value (struct)".to_string(),
            ));
        }
        super::toml_io::save_toml(&path, &args[1]).map_err(|e| ScriptError::runtime(e))?;
    } else if path.ends_with(".csv") {
        if args.len() != 2 {
            return Err(ScriptError::type_err(
                "save: CSV format takes exactly one value".to_string(),
            ));
        }
        write_csv(&path, &args[1]).map_err(|e| ScriptError::runtime(e))?;
    } else {
        // .npy (or any other extension — default to NPY)
        if args.len() != 2 {
            return Err(ScriptError::type_err(
                "save: NPY format takes exactly one value".to_string(),
            ));
        }
        let (data, shape) = value_to_c64_array(&args[1]).map_err(|e| ScriptError::runtime(e))?;
        let bytes = build_npy_bytes(&data, &shape);
        std::fs::write(&path, bytes).map_err(|e| ScriptError::runtime(e.to_string()))?;
    }
    Ok(Value::None)
}

fn builtin_load(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("load", &args, 1, 2)?;
    let path = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;

    if path.ends_with(".npz") {
        if args.len() != 2 {
            return Err(ScriptError::type_err(
                "load: to load all variables use bare load(\"file.npz\") without assignment;\n  to extract one use: x = load(\"file.npz\", \"varname\")".to_string()
            ));
        }
        let name = args[1].to_str().map_err(|e| ScriptError::type_err(e))?;
        load_from_npz(&path, &name).map_err(|e| ScriptError::runtime(e))
    } else if path.ends_with(".toml") {
        super::toml_io::load_toml(&path).map_err(|e| ScriptError::runtime(e))
    } else if path.ends_with(".csv") {
        load_csv(&path).map_err(|e| ScriptError::runtime(e))
    } else {
        // .npy or any other extension
        let bytes = std::fs::read(&path).map_err(|e| ScriptError::runtime(e.to_string()))?;
        parse_npy_bytes(&bytes).map_err(|e| ScriptError::runtime(e))
    }
}

// ─── Matrix construction ───────────────────────────────────────────────────

/// eye(n) — n×n identity matrix
fn builtin_eye(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("eye", &args, 1)?;
    let n = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let mut m: CMatrix = Array2::zeros((n, n));
    for i in 0..n {
        m[[i, i]] = Complex::new(1.0, 0.0);
    }
    Ok(Value::Matrix(m))
}

// ─── Matrix operations ─────────────────────────────────────────────────────

/// transpose(A) — non-conjugate transpose (function form of `.'`)
fn builtin_transpose(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("transpose", &args, 1)?;
    // For sparse matrices, use non-conjugate transpose directly
    args.into_iter()
        .next()
        .unwrap()
        .non_conj_transpose()
        .map_err(|e| ScriptError::type_err(e))
}

/// diag(v)    — create diagonal matrix from vector v
/// diag(M)    — extract main diagonal of matrix M as a vector
/// diag(M, k) — extract k-th diagonal (k>0 superdiagonal, k<0 subdiagonal)
fn builtin_diag(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("diag", &args, 1, 2)?;
    let k: i64 = if args.len() == 2 {
        args[1].to_scalar().map_err(|e| ScriptError::type_err(e))? as i64
    } else {
        0
    };

    match &args[0] {
        Value::Vector(v) => {
            // Create diagonal matrix
            let n = v.len();
            let size = n + k.unsigned_abs() as usize;
            let mut m: CMatrix = Array2::zeros((size, size));
            for (i, &val) in v.iter().enumerate() {
                let (r, c) = if k >= 0 {
                    (i, i + k as usize)
                } else {
                    (i + (-k) as usize, i)
                };
                m[[r, c]] = val;
            }
            Ok(Value::Matrix(m))
        }
        Value::Matrix(m) => {
            // Extract diagonal
            let nrows = m.nrows() as i64;
            let ncols = m.ncols() as i64;
            let len = if k >= 0 {
                (ncols - k).max(0).min(nrows) as usize
            } else {
                (nrows + k).max(0).min(ncols) as usize
            };
            let diag: CVector = Array1::from_iter((0..len).map(|i| {
                let (r, c) = if k >= 0 {
                    (i, i + k as usize)
                } else {
                    (i + (-k) as usize, i)
                };
                m[[r, c]]
            }));
            Ok(Value::Vector(diag))
        }
        other => Err(ScriptError::type_err(format!(
            "diag: expected vector or matrix, got {}",
            other.type_name()
        ))),
    }
}

/// trace(M) — sum of main diagonal
fn builtin_trace(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("trace", &args, 1)?;
    match &args[0] {
        Value::Matrix(m) => {
            let n = m.nrows().min(m.ncols());
            let t: C64 = (0..n).map(|i| m[[i, i]]).sum();
            if t.im.abs() < 1e-12 {
                Ok(Value::Scalar(t.re))
            } else {
                Ok(Value::Complex(t))
            }
        }
        Value::Scalar(n) => Ok(Value::Scalar(*n)),
        other => Err(ScriptError::type_err(format!(
            "trace: expected matrix, got {}",
            other.type_name()
        ))),
    }
}

/// reshape(A, m, n) — reshape A (vector or matrix) into an m×n matrix (column-major order)
fn builtin_reshape(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("reshape", &args, 3, 4)?;
    let m = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let n = args[2].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let p = if args.len() == 4 {
        Some(args[3].to_usize().map_err(|e| ScriptError::type_err(e))?)
    } else {
        None
    };
    // Flatten source in column-major order (Octave convention).
    let flat: Vec<C64> = match &args[0] {
        Value::Vector(v) => v.iter().copied().collect(),
        Value::Matrix(mat) => (0..mat.ncols())
            .flat_map(|c| (0..mat.nrows()).map(move |r| mat[[r, c]]))
            .collect(),
        Value::Tensor3(t) => {
            // Column-major walk over (rows, cols, pages): k outer, j middle, i inner.
            let (sm, sn, sp) = (t.shape()[0], t.shape()[1], t.shape()[2]);
            let mut out = Vec::with_capacity(sm * sn * sp);
            for k in 0..sp {
                for j in 0..sn {
                    for i in 0..sm {
                        out.push(t[[i, j, k]]);
                    }
                }
            }
            out
        }
        Value::Scalar(s) => vec![Complex::new(*s, 0.0)],
        Value::Complex(c) => vec![*c],
        other => {
            return Err(ScriptError::type_err(format!(
                "reshape: cannot reshape {}",
                other.type_name()
            )))
        }
    };
    let total = match p {
        Some(pv) => m * n * pv,
        None => m * n,
    };
    if flat.len() != total {
        return Err(ScriptError::type_err(format!(
            "reshape: cannot reshape {} elements into {}{}{} (= {} elements)",
            flat.len(),
            m,
            p.map(|_| format!("×{n}")).unwrap_or(format!("×{n}")),
            p.map(|pv| format!("×{pv}")).unwrap_or_default(),
            total
        )));
    }
    if let Some(pv) = p {
        // Fill Tensor3 column-major: index in `flat` is (k*m*n + j*m + i).
        let mut t = Array3::<C64>::zeros((m, n, pv));
        for k in 0..pv {
            for j in 0..n {
                for i in 0..m {
                    t[[i, j, k]] = flat[k * m * n + j * m + i];
                }
            }
        }
        return Ok(Value::Tensor3(t));
    }
    if m == 1 || n == 1 {
        Ok(Value::Vector(Array1::from_vec(flat)))
    } else {
        // Build matrix column-major: element [i*n+j] comes from flat[r + c*m] in col-major
        let mut mat: CMatrix = Array2::zeros((m, n));
        for c in 0..n {
            for r in 0..m {
                mat[[r, c]] = flat[r + c * m];
            }
        }
        Ok(Value::Matrix(mat))
    }
}

/// repmat(A, m, n) — tile matrix A m times vertically, n times horizontally
fn builtin_repmat(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("repmat", &args, 3)?;
    let reps_r = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let reps_c = args[2].to_usize().map_err(|e| ScriptError::type_err(e))?;
    // Normalise to a matrix block
    let block: CMatrix = match &args[0] {
        Value::Matrix(m) => m.clone(),
        Value::Vector(v) => {
            let n = v.len();
            let data: Vec<C64> = v.iter().copied().collect();
            Array2::from_shape_vec((1, n), data)
                .map_err(|e| ScriptError::type_err(e.to_string()))?
        }
        Value::Scalar(s) => Array2::from_elem((1, 1), Complex::new(*s, 0.0)),
        Value::Complex(c) => Array2::from_elem((1, 1), *c),
        other => {
            return Err(ScriptError::type_err(format!(
                "repmat: cannot tile {}",
                other.type_name()
            )))
        }
    };
    let br = block.nrows();
    let bc = block.ncols();
    let out_r = br * reps_r;
    let out_c = bc * reps_c;
    let mut out: CMatrix = Array2::zeros((out_r, out_c));
    for ri in 0..reps_r {
        for ci in 0..reps_c {
            let r0 = ri * br;
            let c0 = ci * bc;
            for r in 0..br {
                for c in 0..bc {
                    out[[r0 + r, c0 + c]] = block[[r, c]];
                }
            }
        }
    }
    Ok(Value::Matrix(out))
}

/// horzcat(A, B, ...) — horizontal concatenation (same as [A B])
fn builtin_horzcat(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() {
        return Ok(Value::Vector(Array1::zeros(0)));
    }
    Value::from_matrix_rows(vec![args]).map_err(|e| ScriptError::type_err(e))
}

/// vertcat(A, B, ...) — vertical concatenation (same as [A; B])
fn builtin_vertcat(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() {
        return Ok(Value::Vector(Array1::zeros(0)));
    }
    Value::from_matrix_rows(args.into_iter().map(|v| vec![v]).collect())
        .map_err(|e| ScriptError::type_err(e))
}

/// cat(dim, A, B, ...) — concatenate along dimension `dim`. `dim` is 1 (rows),
/// 2 (cols), or 3 (pages). For dim=3, matrices become pages of a tensor3.
fn builtin_cat(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() {
        return Err(ScriptError::type_err(
            "cat: expected cat(dim, A, B, ...), got no arguments".to_string(),
        ));
    }
    let dim = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let rest: Vec<Value> = args.into_iter().skip(1).collect();
    if rest.is_empty() {
        return Err(ScriptError::type_err(
            "cat: expected at least one value to concatenate after dim".to_string(),
        ));
    }
    match dim {
        1 => builtin_vertcat(rest),
        2 => builtin_horzcat(rest),
        3 => {
            // All inputs must have matching (rows, cols); stack along page axis.
            // Accept Matrix or Tensor3 inputs (with matching (rows, cols)).
            let mut m = 0usize;
            let mut n = 0usize;
            let mut total_pages = 0usize;
            for (idx, v) in rest.iter().enumerate() {
                let (vm, vn, vp) = match v {
                    Value::Matrix(mat) => (mat.nrows(), mat.ncols(), 1),
                    Value::Tensor3(t) => (t.shape()[0], t.shape()[1], t.shape()[2]),
                    Value::Vector(vec) => (1, vec.len(), 1),
                    Value::Scalar(_) | Value::Complex(_) => (1, 1, 1),
                    other => {
                        return Err(ScriptError::type_err(format!(
                            "cat(3, ...): argument {} has type {}",
                            idx + 2,
                            other.type_name()
                        )))
                    }
                };
                if idx == 0 {
                    m = vm;
                    n = vn;
                } else if vm != m || vn != n {
                    return Err(ScriptError::type_err(format!(
                        "cat(3, ...): argument {} is {}×{}, expected {}×{}",
                        idx + 2,
                        vm,
                        vn,
                        m,
                        n
                    )));
                }
                total_pages += vp;
            }
            let mut out = Array3::<C64>::zeros((m, n, total_pages));
            let mut k0 = 0usize;
            for v in rest.into_iter() {
                match v {
                    Value::Matrix(mat) => {
                        for i in 0..m {
                            for j in 0..n {
                                out[[i, j, k0]] = mat[[i, j]];
                            }
                        }
                        k0 += 1;
                    }
                    Value::Tensor3(t) => {
                        let vp = t.shape()[2];
                        for k in 0..vp {
                            for i in 0..m {
                                for j in 0..n {
                                    out[[i, j, k0 + k]] = t[[i, j, k]];
                                }
                            }
                        }
                        k0 += vp;
                    }
                    Value::Vector(vec) => {
                        for j in 0..n {
                            out[[0, j, k0]] = vec[j];
                        }
                        k0 += 1;
                    }
                    Value::Scalar(s) => {
                        out[[0, 0, k0]] = Complex::new(s, 0.0);
                        k0 += 1;
                    }
                    Value::Complex(c) => {
                        out[[0, 0, k0]] = c;
                        k0 += 1;
                    }
                    _ => unreachable!(),
                }
            }
            Ok(Value::Tensor3(out))
        }
        _ => Err(ScriptError::type_err(format!(
            "cat: dim must be 1, 2, or 3, got {dim}"
        ))),
    }
}

/// permute(A, order) — reorder the axes of a Tensor3. `order` is a 3-element
/// permutation of [1, 2, 3] (1-based axis labels).
fn builtin_permute(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("permute", &args, 2)?;
    let t = match &args[0] {
        Value::Tensor3(t) => t.clone(),
        other => {
            return Err(ScriptError::type_err(format!(
                "permute: expected tensor3, got {}",
                other.type_name()
            )))
        }
    };
    let order: Vec<usize> = match &args[1] {
        Value::Vector(v) if v.len() == 3 => v.iter().map(|c| c.re.round() as usize).collect(),
        other => {
            return Err(ScriptError::type_err(format!(
                "permute: order must be a 3-element vector, got {}",
                other.type_name()
            )))
        }
    };
    // Validate it's a permutation of [1, 2, 3]
    let mut sorted = order.clone();
    sorted.sort();
    if sorted != [1, 2, 3] {
        return Err(ScriptError::type_err(format!(
            "permute: order must be a permutation of [1, 2, 3], got {:?}",
            order
        )));
    }
    let s = t.shape();
    let src = [s[0], s[1], s[2]];
    let dst = [src[order[0] - 1], src[order[1] - 1], src[order[2] - 1]];
    let mut out = Array3::<C64>::zeros((dst[0], dst[1], dst[2]));
    for i in 0..src[0] {
        for j in 0..src[1] {
            for k in 0..src[2] {
                let idx_src = [i, j, k];
                let di = idx_src[order[0] - 1];
                let dj = idx_src[order[1] - 1];
                let dk = idx_src[order[2] - 1];
                out[[di, dj, dk]] = t[[i, j, k]];
            }
        }
    }
    Ok(Value::Tensor3(out))
}

/// squeeze(A) — drop singleton dimensions from a Tensor3.
///  - (m, n, 1) → Matrix(m, n)
///  - (m, 1, p) → Matrix(m, p)
///  - (1, n, p) → Matrix(n, p)
///  - (m, 1, 1) → Vector(m); (1, n, 1) → Vector(n); (1, 1, p) → Vector(p)
///  - (1, 1, 1) → Scalar
/// Non-tensor3 inputs pass through unchanged.
fn builtin_squeeze(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("squeeze", &args, 1)?;
    let t = match args.into_iter().next().unwrap() {
        Value::Tensor3(t) => t,
        other => return Ok(other), // pass-through
    };
    let s = t.shape();
    let (m, n, p) = (s[0], s[1], s[2]);
    let dims = [m, n, p];
    let singletons = dims.iter().filter(|&&d| d == 1).count();

    // Collect data in (i, j, k) row-major (C-order) walk — matches ndarray's
    // default and yields the right Matrix/Vector when the singletons are removed.
    match singletons {
        0 => Ok(Value::Tensor3(t)), // nothing to squeeze
        1 => {
            // Exactly one singleton → Matrix over the two non-singleton axes.
            let (rows, cols, drop_dim) = if m == 1 {
                (n, p, 0usize)
            } else if n == 1 {
                (m, p, 1usize)
            } else {
                (m, n, 2usize)
            };
            let mut out = Array2::<C64>::zeros((rows, cols));
            for i in 0..m {
                for j in 0..n {
                    for k in 0..p {
                        let (r, c) = match drop_dim {
                            0 => (j, k),
                            1 => (i, k),
                            2 => (i, j),
                            _ => unreachable!(),
                        };
                        out[[r, c]] = t[[i, j, k]];
                    }
                }
            }
            Ok(Value::Matrix(out))
        }
        2 => {
            // Two singletons → Vector along the single non-singleton axis.
            let len = if m > 1 {
                m
            } else if n > 1 {
                n
            } else {
                p
            };
            let mut out = Vec::with_capacity(len);
            for i in 0..m {
                for j in 0..n {
                    for k in 0..p {
                        out.push(t[[i, j, k]]);
                    }
                }
            }
            Ok(Value::Vector(Array1::from_vec(out)))
        }
        _ => {
            // All singletons → Scalar / Complex
            let c = t[[0, 0, 0]];
            if c.im.abs() < 1e-12 {
                Ok(Value::Scalar(c.re))
            } else {
                Ok(Value::Complex(c))
            }
        }
    }
}

// ─── Linear algebra ────────────────────────────────────────────────────────

/// dot(u, v) — inner (dot) product of two vectors
fn builtin_dot(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("dot", &args, 2)?;
    // Native sparse dot products
    let result: C64 = match (&args[0], &args[1]) {
        (Value::SparseVector(a), Value::SparseVector(b)) => {
            if a.len != b.len {
                return Err(ScriptError::type_err(format!(
                    "dot: vectors must have the same length ({} vs {})",
                    a.len, b.len
                )));
            }
            a.dot(b)
        }
        (Value::SparseVector(sv), _) => {
            let dv = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
            if sv.len != dv.len() {
                return Err(ScriptError::type_err(format!(
                    "dot: vectors must have the same length ({} vs {})",
                    sv.len,
                    dv.len()
                )));
            }
            sv.dot_dense(&dv)
        }
        (_, Value::SparseVector(sv)) => {
            let dv = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
            if dv.len() != sv.len {
                return Err(ScriptError::type_err(format!(
                    "dot: vectors must have the same length ({} vs {})",
                    dv.len(),
                    sv.len
                )));
            }
            sv.dot_dense(&dv)
        }
        _ => {
            let u = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
            let v = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
            if u.len() != v.len() {
                return Err(ScriptError::type_err(format!(
                    "dot: vectors must have the same length ({} vs {})",
                    u.len(),
                    v.len()
                )));
            }
            u.iter().zip(v.iter()).map(|(&a, &b)| a * b).sum()
        }
    };
    if result.im.abs() < 1e-12 {
        Ok(Value::Scalar(result.re))
    } else {
        Ok(Value::Complex(result))
    }
}

/// cross(u, v) — 3D cross product
fn builtin_cross(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("cross", &args, 2)?;
    let u = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let v = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    if u.len() != 3 || v.len() != 3 {
        return Err(ScriptError::type_err(format!(
            "cross: both vectors must have length 3 (got {} and {})",
            u.len(),
            v.len()
        )));
    }
    let result = Array1::from_vec(vec![
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]);
    Ok(Value::Vector(result))
}

/// outer(a, b) — outer product of two vectors, returning an N×M matrix.
/// outer(a, b)[i, j] = a[i] * b[j]
fn builtin_outer(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("outer", &args, 2)?;
    let a = args[0]
        .to_cvector()
        .map_err(|e| ScriptError::type_err(format!("outer: a: {}", e)))?;
    let b = args[1]
        .to_cvector()
        .map_err(|e| ScriptError::type_err(format!("outer: b: {}", e)))?;
    let m = Array2::from_shape_fn((a.len(), b.len()), |(i, j)| a[i] * b[j]);
    Ok(Value::Matrix(m))
}

/// kron(A, B) — Kronecker tensor product.
/// For A (m×n) and B (p×q), returns an mp×nq matrix where block (i,j) = A[i,j]*B.
fn builtin_kron(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("kron", &args, 2)?;
    // Accept matrix or scalar (treat scalar as 1×1)
    let a = to_cmatrix_arg(&args[0], "kron", "A")?;
    let b = to_cmatrix_arg(&args[1], "kron", "B")?;
    let (ma, na) = (a.nrows(), a.ncols());
    let (mb, nb) = (b.nrows(), b.ncols());
    let result = Array2::from_shape_fn((ma * mb, na * nb), |(r, c)| {
        a[[r / mb, c / nb]] * b[[r % mb, c % nb]]
    });
    Ok(Value::Matrix(result))
}

/// norm(v)    — Euclidean (L2) norm of a vector, or Frobenius norm of a matrix
/// norm(v, p) — p-norm (p=1 or p=2 supported; p="fro" for Frobenius)
fn builtin_norm(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("norm", &args, 1, 2)?;
    match &args[0] {
        Value::Vector(v) => {
            let p: f64 = if args.len() == 2 {
                args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?
            } else {
                2.0
            };
            let n = if p == 1.0 {
                v.iter().map(|c| c.norm()).sum::<f64>()
            } else if p == 2.0 {
                v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt()
            } else if p == f64::INFINITY {
                v.iter().map(|c| c.norm()).fold(0.0_f64, f64::max)
            } else {
                v.iter()
                    .map(|c| c.norm().powf(p))
                    .sum::<f64>()
                    .powf(1.0 / p)
            };
            Ok(Value::Scalar(n))
        }
        Value::Matrix(m) => {
            // Frobenius norm by default
            let n = m.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
            Ok(Value::Scalar(n))
        }
        Value::Scalar(n) => Ok(Value::Scalar(n.abs())),
        Value::Complex(c) => Ok(Value::Scalar(c.norm())),
        Value::SparseVector(sv) => {
            let p: f64 = if args.len() == 2 {
                args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?
            } else {
                2.0
            };
            let n = if p == 1.0 {
                sv.entries.iter().map(|(_, c)| c.norm()).sum::<f64>()
            } else if p == 2.0 {
                sv.entries
                    .iter()
                    .map(|(_, c)| c.norm_sqr())
                    .sum::<f64>()
                    .sqrt()
            } else if p == f64::INFINITY {
                sv.entries
                    .iter()
                    .map(|(_, c)| c.norm())
                    .fold(0.0_f64, f64::max)
            } else {
                sv.entries
                    .iter()
                    .map(|(_, c)| c.norm().powf(p))
                    .sum::<f64>()
                    .powf(1.0 / p)
            };
            Ok(Value::Scalar(n))
        }
        Value::SparseMatrix(sm) => {
            let p: f64 = if args.len() == 2 {
                args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?
            } else {
                2.0
            };
            if p == 1.0 {
                // Max absolute column sum
                let mut col_sums = vec![0.0_f64; sm.cols];
                for &(_, c, v) in &sm.entries {
                    col_sums[c] += v.norm();
                }
                Ok(Value::Scalar(col_sums.into_iter().fold(0.0_f64, f64::max)))
            } else if p == f64::INFINITY {
                // Max absolute row sum
                let mut row_sums = vec![0.0_f64; sm.rows];
                for &(r, _, v) in &sm.entries {
                    row_sums[r] += v.norm();
                }
                Ok(Value::Scalar(row_sums.into_iter().fold(0.0_f64, f64::max)))
            } else {
                // Frobenius for p=2, or convert to dense for other p
                let n = sm
                    .entries
                    .iter()
                    .map(|(_, _, c)| c.norm_sqr())
                    .sum::<f64>()
                    .sqrt();
                Ok(Value::Scalar(n))
            }
        }
        other => Err(ScriptError::type_err(format!(
            "norm: unsupported type {}",
            other.type_name()
        ))),
    }
}

/// LU decomposition with partial pivoting.
/// Returns (L*U after in-place elimination, sign of permutation).
fn lu_decompose(m: &CMatrix) -> (CMatrix, C64) {
    let n = m.nrows();
    let mut lu = m.to_owned();
    let mut sign = Complex::new(1.0, 0.0);
    for k in 0..n {
        // Partial pivoting
        let mut max_idx = k;
        let mut max_val = lu[[k, k]].norm();
        for i in k + 1..n {
            let v = lu[[i, k]].norm();
            if v > max_val {
                max_val = v;
                max_idx = i;
            }
        }
        if max_idx != k {
            for j in 0..n {
                let tmp = lu[[k, j]];
                lu[[k, j]] = lu[[max_idx, j]];
                lu[[max_idx, j]] = tmp;
            }
            sign = -sign;
        }
        let pivot = lu[[k, k]];
        if pivot.norm() < 1e-14 {
            continue;
        }
        for i in k + 1..n {
            lu[[i, k]] /= pivot;
            for j in k + 1..n {
                let sub = lu[[i, k]] * lu[[k, j]];
                lu[[i, j]] -= sub;
            }
        }
    }
    (lu, sign)
}

/// det(M) — determinant of a square matrix
fn builtin_det(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("det", &args, 1)?;
    match &args[0] {
        Value::Matrix(m) => {
            let n = m.nrows();
            if n != m.ncols() {
                return Err(ScriptError::type_err(format!(
                    "det: matrix must be square (got {}×{})",
                    n,
                    m.ncols()
                )));
            }
            if n == 0 {
                return Ok(Value::Scalar(1.0));
            }
            if n == 1 {
                let c = m[[0, 0]];
                return if c.im.abs() < 1e-12 {
                    Ok(Value::Scalar(c.re))
                } else {
                    Ok(Value::Complex(c))
                };
            }
            let (lu, sign) = lu_decompose(m);
            let d: C64 = sign * (0..n).map(|i| lu[[i, i]]).product::<C64>();
            if d.im.abs() < 1e-12 {
                Ok(Value::Scalar(d.re))
            } else {
                Ok(Value::Complex(d))
            }
        }
        Value::Scalar(n) => Ok(Value::Scalar(*n)),
        other => Err(ScriptError::type_err(format!(
            "det: expected matrix, got {}",
            other.type_name()
        ))),
    }
}

/// inv(M) — inverse of a square matrix via Gauss-Jordan elimination
fn matrix_inv(m: &CMatrix) -> Result<CMatrix, String> {
    let n = m.nrows();
    if n != m.ncols() {
        return Err(format!(
            "inv: matrix must be square (got {}×{})",
            n,
            m.ncols()
        ));
    }
    // Augmented [A | I]
    let mut aug: Array2<C64> = Array2::zeros((n, 2 * n));
    for i in 0..n {
        for j in 0..n {
            aug[[i, j]] = m[[i, j]];
        }
        aug[[i, n + i]] = Complex::new(1.0, 0.0);
    }
    for k in 0..n {
        // Pivot
        let mut max_idx = k;
        let mut max_val = aug[[k, k]].norm();
        for i in k + 1..n {
            let v = aug[[i, k]].norm();
            if v > max_val {
                max_val = v;
                max_idx = i;
            }
        }
        if max_idx != k {
            for j in 0..2 * n {
                let tmp = aug[[k, j]];
                aug[[k, j]] = aug[[max_idx, j]];
                aug[[max_idx, j]] = tmp;
            }
        }
        if aug[[k, k]].norm() < 1e-14 {
            return Err("inv: matrix is singular or nearly singular".to_string());
        }
        let pivot = aug[[k, k]];
        for j in 0..2 * n {
            aug[[k, j]] /= pivot;
        }
        for i in 0..n {
            if i != k {
                let factor = aug[[i, k]];
                for j in 0..2 * n {
                    let sub = factor * aug[[k, j]];
                    aug[[i, j]] -= sub;
                }
            }
        }
    }
    let mut result: CMatrix = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            result[[i, j]] = aug[[i, n + j]];
        }
    }
    Ok(result)
}

fn builtin_inv(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("inv", &args, 1)?;
    match &args[0] {
        Value::Matrix(m) => {
            let result = matrix_inv(m).map_err(|e| ScriptError::type_err(e))?;
            Ok(Value::Matrix(result))
        }
        Value::Scalar(n) => {
            if *n == 0.0 {
                return Err(ScriptError::type_err(
                    "inv: singular (scalar is zero)".to_string(),
                ));
            }
            Ok(Value::Scalar(1.0 / n))
        }
        other => Err(ScriptError::type_err(format!(
            "inv: expected matrix, got {}",
            other.type_name()
        ))),
    }
}

/// expm(M) — matrix exponential e^M via scaling-and-squaring with a degree-6 Padé approximant.
fn builtin_expm(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("expm", &args, 1)?;
    let m = match &args[0] {
        Value::Matrix(m) => m.clone(),
        Value::Scalar(n) => {
            return Ok(Value::Scalar(n.exp()));
        }
        other => {
            return Err(ScriptError::type_err(format!(
                "expm: expected matrix, got {}",
                other.type_name()
            )))
        }
    };
    let n = m.nrows();
    if n != m.ncols() {
        return Err(ScriptError::type_err(format!(
            "expm: matrix must be square (got {}×{})",
            n,
            m.ncols()
        )));
    }
    Ok(Value::Matrix(matrix_expm(&m)))
}

/// Compute e^A for a square complex matrix.
/// Uses scaling-and-squaring with a [6/6] Padé approximant.
/// Coefficients: c_k = m!(2m-k)! / ((2m)! k! (m-k)!) for m=6 — exact rational values.
/// Threshold theta_6 from Higham 2008, Table A.1.
fn matrix_expm(a: &CMatrix) -> CMatrix {
    let n = a.nrows();

    // 1-norm (largest column sum of absolute values)
    let norm1: f64 = (0..n)
        .map(|j| (0..n).map(|i| a[[i, j]].norm()).sum::<f64>())
        .fold(0.0_f64, f64::max);

    // Scale so ||A/2^s||_1 <= theta_6 (Higham 2008, Table A.1)
    let theta_6: f64 = 0.537_192_035_114_815_2;
    let s = if norm1 > theta_6 {
        ((norm1 / theta_6).log2().ceil() as i32).max(0)
    } else {
        0
    };
    let a_s: CMatrix = a.mapv(|c| c / (2.0_f64).powi(s));

    // [6/6] Padé coefficients c_k = 6!(12-k)! / (12! k! (6-k)!)
    let c0: f64 = 1.0; // 1
    let c1: f64 = 0.5; // 1/2
    let c2: f64 = 5.0 / 44.0; // 5/44
    let c3: f64 = 1.0 / 66.0; // 1/66
    let c4: f64 = 1.0 / 792.0; // 1/792
    let c5: f64 = 1.0 / 15840.0; // 1/15840
    let c6: f64 = 1.0 / 665280.0; // 1/665280

    let eye: CMatrix = Array2::eye(n);
    let a2 = a_s.dot(&a_s);
    let a4 = a2.dot(&a2);
    let a6 = a4.dot(&a2);

    // V = c0*I + c2*A² + c4*A⁴ + c6*A⁶  (even)
    let v =
        eye.mapv(|x: C64| x * c0) + a2.mapv(|x| x * c2) + a4.mapv(|x| x * c4) + a6.mapv(|x| x * c6);

    // U = A·(c1*I + c3*A² + c5*A⁴)  (odd, A factored out)
    let inner = eye.mapv(|x: C64| x * c1) + a2.mapv(|x| x * c3) + a4.mapv(|x| x * c5);
    let u = a_s.dot(&inner);

    // expm_s = (V - U)⁻¹ · (U + V)
    let num: CMatrix = &u + &v;
    let den: CMatrix = &v - &u;
    let den_inv = match matrix_inv(&den) {
        Ok(m) => m,
        Err(_) => return Array2::eye(n),
    };
    let mut result = den_inv.dot(&num);

    // Undo scaling by repeated squaring
    for _ in 0..s {
        result = result.dot(&result.clone());
    }
    result
}

/// linsolve(A, b) — solve the linear system A*x = b via Gaussian elimination
fn builtin_linsolve(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("linsolve", &args, 2)?;
    let a = match &args[0] {
        Value::Matrix(m) => m.clone(),
        Value::SparseMatrix(sm) => sm.to_dense(),
        other => {
            return Err(ScriptError::type_err(format!(
                "linsolve: A must be a matrix, got {}",
                other.type_name()
            )))
        }
    };
    let b = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let n = a.nrows();
    if n != a.ncols() {
        return Err(ScriptError::type_err(format!(
            "linsolve: A must be square (got {}×{})",
            n,
            a.ncols()
        )));
    }
    if n != b.len() {
        return Err(ScriptError::type_err(format!(
            "linsolve: A is {}×{} but b has length {}",
            n,
            n,
            b.len()
        )));
    }
    // Augmented [A | b]
    let mut aug: Array2<C64> = Array2::zeros((n, n + 1));
    for i in 0..n {
        for j in 0..n {
            aug[[i, j]] = a[[i, j]];
        }
        aug[[i, n]] = b[i];
    }
    // Forward elimination with partial pivoting
    for k in 0..n {
        let mut max_idx = k;
        let mut max_val = aug[[k, k]].norm();
        for i in k + 1..n {
            let v = aug[[i, k]].norm();
            if v > max_val {
                max_val = v;
                max_idx = i;
            }
        }
        if max_idx != k {
            for j in 0..n + 1 {
                let tmp = aug[[k, j]];
                aug[[k, j]] = aug[[max_idx, j]];
                aug[[max_idx, j]] = tmp;
            }
        }
        if aug[[k, k]].norm() < 1e-14 {
            return Err(ScriptError::type_err(
                "linsolve: matrix is singular or nearly singular".to_string(),
            ));
        }
        for i in k + 1..n {
            let factor = aug[[i, k]] / aug[[k, k]];
            for j in k..n + 1 {
                let sub = factor * aug[[k, j]];
                aug[[i, j]] -= sub;
            }
        }
    }
    // Back substitution
    let mut x: CVector = Array1::zeros(n);
    for i in (0..n).rev() {
        let mut s = aug[[i, n]];
        for j in i + 1..n {
            s -= aug[[i, j]] * x[j];
        }
        x[i] = s / aug[[i, i]];
    }
    // Return as scalar if 1-element, else vector
    if x.len() == 1 {
        let c = x[0];
        if c.im.abs() < 1e-12 {
            Ok(Value::Scalar(c.re))
        } else {
            Ok(Value::Complex(c))
        }
    } else {
        Ok(Value::Vector(x))
    }
}

// ─── factor(n) ────────────────────────────────────────────────────────────────

// ─── Special functions ────────────────────────────────────────────────────────

/// laguerre(n, alpha, x) — associated Laguerre polynomial L_n^alpha(x).
/// n must be a non-negative integer scalar; alpha is a real scalar.
/// x may be a scalar, vector, or matrix (element-wise).
/// Uses the 3-term recurrence:
///   L_0 = 1,  L_1 = 1 + alpha - x
///   L_{k+1} = ((2k+1+alpha-x)*L_k - (k+alpha)*L_{k-1}) / (k+1)
fn builtin_laguerre(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("laguerre", &args, 3)?;
    let n = args[0].to_scalar().map_err(|_| {
        ScriptError::type_err("laguerre: n must be a non-negative integer scalar".to_string())
    })?;
    let n = n.round() as i64;
    if n < 0 {
        return Err(ScriptError::type_err(
            "laguerre: n must be non-negative".to_string(),
        ));
    }
    let alpha = args[1]
        .to_scalar()
        .map_err(|_| ScriptError::type_err("laguerre: alpha must be a real scalar".to_string()))?;

    fn laguerre_scalar(n: i64, alpha: f64, x: f64) -> f64 {
        if n == 0 {
            return 1.0;
        }
        if n == 1 {
            return 1.0 + alpha - x;
        }
        let (mut lk_1, mut lk) = (1.0_f64, 1.0 + alpha - x);
        for k in 1..n {
            let next = ((2 * k + 1) as f64 + alpha - x) * lk - (k as f64 + alpha) * lk_1;
            let next = next / (k + 1) as f64;
            lk_1 = lk;
            lk = next;
        }
        lk
    }

    match &args[2] {
        Value::Scalar(x) => Ok(Value::Scalar(laguerre_scalar(n, alpha, *x))),
        Value::Complex(c) => Ok(Value::Scalar(laguerre_scalar(n, alpha, c.re))),
        Value::Vector(v) => {
            let result: CVector = v.mapv(|c| Complex::new(laguerre_scalar(n, alpha, c.re), 0.0));
            Ok(Value::Vector(result))
        }
        Value::Matrix(m) => {
            let result: CMatrix = m.mapv(|c| Complex::new(laguerre_scalar(n, alpha, c.re), 0.0));
            Ok(Value::Matrix(result))
        }
        other => Err(ScriptError::type_err(format!(
            "laguerre: x must be scalar/vector/matrix, got {}",
            other.type_name()
        ))),
    }
}

/// legendre(l, m, x) — associated Legendre polynomial P_l^m(x).
/// l, m must be non-negative integer scalars with 0 <= m <= l.
/// x may be a scalar, vector, or matrix (element-wise); typically |x| <= 1.
/// Uses Condon-Shortley phase convention. Recurrence:
///   P_m^m(x) = (-1)^m (2m-1)!! (1-x²)^(m/2)
///   P_{m+1}^m(x) = x (2m+1) P_m^m(x)
///   P_l^m(x) = ((2l-1) x P_{l-1}^m - (l+m-1) P_{l-2}^m) / (l-m)
fn builtin_legendre(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("legendre", &args, 3)?;
    let l = args[0]
        .to_scalar()
        .map_err(|_| {
            ScriptError::type_err("legendre: l must be a non-negative integer scalar".to_string())
        })?
        .round() as i64;
    let m = args[1]
        .to_scalar()
        .map_err(|_| ScriptError::type_err("legendre: m must be an integer scalar".to_string()))?
        .round() as i64;
    if l < 0 || m.abs() > l {
        return Err(ScriptError::type_err(format!(
            "legendre: require 0 <= l and |m| <= l (got l={}, m={})",
            l, m
        )));
    }

    fn legendre_scalar(l: i64, m: i64, x: f64) -> f64 {
        // Handle negative m via symmetry: P_l^{-m} = (-1)^m (l-m)!/(l+m)! P_l^m
        let (l_use, m_use, negate) = if m < 0 {
            let sign = if m % 2 == 0 { 1.0_f64 } else { -1.0_f64 };
            let m_pos = m.unsigned_abs() as i64;
            // factorial ratio (l-m_pos)!/(l+m_pos)!
            let mut ratio = 1.0_f64;
            for k in (l - m_pos + 1)..=(l + m_pos) {
                ratio /= k as f64;
            }
            (l, m_pos, sign * ratio)
        } else {
            (l, m, 1.0_f64)
        };

        // Seed: P_{m_use}^{m_use}
        let sin_th = (1.0 - x * x).max(0.0).sqrt();
        let mut pmm = 1.0_f64;
        // (2k-1)!! * sin^m: build iteratively
        for k in 1..=m_use {
            pmm *= -(2 * k - 1) as f64 * sin_th;
        }

        if l_use == m_use {
            return negate * pmm;
        }

        // P_{m_use+1}^{m_use}
        let mut pmm1 = x * (2 * m_use + 1) as f64 * pmm;
        if l_use == m_use + 1 {
            return negate * pmm1;
        }

        // Recurrence up to l
        let mut pll = 0.0_f64;
        for ll in (m_use + 2)..=l_use {
            pll = ((2 * ll - 1) as f64 * x * pmm1 - (ll + m_use - 1) as f64 * pmm)
                / (ll - m_use) as f64;
            pmm = pmm1;
            pmm1 = pll;
        }
        negate * pll
    }

    match &args[2] {
        Value::Scalar(x) => Ok(Value::Scalar(legendre_scalar(l, m, *x))),
        Value::Complex(c) => Ok(Value::Scalar(legendre_scalar(l, m, c.re))),
        Value::Vector(v) => {
            let result: CVector = v.mapv(|c| Complex::new(legendre_scalar(l, m, c.re), 0.0));
            Ok(Value::Vector(result))
        }
        Value::Matrix(mx) => {
            let result: CMatrix = mx.mapv(|c| Complex::new(legendre_scalar(l, m, c.re), 0.0));
            Ok(Value::Matrix(result))
        }
        other => Err(ScriptError::type_err(format!(
            "legendre: x must be scalar/vector/matrix, got {}",
            other.type_name()
        ))),
    }
}

/// factor(n) — prime factorization of a positive integer.
/// Returns a real Vector of prime factors in ascending order (with repetition).
/// factor(12) → [2, 2, 3],  factor(17) → [17],  factor(1) → []
fn builtin_factor(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("factor", &args, 1)?;
    let n_f = match &args[0] {
        Value::Scalar(n) => *n,
        other => {
            return Err(ScriptError::type_err(format!(
                "factor: expected a positive integer scalar, got {}",
                other.type_name()
            )))
        }
    };
    if n_f <= 0.0 || n_f.fract() != 0.0 {
        return Err(ScriptError::type_err(format!(
            "factor: argument must be a positive integer, got {}",
            n_f
        )));
    }
    let mut n = n_f as u64;
    let mut factors: Vec<C64> = Vec::new();
    let mut d = 2u64;
    while d * d <= n {
        while n % d == 0 {
            factors.push(Complex::new(d as f64, 0.0));
            n /= d;
        }
        d += 1;
    }
    if n > 1 {
        factors.push(Complex::new(n as f64, 0.0));
    }
    Ok(Value::Vector(Array1::from_vec(factors)))
}

// ─── eig(M) ───────────────────────────────────────────────────────────────────

/// Reduce a square matrix to upper Hessenberg form via Householder reflectors.
/// Returns H such that H has the same eigenvalues as the input.
fn hessenberg_reduce(a: &CMatrix) -> CMatrix {
    let n = a.nrows();
    let mut h = a.to_owned();
    for k in 0..n.saturating_sub(2) {
        // Build Householder vector from column k below the subdiagonal
        let col_len = n - k - 1;
        let mut x: Vec<C64> = (0..col_len).map(|i| h[[k + 1 + i, k]]).collect();
        let norm_x: f64 = x.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        if norm_x < 1e-15 {
            continue;
        }
        // Phase of first element
        let phase = if x[0].norm() < 1e-15 {
            Complex::new(1.0, 0.0)
        } else {
            x[0] / x[0].norm()
        };
        x[0] += phase * norm_x;
        let norm_v: f64 = x.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        if norm_v < 1e-15 {
            continue;
        }
        for c in &mut x {
            *c /= norm_v;
        }
        // H = (I - 2 v v*) H (I - 2 v v*)  — apply from left then right
        // Left: H[k+1:, k:] -= 2 * v * (v* H[k+1:, k:])
        for j in k..n {
            let dot: C64 = x
                .iter()
                .enumerate()
                .map(|(i, vi)| vi.conj() * h[[k + 1 + i, j]])
                .sum();
            for i in 0..col_len {
                h[[k + 1 + i, j]] -= 2.0 * x[i] * dot;
            }
        }
        // Right: H[:, k+1:] -= 2 * (H[:, k+1:] v) v*
        for i in 0..n {
            let dot: C64 = x
                .iter()
                .enumerate()
                .map(|(j, vj)| h[[i, k + 1 + j]] * *vj)
                .sum();
            for j in 0..col_len {
                h[[i, k + 1 + j]] -= 2.0 * dot * x[j].conj();
            }
        }
    }
    h
}

/// Compute eigenvalues of an upper Hessenberg matrix using shifted QR iteration.
/// Uses complex Wilkinson shifts for reliable convergence.
/// Returns eigenvalues as a Vec<C64>.
fn eig_hessenberg(h_in: &CMatrix) -> Result<Vec<C64>, String> {
    let n = h_in.nrows();
    if n == 0 {
        return Ok(vec![]);
    }
    if n == 1 {
        return Ok(vec![h_in[[0, 0]]]);
    }
    if n == 2 {
        // Direct 2×2 formula
        let a = h_in[[0, 0]];
        let b = h_in[[0, 1]];
        let c = h_in[[1, 0]];
        let d = h_in[[1, 1]];
        let tr = a + d;
        let det = a * d - b * c;
        let disc = (tr * tr - 4.0 * det).sqrt();
        return Ok(vec![(tr + disc) / 2.0, (tr - disc) / 2.0]);
    }

    let mut h = h_in.to_owned();
    let mut eigenvalues: Vec<C64> = Vec::with_capacity(n);
    let max_iter_per = 100; // iterations per eigenvalue
    let mut p = n; // active size: working on h[0..p, 0..p]

    while p > 0 {
        if p == 1 {
            eigenvalues.push(h[[0, 0]]);
            break;
        }
        if p == 2 {
            let a = h[[0, 0]];
            let b = h[[0, 1]];
            let c = h[[1, 0]];
            let d = h[[1, 1]];
            let tr = a + d;
            let det = a * d - b * c;
            let disc = (tr * tr - 4.0 * det).sqrt();
            eigenvalues.push((tr + disc) / 2.0);
            eigenvalues.push((tr - disc) / 2.0);
            break;
        }

        let mut converged = false;
        for _iter in 0..max_iter_per {
            let q = p - 1;

            // ── Deflation check ────────────────────────────────────────────
            // Check all subdiagonals for small values (from bottom up)
            let mut split_at = None;
            for i in (1..p).rev() {
                let tol = 1e-12 * (h[[i - 1, i - 1]].norm() + h[[i, i]].norm());
                if h[[i, i - 1]].norm() <= tol {
                    h[[i, i - 1]] = Complex::new(0.0, 0.0);
                    split_at = Some(i);
                    break;
                }
            }
            if let Some(i) = split_at {
                if i == q {
                    // Single eigenvalue deflated at bottom
                    eigenvalues.push(h[[q, q]]);
                    p -= 1;
                    converged = true;
                    break;
                } else if i == q - 1 {
                    // 2×2 block at bottom
                    let a = h[[q - 1, q - 1]];
                    let b = h[[q - 1, q]];
                    let c = h[[q, q - 1]];
                    let d = h[[q, q]];
                    let tr = a + d;
                    let det = a * d - b * c;
                    let disc = (tr * tr - 4.0 * det).sqrt();
                    eigenvalues.push((tr + disc) / 2.0);
                    eigenvalues.push((tr - disc) / 2.0);
                    p -= 2;
                    converged = true;
                    break;
                } else {
                    // Split: recursively handle upper part later, reduce p for lower
                    // For simplicity, just continue working on 0..p
                    // (next iteration will check deflation again)
                }
            }
            if converged {
                break;
            }

            // ── Wilkinson shift: eigenvalue of bottom 2×2 closest to h[q,q] ──
            let a = h[[q - 1, q - 1]];
            let b = h[[q - 1, q]];
            let c = h[[q, q - 1]];
            let d = h[[q, q]];
            let tr2 = a + d;
            let det2 = a * d - b * c;
            let disc = (tr2 * tr2 - 4.0 * det2).sqrt();
            let e1 = (tr2 + disc) / 2.0;
            let e2 = (tr2 - disc) / 2.0;
            // Pick the eigenvalue of the 2×2 closest to h[q,q]
            let shift = if (e1 - d).norm() <= (e2 - d).norm() {
                e1
            } else {
                e2
            };

            // ── Single-shift QR step using Givens rotations ────────────────
            // Apply H ← G_k^* H G_k for k = 0..p-2
            // First rotation eliminates h[1,0] after shift
            let mut x = h[[0, 0]] - shift;
            let mut y = h[[1, 0]];

            for k in 0..p - 1 {
                // Compute Givens rotation [c, s; -s*, c] to zero y using x
                let r = (x.norm_sqr() + y.norm_sqr()).sqrt();
                if r < 1e-15 {
                    continue;
                }
                let gc = x / r;
                let gs = y / r;

                // Left multiply: rows k and k+1, columns k-1..p
                let jstart = if k > 0 { k - 1 } else { 0 };
                for j in jstart..p {
                    let u = h[[k, j]];
                    let v = h[[k + 1, j]];
                    h[[k, j]] = gc.conj() * u + gs.conj() * v;
                    h[[k + 1, j]] = -gs * u + gc * v;
                }
                // Right multiply: rows 0..p, columns k and k+1
                // (only need rows 0..min(k+3, p) for Hessenberg, but use p for correctness)
                let iend = (k + 3).min(p);
                for i in 0..iend {
                    let u = h[[i, k]];
                    let v = h[[i, k + 1]];
                    h[[i, k]] = gc * u + gs * v;
                    h[[i, k + 1]] = -gs.conj() * u + gc.conj() * v;
                }

                // Next iteration uses the subdiagonal entry created
                if k + 1 < p - 1 {
                    x = h[[k + 1, k]];
                    y = h[[k + 2, k]];
                }
            }
        }

        if !converged {
            // Force deflation at the bottom even if not fully converged
            // (prevents infinite loop — take best approximation)
            eigenvalues.push(h[[p - 1, p - 1]]);
            p -= 1;
        }
    }

    Ok(eigenvalues)
}

/// eig(M) — eigenvalues of a square matrix.
/// Returns a complex Vector of length n.
/// Nargout-aware dispatch for `eig`:
///   nargout == 1 (default): return an `N×1` column matrix of eigenvalues.
///   nargout >= 2: return `[V, D]` — V is the `N×N` matrix of eigenvectors
///                 (one per column), D is a diagonal `N×N` matrix of
///                 eigenvalues (matlab convention; `diag(D)` extracts the
///                 vector). The eigenvalues of `D(k, k)` correspond to
///                 column `V(:, k)`.
/// Optional output-form override for the eig family. `eig(_, "vector")` /
/// `eig(_, "matrix")` (matlab convention) lets callers force D's shape
/// independent of nargout. Without a flag, D defaults to a column vector
/// in 1-output context and a diagonal matrix in 2-output context (matlab).
#[derive(Clone, Copy, PartialEq)]
enum EigDShape {
    Vector,
    Matrix,
}

/// Strip a trailing `Value::Str("vector"|"matrix")` from `args` if present.
/// Returns `(flag, args_without_flag_slice)`. Errors on any other string.
fn parse_eig_d_shape<'a>(
    name: &str,
    args: &'a [Value],
) -> Result<(Option<EigDShape>, &'a [Value]), ScriptError> {
    if let Some(Value::Str(s)) = args.last() {
        let flag = match s.as_str() {
            "vector" => EigDShape::Vector,
            "matrix" => EigDShape::Matrix,
            other => {
                return Err(ScriptError::type_err(format!(
                    "{}: output-form flag must be \"vector\" or \"matrix\", got \"{}\"",
                    name, other
                )))
            }
        };
        Ok((Some(flag), &args[..args.len() - 1]))
    } else {
        Ok((None, args))
    }
}

/// Resolve eig's input(s) to the effective standard-form matrix:
/// - `eig(A)`: returns `A` after a square-shape check.
/// - `eig(A, B)` (generalized): returns `inv(B) · A`. Eigenvalues of this
///   product are the generalized eigenvalues of `A·v = λ·B·v`, and the
///   eigenvector matrix is unchanged. Requires B square, same size as A,
///   and invertible. SPD-aware Cholesky reduction is a future optimization;
///   QZ for non-SPD non-invertible cases is deferred.
fn eig_resolve_input(name: &str, args: &[Value]) -> Result<CMatrix, ScriptError> {
    let extract = |v: &Value, label: &str| -> Result<CMatrix, ScriptError> {
        match v {
            Value::Matrix(m) => {
                if m.nrows() != m.ncols() {
                    return Err(ScriptError::type_err(format!(
                        "{}: {} must be square (got {}×{})",
                        name,
                        label,
                        m.nrows(),
                        m.ncols()
                    )));
                }
                Ok(m.clone())
            }
            Value::Scalar(s) => {
                Ok(Array2::from_shape_fn((1, 1), |_| Complex::new(*s, 0.0)))
            }
            other => Err(ScriptError::type_err(format!(
                "{}: {} must be a square matrix or scalar, got {}",
                name,
                label,
                other.type_name()
            ))),
        }
    };
    let a = extract(&args[0], "A")?;
    if args.len() == 1 {
        return Ok(a);
    }
    let b = extract(&args[1], "B")?;
    if a.nrows() != b.nrows() {
        return Err(ScriptError::type_err(format!(
            "{}: A and B must have the same size (got A: {}×{}, B: {}×{})",
            name,
            a.nrows(),
            a.ncols(),
            b.nrows(),
            b.ncols()
        )));
    }
    let inv_b = matrix_inv(&b).map_err(|e| {
        ScriptError::type_err(format!(
            "{}: B is singular or ill-conditioned ({}); the generalized \
             problem A·v = λ·B·v requires an invertible B",
            name, e
        ))
    })?;
    Ok(inv_b.dot(&a))
}

/// Compute eigenvalues (always) and eigenvectors (when `want_v`) of a square
/// dense complex matrix. One place that owns the Hessenberg-reduce + shifted
/// QR + inverse-iteration pipeline used by every `eig` entry point.
/// `name` is woven into error messages for context.
fn compute_eig_dense(
    name: &str,
    m: &CMatrix,
    want_v: bool,
) -> Result<(Vec<C64>, Option<CMatrix>), ScriptError> {
    if m.nrows() == 0 {
        return Err(ScriptError::type_err(format!(
            "{}: matrix must be non-empty",
            name
        )));
    }
    let h = hessenberg_reduce(m);
    let vals = eig_hessenberg(&h).map_err(|e| ScriptError::runtime(format!("{}: {}", name, e)))?;
    if !want_v {
        return Ok((vals, None));
    }
    let n = vals.len();
    let mut v_mat: CMatrix = Array2::zeros((n, n));
    for (k, &lambda) in vals.iter().enumerate() {
        let v = inverse_iteration_cx(m, lambda, 40).map_err(|e| {
            ScriptError::runtime(format!(
                "{}: eigenvector for λ_{}={} could not be recovered ({}). \
                 The matrix may be defective (repeated eigenvalues with fewer \
                 linearly independent eigenvectors than algebraic multiplicity); \
                 use eig(A) for eigenvalues only.",
                name,
                k + 1,
                lambda,
                e
            ))
        })?;
        for i in 0..n {
            v_mat[[i, k]] = v[i];
        }
    }
    Ok((vals, Some(v_mat)))
}

/// Wrap an eigenvalue list as `Value::Matrix`, either an `N×1` column or an
/// `N×N` diagonal matrix. The flag overrides the default shape; otherwise
/// `default_diag` (set by the caller per its nargout context) decides.
fn eigenvalues_to_value(vals: &[C64], flag: Option<EigDShape>, default_diag: bool) -> Value {
    let want_diag = match flag {
        Some(EigDShape::Matrix) => true,
        Some(EigDShape::Vector) => false,
        None => default_diag,
    };
    let n = vals.len();
    let zero = Complex::new(0.0, 0.0);
    if want_diag {
        Value::Matrix(Array2::from_shape_fn((n, n), |(i, j)| {
            if i == j { vals[i] } else { zero }
        }))
    } else {
        Value::Matrix(Array2::from_shape_fn((n, 1), |(i, _)| vals[i]))
    }
}

fn builtin_eig_nargout(args: Vec<Value>, nargout: usize) -> Result<Value, ScriptError> {
    check_args_range("eig", &args, 1, 3)?;
    let (flag, rest) = parse_eig_d_shape("eig", &args)?;
    if rest.is_empty() {
        return Err(ScriptError::type_err(
            "eig: missing matrix argument".to_string(),
        ));
    }
    let m = eig_resolve_input("eig", rest)?;
    let want_v = nargout >= 2;
    // Default D shape: matrix when destructured to [V, D] (matlab), vector
    // for the bare `e = eig(A)` form. The flag overrides.
    let default_diag = want_v;
    let (vals, v_opt) = compute_eig_dense("eig", &m, want_v)?;
    let d = eigenvalues_to_value(&vals, flag, default_diag);
    match v_opt {
        Some(v_mat) => Ok(Value::Tuple(vec![Value::Matrix(v_mat), d])),
        None => Ok(d),
    }
}

/// `eigs(A, n)` / `eigs(A, n, which)` / `eigs(A, B, n)` / `eigs(A, B, n, which)`
///
/// Sparse partial eigensolver. Returns the `n` eigenvalues selected by
/// `which` (`"sm"` smallest-magnitude, default; `"lm"` largest-magnitude)
/// of the standard problem `A x = λ x` or, when `B` is supplied, the
/// generalized problem `A x = λ B x` (with `B` Hermitian positive
/// definite).
///
/// Returns `[V, D]` where `V` is a dense `n_rows × n` matrix of
/// eigenvectors (one column per eigenvalue) and `D` is a length-`n`
/// vector of eigenvalues — same shape convention as MATLAB / Octave's
/// `eigs` and consistent with the dense `eig`.
fn builtin_eigs(args: Vec<Value>) -> Result<Value, ScriptError> {
    use rustlab_core::sparse_eig::{eigs as core_eigs, eigs_gen as core_eigs_gen, Which};

    check_args_range("eigs", &args, 2, 4)?;

    // Decide between standard `eigs(A, n[, which])` and generalized
    // `eigs(A, B, n[, which])` based on the second argument's type.
    let (a, b_opt, n_arg_idx) = match &args[1] {
        Value::SparseMatrix(b) => (
            // generalized form: A and B both sparse
            unwrap_sparse(&args[0], "eigs", "A")?,
            Some(b),
            2,
        ),
        Value::Matrix(_) => {
            return Err(ScriptError::type_err(
                "eigs: matrix arguments must be sparse — call sparse(A) first".into(),
            ));
        }
        _ => (unwrap_sparse(&args[0], "eigs", "A")?, None, 1),
    };

    if args.len() <= n_arg_idx {
        return Err(ScriptError::type_err(
            "eigs: missing required argument `n` (number of eigenvalues)".into(),
        ));
    }
    let n = args[n_arg_idx]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("eigs: n: {e}")))?;

    let which = if args.len() > n_arg_idx + 1 {
        match &args[n_arg_idx + 1] {
            Value::Str(s) => Which::from_str(s.as_str())
                .map_err(|e| ScriptError::type_err(format!("eigs: {e}")))?,
            other => {
                return Err(ScriptError::type_err(format!(
                    "eigs: which must be a string (\"sm\"|\"lm\"), got {}",
                    other.type_name()
                )));
            }
        }
    } else {
        Which::SmallestMagnitude
    };

    let result = match b_opt {
        Some(b) => core_eigs_gen(a, b, n, which, None)
            .map_err(|e| ScriptError::type_err(format!("eigs: {e}")))?,
        None => core_eigs(a, n, which, None)
            .map_err(|e| ScriptError::type_err(format!("eigs: {e}")))?,
    };

    Ok(Value::Tuple(vec![
        Value::Matrix(result.vectors),
        Value::Vector(result.values),
    ]))
}

fn unwrap_sparse<'a>(
    value: &'a Value,
    fn_name: &str,
    arg_name: &str,
) -> Result<&'a SparseMat, ScriptError> {
    match value {
        Value::SparseMatrix(sm) => Ok(sm),
        other => Err(ScriptError::type_err(format!(
            "{fn_name}: {arg_name} must be a sparse matrix, got {}",
            other.type_name()
        ))),
    }
}

fn builtin_whos_file(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("whos", &args, 1)?;
    let path = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;

    if !path.ends_with(".npz") {
        return Err(ScriptError::type_err(
            "whos: only .npz files are supported (e.g. whos(\"data.npz\"))".to_string(),
        ));
    }

    use std::io::Read;
    use zip::ZipArchive;

    let file = std::fs::File::open(&path).map_err(|e| ScriptError::runtime(e.to_string()))?;
    let mut zip = ZipArchive::new(file).map_err(|e| ScriptError::runtime(e.to_string()))?;

    super::output::script_println(&format!("\n  {:<20} {:<10} {}", "Name", "Type", "Size"));
    super::output::script_println(&format!("  {}", "─".repeat(44)));

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| ScriptError::runtime(e.to_string()))?;
        let raw_name = entry.name().to_string();
        let name = raw_name.trim_end_matches(".npy");

        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| ScriptError::runtime(e.to_string()))?;

        let info = if buf.len() >= 10 && &buf[0..6] == b"\x93NUMPY" {
            let hlen = u16::from_le_bytes([buf[8], buf[9]]) as usize;
            if buf.len() >= 10 + hlen {
                if let Ok(header) = std::str::from_utf8(&buf[10..10 + hlen]) {
                    let dtype = if header.contains("<c16") || header.contains(">c16") {
                        "complex"
                    } else {
                        "real"
                    };
                    let size = match parse_npy_shape(header) {
                        Ok(s) => s
                            .iter()
                            .map(|d| d.to_string())
                            .collect::<Vec<_>>()
                            .join("×"),
                        Err(_) => "?".to_string(),
                    };
                    (dtype.to_string(), size)
                } else {
                    ("?".to_string(), "?".to_string())
                }
            } else {
                ("?".to_string(), "?".to_string())
            }
        } else {
            ("?".to_string(), "?".to_string())
        };

        super::output::script_println(&format!("  {:<20} {:<10} {}", name, info.0, info.1));
    }
    super::output::script_println("");
    Ok(Value::None)
}

// ─── Struct construction and inspection ───────────────────────────────────────

fn builtin_struct(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() % 2 != 0 {
        return Err(ScriptError::runtime(
            "struct() requires an even number of arguments: (field, value, ...)".to_string(),
        ));
    }
    let mut fields = HashMap::new();
    let mut iter = args.into_iter();
    while let (Some(key), Some(val)) = (iter.next(), iter.next()) {
        let name = key.to_str().map_err(|e| ScriptError::runtime(e))?;
        fields.insert(name, val);
    }
    Ok(Value::Struct(fields))
}

// ─── Output builtins ──────────────────────────────────────────────────────────

fn builtin_disp(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("disp", &args, 1)?;
    super::output::script_println(&format!("{}", args[0]));
    Ok(Value::None)
}

fn builtin_fprintf(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() {
        return Err(ScriptError::runtime(
            "fprintf: expected a format string".to_string(),
        ));
    }
    let fmt = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;
    let output = apply_format(&fmt, &args[1..]).map_err(|e| ScriptError::runtime(e))?;
    super::output::script_print(&output);
    Ok(Value::None)
}

fn builtin_sprintf(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() {
        return Err(ScriptError::runtime(
            "sprintf: expected a format string".to_string(),
        ));
    }
    let fmt = args[0].to_str().map_err(|e| ScriptError::type_err(e))?;
    let output = apply_format(&fmt, &args[1..]).map_err(|e| ScriptError::runtime(e))?;
    Ok(Value::Str(output))
}

fn builtin_commas(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() || args.len() > 2 {
        return Err(ScriptError::runtime(
            "commas: expected commas(x) or commas(x, precision)".to_string(),
        ));
    }
    let n = args[0]
        .to_scalar()
        .map_err(|e| ScriptError::type_err(format!("commas: {}", e)))?;
    let s = if args.len() == 2 {
        let p = args[1]
            .to_scalar()
            .map_err(|e| ScriptError::type_err(format!("commas: {}", e)))? as usize;
        insert_commas(&format!("{:.prec$}", n, prec = p))
    } else {
        // Integer if no fractional part, otherwise default float display
        if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
            insert_commas(&format!("{}", n as i64))
        } else {
            insert_commas(&format!("{}", n))
        }
    };
    Ok(Value::Str(s))
}

/// Normalise Rust's `{:e}` exponent to C-style `e+XX` / `e-XX`.
/// e.g. `1.23e4` → `1.23e+04`,  `1e-3` → `1.00e-03`
fn normalise_exp(s: &str) -> String {
    if let Some(e_pos) = s.find('e') {
        let mantissa = &s[..e_pos];
        let exp_str = &s[e_pos + 1..];
        let (sign, digits) = if exp_str.starts_with('-') {
            ("-", &exp_str[1..])
        } else if exp_str.starts_with('+') {
            ("+", &exp_str[1..])
        } else {
            ("+", exp_str)
        };
        // Ensure at least 2 exponent digits
        let exp_num: i32 = digits.parse().unwrap_or(0);
        format!("{}e{}{:02}", mantissa, sign, exp_num)
    } else {
        s.to_string()
    }
}

/// Apply a C-style format string with the given argument slice.
/// Supports: %d %i %f %g %e %s %%   and escape sequences \n \t \\
pub fn apply_format(fmt: &str, args: &[Value]) -> Result<String, String> {
    let mut result = String::new();
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    let mut arg_idx = 0;

    while i < chars.len() {
        // Escape sequences
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                'n' => {
                    result.push('\n');
                    i += 2;
                    continue;
                }
                't' => {
                    result.push('\t');
                    i += 2;
                    continue;
                }
                '\\' => {
                    result.push('\\');
                    i += 2;
                    continue;
                }
                _ => {
                    result.push(chars[i]);
                    i += 1;
                    continue;
                }
            }
        }

        if chars[i] != '%' {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        i += 1; // skip '%'
        if i >= chars.len() {
            return Err("fprintf: trailing '%'".to_string());
        }
        if chars[i] == '%' {
            result.push('%');
            i += 1;
            continue;
        }

        // Parse optional flags, width, precision
        let mut flags = String::new();
        while i < chars.len() && "-+ 0#,".contains(chars[i]) {
            flags.push(chars[i]);
            i += 1;
        }
        let use_commas = flags.contains(',');

        let mut width_str = String::new();
        while i < chars.len() && chars[i].is_ascii_digit() {
            width_str.push(chars[i]);
            i += 1;
        }

        let mut prec_str = String::new();
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                prec_str.push(chars[i]);
                i += 1;
            }
        }

        if i >= chars.len() {
            return Err("fprintf: incomplete format specifier".to_string());
        }
        let spec = chars[i];
        i += 1;

        let arg = args.get(arg_idx).ok_or_else(|| {
            format!(
                "fprintf: not enough arguments (need arg {} for '%{}')",
                arg_idx + 1,
                spec
            )
        })?;
        arg_idx += 1;

        let w = width_str.parse::<usize>().unwrap_or(0);
        let p = prec_str.parse::<usize>().unwrap_or(6);
        let left = flags.contains('-');

        let piece = match spec {
            'd' | 'i' => {
                let n = arg.to_scalar().map_err(|e| format!("fprintf %d: {}", e))? as i64;
                let base = format!("{}", n);
                let base = if use_commas {
                    insert_commas(&base)
                } else {
                    base
                };
                if left {
                    format!("{:<width$}", base, width = w)
                } else {
                    format!("{:>width$}", base, width = w)
                }
            }
            'f' => {
                let n = arg.to_scalar().map_err(|e| format!("fprintf %f: {}", e))?;
                let base = format!("{:.prec$}", n, prec = p);
                let base = if use_commas {
                    insert_commas(&base)
                } else {
                    base
                };
                if left {
                    format!("{:<width$}", base, width = w)
                } else {
                    format!("{:>width$}", base, width = w)
                }
            }
            'e' => {
                let n = arg.to_scalar().map_err(|e| format!("fprintf %e: {}", e))?;
                // Rust's {:e} omits the '+' sign and leading zeros in the exponent;
                // normalise to C-style e+XX / e-XX  (e.g.  1.23e+04)
                let base = format!("{:.prec$e}", n, prec = p);
                let base = normalise_exp(&base);
                let base = if use_commas {
                    insert_commas(&base)
                } else {
                    base
                };
                if left {
                    format!("{:<width$}", base, width = w)
                } else {
                    format!("{:>width$}", base, width = w)
                }
            }
            'g' => {
                let n = arg.to_scalar().map_err(|e| format!("fprintf %g: {}", e))?;
                let base = if n == 0.0 || (n.abs() >= 1e-4 && n.abs() < 1e6) {
                    // Trim trailing zeros like %g
                    let s = format!("{:.prec$}", n, prec = p);
                    s.trim_end_matches('0').trim_end_matches('.').to_string()
                } else {
                    let s = format!("{:.prec$e}", n, prec = p);
                    s
                };
                let base = if use_commas {
                    insert_commas(&base)
                } else {
                    base
                };
                if left {
                    format!("{:<width$}", base, width = w)
                } else {
                    format!("{:>width$}", base, width = w)
                }
            }
            's' => {
                let s = arg.to_str().map_err(|e| format!("fprintf %s: {}", e))?;
                if left {
                    format!("{:<width$}", s, width = w)
                } else {
                    format!("{:>width$}", s, width = w)
                }
            }
            other => return Err(format!("fprintf: unknown specifier '%{}'", other)),
        };
        result.push_str(&piece);
    }
    Ok(result)
}

// ─── Aggregate builtins ───────────────────────────────────────────────────────

fn builtin_all(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("all", &args, 1)?;
    match &args[0] {
        Value::Bool(b) => Ok(Value::Bool(*b)),
        Value::Scalar(n) => Ok(Value::Bool(*n != 0.0)),
        Value::Vector(v) => Ok(Value::Bool(v.iter().all(|c| c.re != 0.0 || c.im != 0.0))),
        other => Err(ScriptError::type_err(format!(
            "all: expected vector or scalar, got {}",
            other.type_name()
        ))),
    }
}

fn builtin_any(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("any", &args, 1)?;
    match &args[0] {
        Value::Bool(b) => Ok(Value::Bool(*b)),
        Value::Scalar(n) => Ok(Value::Bool(*n != 0.0)),
        Value::Vector(v) => Ok(Value::Bool(v.iter().any(|c| c.re != 0.0 || c.im != 0.0))),
        other => Err(ScriptError::type_err(format!(
            "any: expected vector or scalar, got {}",
            other.type_name()
        ))),
    }
}

// ─── rank() and roots() ───────────────────────────────────────────────────────

fn builtin_rank(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("rank", &args, 1)?;
    let m = match &args[0] {
        Value::Matrix(m) => m.clone(),
        Value::Scalar(_) => return Ok(Value::Scalar(1.0)),
        Value::Vector(v) if !v.is_empty() => return Ok(Value::Scalar(1.0)),
        other => {
            return Err(ScriptError::type_err(format!(
                "rank: expected matrix, got {}",
                other.type_name()
            )))
        }
    };
    if m.nrows() == 0 || m.ncols() == 0 {
        return Ok(Value::Scalar(0.0));
    }

    // Singular values = sqrt(|eigenvalues of A†A|)
    let ata: CMatrix = m.t().mapv(|c| c.conj()).dot(&m);
    let h = hessenberg_reduce(&ata);
    let evals = eig_hessenberg(&h).map_err(|e| ScriptError::runtime(e))?;

    let svs: Vec<f64> = evals.iter().map(|c| c.norm().sqrt()).collect();
    let max_sv = svs.iter().cloned().fold(0.0_f64, f64::max);
    // A†A eigenvalue approach squares the matrix, so rounding errors are amplified;
    // use sqrt(eps) rather than eps to set a robust threshold.
    let tol = f64::EPSILON.sqrt() * (m.nrows().max(m.ncols()) as f64) * max_sv;

    let r = svs.iter().filter(|&&s| s > tol).count();
    Ok(Value::Scalar(r as f64))
}

fn builtin_roots(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("roots", &args, 1)?;
    let coeffs = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;

    // Strip leading near-zero coefficients
    let first = match coeffs.iter().position(|c| c.norm() > 1e-15) {
        Some(i) => i,
        None => return Ok(Value::Vector(Array1::zeros(0))),
    };
    let p: Vec<C64> = coeffs.iter().skip(first).cloned().collect();

    let deg = p.len().saturating_sub(1);
    if deg == 0 {
        return Ok(Value::Vector(Array1::zeros(0)));
    }
    if deg == 1 {
        // a*x + b = 0  →  x = -b/a
        return Ok(Value::Vector(Array1::from_vec(vec![-p[1] / p[0]])));
    }

    // Build Frobenius companion matrix (deg × deg)
    let lead = p[0];
    let mut comp: CMatrix = Array2::zeros((deg, deg));
    // First row: -p[1..] / lead
    for j in 0..deg {
        comp[[0, j]] = -p[j + 1] / lead;
    }
    // Sub-diagonal of ones
    for i in 1..deg {
        comp[[i, i - 1]] = Complex::new(1.0, 0.0);
    }

    let h = hessenberg_reduce(&comp);
    let rs = eig_hessenberg(&h).map_err(|e| ScriptError::runtime(e))?;
    Ok(Value::Vector(Array1::from_vec(rs)))
}

fn builtin_iscell(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("iscell", &args, 1)?;
    Ok(Value::Bool(matches!(args[0], Value::StringArray(_))))
}

fn builtin_isstruct(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("isstruct", &args, 1)?;
    Ok(Value::Bool(matches!(args[0], Value::Struct(_))))
}

fn builtin_fieldnames(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("fieldnames", &args, 1)?;
    match &args[0] {
        Value::Struct(fields) => {
            let mut names: Vec<_> = fields.keys().cloned().collect();
            names.sort();
            for name in &names {
                super::output::script_println(&format!("  {}", name));
            }
            Ok(Value::None)
        }
        other => Err(ScriptError::runtime(format!(
            "fieldnames() requires a struct, got {}",
            other.type_name()
        ))),
    }
}

fn builtin_isfield(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("isfield", &args, 2)?;
    let field = args[1].to_str().map_err(|e| ScriptError::runtime(e))?;
    match &args[0] {
        Value::Struct(fields) => Ok(Value::Bool(fields.contains_key(&field))),
        other => Err(ScriptError::runtime(format!(
            "isfield() requires a struct, got {}",
            other.type_name()
        ))),
    }
}

fn builtin_rmfield(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("rmfield", &args, 2)?;
    let field = args[1].to_str().map_err(|e| ScriptError::runtime(e))?;
    match args.into_iter().next().unwrap() {
        Value::Struct(mut fields) => {
            if fields.remove(&field).is_none() {
                return Err(ScriptError::runtime(format!(
                    "struct has no field '{}'",
                    field
                )));
            }
            Ok(Value::Struct(fields))
        }
        other => Err(ScriptError::runtime(format!(
            "rmfield() requires a struct, got {}",
            other.type_name()
        ))),
    }
}

// ── Phase 2: Transfer Function builtins ──────────────────────────────────────

fn builtin_tf(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("tf", &args, 1, 2)?;
    if args.len() == 1 {
        // tf("s") → Laplace variable s, representing the polynomial s/1
        let s = args[0].to_str().map_err(|e| ScriptError::runtime(e))?;
        if s != "s" {
            return Err(ScriptError::runtime(format!(
                "tf: single-argument form expects \"s\", got \"{}\"",
                s
            )));
        }
        Ok(Value::TransferFn {
            num: vec![1.0, 0.0],
            den: vec![1.0],
        })
    } else {
        // tf(num_vec, den_vec) → explicit transfer function
        let num_cv = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
        let den_cv = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
        let num: Result<Vec<f64>, ScriptError> = num_cv
            .iter()
            .map(|c| {
                if c.im.abs() > 1e-12 {
                    Err(ScriptError::type_err(
                        "tf: numerator coefficients must be real".to_string(),
                    ))
                } else {
                    Ok(c.re)
                }
            })
            .collect();
        let den: Result<Vec<f64>, ScriptError> = den_cv
            .iter()
            .map(|c| {
                if c.im.abs() > 1e-12 {
                    Err(ScriptError::type_err(
                        "tf: denominator coefficients must be real".to_string(),
                    ))
                } else {
                    Ok(c.re)
                }
            })
            .collect();
        if den_cv.is_empty() {
            return Err(ScriptError::runtime(
                "tf: denominator must be non-empty".to_string(),
            ));
        }
        Ok(Value::TransferFn {
            num: num?,
            den: den?,
        })
    }
}

/// Convert a real polynomial coefficient slice to a complex Value::Vector for roots().
fn real_poly_to_value(coeffs: &[f64]) -> Value {
    Value::Vector(Array1::from_iter(
        coeffs.iter().map(|&x| Complex::new(x, 0.0)),
    ))
}

fn builtin_pole(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("pole", &args, 1)?;
    match &args[0] {
        Value::TransferFn { den, .. } => builtin_roots(vec![real_poly_to_value(den)]),
        other => Err(ScriptError::type_err(format!(
            "pole: expected tf, got {}",
            other.type_name()
        ))),
    }
}

fn builtin_zero(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("zero", &args, 1)?;
    match &args[0] {
        Value::TransferFn { num, .. } => builtin_roots(vec![real_poly_to_value(num)]),
        other => Err(ScriptError::type_err(format!(
            "zero: expected tf, got {}",
            other.type_name()
        ))),
    }
}

// ── Phase 3: State-Space builtins ─────────────────────────────────────────────

/// Convert a TransferFn to observable canonical form StateSpace.
fn tf_to_ss(num: &[f64], den: &[f64]) -> Result<Value, String> {
    use ndarray::Array2;

    if den.is_empty() {
        return Err("ss: empty denominator".to_string());
    }
    let n = den.len() - 1;
    if n == 0 {
        return Err("ss: transfer function must have order >= 1".to_string());
    }
    let d0 = den[0];
    if d0.abs() < 1e-15 {
        return Err("ss: leading denominator coefficient is zero".to_string());
    }

    // Monic denominator: a = [1, a₁, ..., aₙ]
    let a: Vec<f64> = den.iter().map(|&x| x / d0).collect();
    let num_norm: Vec<f64> = num.iter().map(|&x| x / d0).collect();

    // Separate direct feedthrough (D) from strictly proper numerator coefficients (b)
    let (d_val, b): (f64, Vec<f64>) = if num_norm.len() == n + 1 {
        // Proper (non-strictly-proper): subtract D * monic_den
        let dv = num_norm[0];
        let bv: Vec<f64> = num_norm[1..]
            .iter()
            .zip(a[1..].iter())
            .map(|(&ni, &ai)| ni - dv * ai)
            .collect();
        (dv, bv)
    } else {
        // Strictly proper: pad numerator with leading zeros to length n
        let mut bv = vec![0.0f64; n];
        let offset = n.saturating_sub(num_norm.len());
        for (i, &x) in num_norm.iter().enumerate() {
            bv[offset + i] = x;
        }
        (0.0, bv)
    };

    // Observable canonical form:
    //   A[i,0] = -aᵢ₊₁  (first column)
    //   A[i,i+1] = 1     (super-diagonal)
    let mut a_mat: CMatrix = Array2::zeros((n, n));
    for i in 0..n {
        a_mat[[i, 0]] = Complex::new(-a[i + 1], 0.0);
    }
    for i in 0..n - 1 {
        a_mat[[i, i + 1]] = Complex::new(1.0, 0.0);
    }

    // B: n×1, numerator coefficients
    let b_mat: CMatrix = Array2::from_shape_fn((n, 1), |(i, _)| Complex::new(b[i], 0.0));

    // C: 1×n, [1, 0, 0, ...]
    let c_mat: CMatrix = Array2::from_shape_fn((1, n), |(_, j)| {
        if j == 0 {
            Complex::new(1.0, 0.0)
        } else {
            Complex::new(0.0, 0.0)
        }
    });

    // D: 1×1
    let d_mat: CMatrix = Array2::from_shape_vec((1, 1), vec![Complex::new(d_val, 0.0)])
        .map_err(|e| e.to_string())?;

    Ok(Value::StateSpace {
        a: a_mat,
        b: b_mat,
        c: c_mat,
        d: d_mat,
    })
}

fn builtin_ss(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("ss", &args, 1)?;
    match &args[0] {
        Value::TransferFn { num, den } => tf_to_ss(num, den).map_err(|e| ScriptError::runtime(e)),
        other => Err(ScriptError::type_err(format!(
            "ss: expected tf, got {} (direct ss(A,B,C,D) construction not yet supported)",
            other.type_name()
        ))),
    }
}

fn builtin_ctrb(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("ctrb", &args, 2)?;
    let a = match &args[0] {
        Value::Matrix(m) => m.clone(),
        other => {
            return Err(ScriptError::type_err(format!(
                "ctrb: A must be a matrix, got {}",
                other.type_name()
            )))
        }
    };
    let b = match &args[1] {
        Value::Matrix(m) => m.clone(),
        Value::Vector(v) => {
            // Treat a vector as a column matrix
            let n = v.len();
            Array2::from_shape_fn((n, 1), |(i, _)| v[i])
        }
        other => {
            return Err(ScriptError::type_err(format!(
                "ctrb: B must be a matrix or vector, got {}",
                other.type_name()
            )))
        }
    };
    let n = a.nrows();
    if a.ncols() != n {
        return Err(ScriptError::runtime("ctrb: A must be square".to_string()));
    }
    if b.nrows() != n {
        return Err(ScriptError::runtime(format!(
            "ctrb: B has {} rows but A is {}×{}",
            b.nrows(),
            n,
            n
        )));
    }
    let m = b.ncols();
    // Build [B, AB, A²B, ..., A^(n-1)B] — n×(n*m)
    let mut cols: Vec<CMatrix> = Vec::with_capacity(n);
    let mut ab = b.clone();
    cols.push(ab.clone());
    for _ in 1..n {
        ab = a.dot(&ab);
        cols.push(ab.clone());
    }
    // Horizontally concatenate all columns
    let total_cols = n * m;
    let mut data: Vec<C64> = Vec::with_capacity(n * total_cols);
    for r in 0..n {
        for block in &cols {
            for c in 0..m {
                data.push(block[[r, c]]);
            }
        }
    }
    let result = Array2::from_shape_vec((n, total_cols), data)
        .map_err(|e| ScriptError::runtime(e.to_string()))?;
    Ok(Value::Matrix(result))
}

fn builtin_obsv(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("obsv", &args, 2)?;
    let a = match &args[0] {
        Value::Matrix(m) => m.clone(),
        other => {
            return Err(ScriptError::type_err(format!(
                "obsv: A must be a matrix, got {}",
                other.type_name()
            )))
        }
    };
    let c = match &args[1] {
        Value::Matrix(m) => m.clone(),
        Value::Vector(v) => {
            // Treat a vector as a row matrix
            let n = v.len();
            Array2::from_shape_fn((1, n), |(_, j)| v[j])
        }
        other => {
            return Err(ScriptError::type_err(format!(
                "obsv: C must be a matrix or vector, got {}",
                other.type_name()
            )))
        }
    };
    let n = a.nrows();
    if a.ncols() != n {
        return Err(ScriptError::runtime("obsv: A must be square".to_string()));
    }
    if c.ncols() != n {
        return Err(ScriptError::runtime(format!(
            "obsv: C has {} columns but A is {}×{}",
            c.ncols(),
            n,
            n
        )));
    }
    let p = c.nrows();
    // Build [C; CA; CA²; ...; CA^(n-1)] — (n*p)×n
    let mut rows: Vec<CMatrix> = Vec::with_capacity(n);
    let mut ca = c.clone();
    rows.push(ca.clone());
    for _ in 1..n {
        ca = ca.dot(&a);
        rows.push(ca.clone());
    }
    // Vertically concatenate all rows
    let total_rows = n * p;
    let mut data: Vec<C64> = Vec::with_capacity(total_rows * n);
    for block in &rows {
        for r in 0..p {
            for c_idx in 0..n {
                data.push(block[[r, c_idx]]);
            }
        }
    }
    let result = Array2::from_shape_vec((total_rows, n), data)
        .map_err(|e| ScriptError::runtime(e.to_string()))?;
    Ok(Value::Matrix(result))
}

// ── Phase 4: Frequency & Time-Domain Analysis ─────────────────────────────────

/// Evaluate a real polynomial at a complex point via Horner's method.
fn poly_eval_c(coeffs: &[f64], s: C64) -> C64 {
    let mut acc = Complex::new(0.0, 0.0);
    for &c in coeffs {
        acc = acc * s + Complex::new(c, 0.0);
    }
    acc
}

/// n log-spaced values from start to stop.
fn logspace(start: f64, stop: f64, n: usize) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![start];
    }
    let ls = start.log10();
    let le = stop.log10();
    (0..n)
        .map(|i| {
            let t = i as f64 / (n - 1) as f64;
            10.0_f64.powf(ls + t * (le - ls))
        })
        .collect()
}

/// Take real part of a CMatrix.
fn to_real_mat(m: &CMatrix) -> ndarray::Array2<f64> {
    m.mapv(|c| c.re)
}

/// Single RK4 integration step for ẋ = Ax + Bu (real, SISO u scalar).
fn rk4_step(
    a: &ndarray::Array2<f64>,
    b: &ndarray::Array2<f64>,
    x: &ndarray::Array1<f64>,
    u: f64,
    h: f64,
) -> ndarray::Array1<f64> {
    let bu: ndarray::Array1<f64> = b.column(0).mapv(|bi| bi * u);
    let f = |xk: &ndarray::Array1<f64>| -> ndarray::Array1<f64> { a.dot(xk) + &bu };
    let k1 = f(x);
    let x2: ndarray::Array1<f64> = x + &k1.mapv(|v| v * (h / 2.0));
    let k2 = f(&x2);
    let x3: ndarray::Array1<f64> = x + &k2.mapv(|v| v * (h / 2.0));
    let k3 = f(&x3);
    let x4: ndarray::Array1<f64> = x + &k3.mapv(|v| v * h);
    let k4 = f(&x4);
    let dx = (k1 + k2.mapv(|v| v * 2.0) + k3.mapv(|v| v * 2.0) + k4).mapv(|v| v * (h / 6.0));
    x + &dx
}

/// Unwrap phase (degrees) across a sequence to remove ±360° jumps.
fn unwrap_phase_deg(phase: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0f64; phase.len()];
    if phase.is_empty() {
        return out;
    }
    out[0] = phase[0];
    for i in 1..phase.len() {
        let diff = phase[i] - out[i - 1];
        let adj = if diff > 180.0 {
            diff - 360.0
        } else if diff < -180.0 {
            diff + 360.0
        } else {
            diff
        };
        out[i] = out[i - 1] + adj;
    }
    out
}

/// Find the x-value where y crosses `target` (first crossing, linear interpolation).
fn find_crossing(x: &[f64], y: &[f64], target: f64) -> Option<f64> {
    for i in 0..y.len().saturating_sub(1) {
        let y0 = y[i] - target;
        let y1 = y[i + 1] - target;
        if y0 * y1 <= 0.0 && (y0 - y1).abs() > 1e-30 {
            let t = y0 / (y0 - y1);
            return Some(x[i] + t * (x[i + 1] - x[i]));
        }
    }
    None
}

/// Evaluate Bode data (mag_dB, phase_deg unwrapped) for a TF over frequency vector w.
fn bode_compute(num: &[f64], den: &[f64], w: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let h: Vec<C64> = w
        .iter()
        .map(|&wi| {
            let jw = Complex::new(0.0, wi);
            let n = poly_eval_c(num, jw);
            let d = poly_eval_c(den, jw);
            if d.norm() < 1e-300 {
                Complex::new(f64::INFINITY, 0.0)
            } else {
                n / d
            }
        })
        .collect();
    let mag_db: Vec<f64> = h.iter().map(|v| 20.0 * v.norm().log10()).collect();
    let phase_raw: Vec<f64> = h.iter().map(|v| v.arg().to_degrees()).collect();
    (mag_db, unwrap_phase_deg(&phase_raw))
}

/// Auto frequency range based on pole magnitudes.
fn auto_freq_range(den: &[f64]) -> Result<Vec<f64>, ScriptError> {
    let poles = builtin_roots(vec![real_poly_to_value(den)])?;
    let w_nat = match &poles {
        Value::Vector(v) if !v.is_empty() => {
            v.iter().map(|c| c.norm()).fold(0.0f64, f64::max).max(1.0)
        }
        _ => 1.0,
    };
    Ok(logspace((w_nat * 0.01).max(1e-3), w_nat * 100.0, 200))
}

fn builtin_bode(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("bode", &args, 1, 2)?;
    let (num, den) = match &args[0] {
        Value::TransferFn { num, den } => (num.clone(), den.clone()),
        other => {
            return Err(ScriptError::type_err(format!(
                "bode: expected tf, got {}",
                other.type_name()
            )))
        }
    };

    let w_vec: Vec<f64> = if args.len() == 2 {
        match &args[1] {
            Value::Vector(v) => v.iter().map(|c| c.re).collect(),
            other => {
                return Err(ScriptError::type_err(format!(
                    "bode: w must be a vector, got {}",
                    other.type_name()
                )))
            }
        }
    } else {
        auto_freq_range(&den)?
    };

    let (mag_db, phase_deg) = bode_compute(&num, &den, &w_vec);

    // Plot on log10(ω) x-axis for visual log scaling
    let log_w: Vec<f64> = w_vec.iter().map(|&w| w.log10()).collect();

    FIGURE.with(|fig| fig.borrow_mut().set_subplot(2, 1, 1));
    push_xy_line(
        log_w.clone(),
        mag_db.clone(),
        "magnitude",
        "Bode Plot",
        None,
        LineStyle::Solid,
    );
    FIGURE.with(|fig| {
        let mut f = fig.borrow_mut();
        let sp = f.current_mut();
        sp.xlabel = "log10(ω rad/s)".to_string();
        sp.ylabel = "Magnitude (dB)".to_string();
    });

    FIGURE.with(|fig| fig.borrow_mut().set_subplot(2, 1, 2));
    push_xy_line(
        log_w,
        phase_deg.clone(),
        "phase",
        "",
        None,
        LineStyle::Solid,
    );
    FIGURE.with(|fig| {
        let mut f = fig.borrow_mut();
        let sp = f.current_mut();
        sp.xlabel = "log10(ω rad/s)".to_string();
        sp.ylabel = "Phase (deg)".to_string();
    });

    render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();

    let w_val = Value::Vector(Array1::from_iter(
        w_vec.iter().map(|&x| Complex::new(x, 0.0)),
    ));
    let mag_val = Value::Vector(Array1::from_iter(
        mag_db.iter().map(|&x| Complex::new(x, 0.0)),
    ));
    let ph_val = Value::Vector(Array1::from_iter(
        phase_deg.iter().map(|&x| Complex::new(x, 0.0)),
    ));
    Ok(Value::Tuple(vec![mag_val, ph_val, w_val]))
}

fn builtin_step(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("step", &args, 1, 2)?;
    let (num, den) = match &args[0] {
        Value::TransferFn { num, den } => (num.clone(), den.clone()),
        other => {
            return Err(ScriptError::type_err(format!(
                "step: expected tf, got {}",
                other.type_name()
            )))
        }
    };

    // Convert TF → SS
    let (a_c, b_c, c_c, d_c) = match tf_to_ss(&num, &den).map_err(|e| ScriptError::runtime(e))? {
        Value::StateSpace { a, b, c, d } => (a, b, c, d),
        _ => unreachable!(),
    };
    let a = to_real_mat(&a_c);
    let b = to_real_mat(&b_c);
    let c = to_real_mat(&c_c);
    let d = to_real_mat(&d_c);

    // Auto t_end: 10 / slowest pole decay rate, capped at 100 s
    let t_end: f64 = if args.len() == 2 {
        args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?
    } else {
        let poles = builtin_roots(vec![real_poly_to_value(&den)])?;
        let min_decay = match &poles {
            Value::Vector(v) if !v.is_empty() => v
                .iter()
                .map(|p| p.re.abs())
                .fold(f64::INFINITY, f64::min)
                .max(1e-6),
            _ => 1.0,
        };
        (10.0 / min_decay).min(100.0)
    };

    let n_steps = 1000usize;
    let h = t_end / n_steps as f64;
    let n = a.nrows();

    let mut x: ndarray::Array1<f64> = ndarray::Array1::zeros(n);
    let mut t_out = Vec::with_capacity(n_steps + 1);
    let mut y_out = Vec::with_capacity(n_steps + 1);

    for k in 0..=n_steps {
        let y_k = c.dot(&x)[0] + d[[0, 0]]; // u = 1
        t_out.push(k as f64 * h);
        y_out.push(y_k);
        if k < n_steps {
            x = rk4_step(&a, &b, &x, 1.0, h);
        }
    }

    push_xy_line(
        t_out.clone(),
        y_out.clone(),
        "y(t)",
        "Step Response",
        None,
        LineStyle::Solid,
    );
    FIGURE.with(|fig| {
        let mut f = fig.borrow_mut();
        let sp = f.current_mut();
        sp.xlabel = "Time (s)".to_string();
        sp.ylabel = "Amplitude".to_string();
    });
    render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();

    let y_val = Value::Vector(Array1::from_iter(
        y_out.iter().map(|&v| Complex::new(v, 0.0)),
    ));
    let t_val = Value::Vector(Array1::from_iter(
        t_out.iter().map(|&v| Complex::new(v, 0.0)),
    ));
    Ok(Value::Tuple(vec![y_val, t_val]))
}

// ─── LQR helpers ──────────────────────────────────────────────────────────────

/// Multiply two complex matrices: C = A * B  (naive O(n³), fine for small n)
fn mat_mul_cx(a: &CMatrix, b: &CMatrix) -> CMatrix {
    let (ra, ca) = (a.nrows(), a.ncols());
    let cb = b.ncols();
    assert_eq!(ca, b.nrows(), "mat_mul_cx: inner dimensions must match");
    let mut c: CMatrix = Array2::zeros((ra, cb));
    for i in 0..ra {
        for j in 0..cb {
            for k in 0..ca {
                c[[i, j]] += a[[i, k]] * b[[k, j]];
            }
        }
    }
    c
}

/// Inverse iteration: given matrix M and approximate eigenvalue λ,
/// return the corresponding eigenvector (2n-dim complex vector).
fn inverse_iteration_cx(
    m: &CMatrix,
    eigenvalue: C64,
    max_iter: usize,
) -> Result<CVector, ScriptError> {
    let n = m.nrows();
    // Perturb the shift so (M - shift*I) is nonsingular
    let scale = eigenvalue.norm().max(1.0);
    let shift = eigenvalue + Complex::new(scale * 1e-6, scale * 1e-6);

    let mut shifted = m.to_owned();
    for i in 0..n {
        shifted[[i, i]] -= shift;
    }

    let inv = matrix_inv(&shifted).map_err(|e| {
        ScriptError::type_err(format!(
            "inverse iteration failed (singular shift): {}",
            e
        ))
    })?;

    // Initial vector: a sine-of-index pattern. Diverse enough that for
    // (near-)triangular shifted matrices the iterate doesn't get trapped
    // in the e_0 invariant subspace — `e_0` collapses to the eigenvector
    // for the leading diagonal entry no matter which shift is in use,
    // which produced wrong eigenvectors for triangular inputs.
    // Same pattern as the core helper in
    // `rustlab-core/src/sparse_eig/hessenberg_eig.rs`.
    let mut v: CVector = Array1::from_iter(
        (0..n).map(|i| Complex::new(((i + 1) as f64 * 0.7).sin(), 0.0)),
    );
    let init_norm: f64 = v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
    if init_norm > 0.0 {
        for c in v.iter_mut() {
            *c /= init_norm;
        }
    }

    for _ in 0..max_iter {
        // v = inv * v
        let mut new_v: CVector = Array1::zeros(n);
        for i in 0..n {
            for j in 0..n {
                new_v[i] += inv[[i, j]] * v[j];
            }
        }
        let norm: f64 = new_v.iter().map(|c| c.norm_sqr()).sum::<f64>().sqrt();
        if norm < 1e-15 {
            break;
        }
        for c in new_v.iter_mut() {
            *c /= norm;
        }
        v = new_v;
    }
    Ok(v)
}

// ─── lqr(sys_ss, Q, R) ────────────────────────────────────────────────────────

/// `[K, S, e] = lqr(sys_ss, Q, R)`
///
/// Solve the continuous-time algebraic Riccati equation (CARE):
///   A'P + PA − P·B·R⁻¹·B'·P + Q = 0
/// Returns:
///   K  (m×n) — optimal feedback gain: u = -K*x
///   S  (n×n) — Riccati solution P
///   e  (n×1) — closed-loop eigenvalues of (A − B·K)
///
/// Algorithm: Hamiltonian matrix eigendecomposition.
///   H = [A, -B·R⁻¹·B'; -Q, -A']
/// Select the n stable eigenvectors [V1; V2], then P = V2·inv(V1).
fn builtin_lqr(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() != 3 {
        return Err(ScriptError::type_err(
            "lqr: requires 3 arguments: lqr(sys, Q, R)".to_string(),
        ));
    }

    // Extract A, B from state-space system
    let (a_mat, b_mat) = match &args[0] {
        Value::StateSpace { a, b, .. } => (a.clone(), b.clone()),
        other => {
            return Err(ScriptError::type_err(format!(
                "lqr: first argument must be a state-space system, got {}",
                other.type_name()
            )))
        }
    };

    // Extract Q
    let q_mat = match &args[1] {
        Value::Matrix(m) => m.clone(),
        other => {
            return Err(ScriptError::type_err(format!(
                "lqr: Q must be a matrix, got {}",
                other.type_name()
            )))
        }
    };

    // Extract R (allow scalar or matrix)
    let r_mat: CMatrix = match &args[2] {
        Value::Matrix(m) => m.clone(),
        Value::Scalar(s) => {
            let mut m: CMatrix = Array2::zeros((1, 1));
            m[[0, 0]] = Complex::new(*s, 0.0);
            m
        }
        other => {
            return Err(ScriptError::type_err(format!(
                "lqr: R must be a matrix or scalar, got {}",
                other.type_name()
            )))
        }
    };

    let n = a_mat.nrows();
    if n != a_mat.ncols() {
        return Err(ScriptError::type_err("lqr: A must be square".to_string()));
    }
    if q_mat.nrows() != n || q_mat.ncols() != n {
        return Err(ScriptError::type_err(format!(
            "lqr: Q must be {}×{}, got {}×{}",
            n,
            n,
            q_mat.nrows(),
            q_mat.ncols()
        )));
    }
    let m_in = b_mat.ncols();
    if r_mat.nrows() != m_in || r_mat.ncols() != m_in {
        return Err(ScriptError::type_err(format!(
            "lqr: R must be {}×{} (inputs), got {}×{}",
            m_in,
            m_in,
            r_mat.nrows(),
            r_mat.ncols()
        )));
    }

    // R⁻¹
    let r_inv = matrix_inv(&r_mat)
        .map_err(|e| ScriptError::type_err(format!("lqr: R is singular: {}", e)))?;

    // G = B · R⁻¹ · B'  (n×n)
    let br = mat_mul_cx(&b_mat, &r_inv); // n×m
    let bt: CMatrix = b_mat.t().mapv(|c| c.conj()).to_owned(); // m×n
    let g = mat_mul_cx(&br, &bt); // n×n

    // Hamiltonian H = [A, -G; -Q, -A']  (2n×2n)
    let two_n = 2 * n;
    let mut ham: CMatrix = Array2::zeros((two_n, two_n));
    for i in 0..n {
        for j in 0..n {
            ham[[i, j]] = a_mat[[i, j]];
            ham[[i, n + j]] = -g[[i, j]];
            ham[[n + i, j]] = -q_mat[[i, j]];
            ham[[n + i, n + j]] = -a_mat[[j, i]].conj(); // -A'
        }
    }

    // Eigenvalues of H
    let h_hess = hessenberg_reduce(&ham);
    let all_eigs = eig_hessenberg(&h_hess).map_err(|e| {
        ScriptError::type_err(format!("lqr: Hamiltonian eigenvalues failed: {}", e))
    })?;

    // Select the n stable eigenvalues (Re < 0), sort for determinism
    let mut stable: Vec<C64> = all_eigs.iter().filter(|e| e.re < -1e-10).cloned().collect();

    if stable.len() < n {
        return Err(ScriptError::type_err(format!(
            "lqr: found only {} stable Hamiltonian eigenvalues (need {}); \
             system may not be stabilizable",
            stable.len(),
            n
        )));
    }

    // Sort by real part (most negative first) for numerical consistency
    stable.sort_by(|a, b| a.re.partial_cmp(&b.re).unwrap_or(std::cmp::Ordering::Equal));
    let stable = &stable[..n];

    // Eigenvectors via inverse iteration  →  V is 2n×n
    let mut v_mat: CMatrix = Array2::zeros((two_n, n));
    for (col, &lam) in stable.iter().enumerate() {
        let v = inverse_iteration_cx(&ham, lam, 40)?;
        for i in 0..two_n {
            v_mat[[i, col]] = v[i];
        }
    }

    // V1 = top n rows, V2 = bottom n rows
    let mut v1: CMatrix = Array2::zeros((n, n));
    let mut v2: CMatrix = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            v1[[i, j]] = v_mat[[i, j]];
            v2[[i, j]] = v_mat[[n + i, j]];
        }
    }

    // P = V2 · inv(V1)  — should be real symmetric positive semi-definite
    let v1_inv = matrix_inv(&v1).map_err(|e| {
        ScriptError::type_err(format!("lqr: eigenvector matrix V1 is singular: {}", e))
    })?;
    let p_cx = mat_mul_cx(&v2, &v1_inv);

    // Take real part (imaginary residuals ≈ 0 for well-conditioned problems)
    let mut p: CMatrix = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            p[[i, j]] = Complex::new(p_cx[[i, j]].re, 0.0);
        }
    }

    // K = R⁻¹ · B' · P  (m×n)
    let bt_p = mat_mul_cx(&bt, &p); // m×n
    let k_mat = mat_mul_cx(&r_inv, &bt_p); // m×n

    // Closed-loop eigenvalues  e = eig(A − B·K)
    let bk = mat_mul_cx(&b_mat, &k_mat); // n×n
    let mut a_cl: CMatrix = a_mat.clone();
    for i in 0..n {
        for j in 0..n {
            a_cl[[i, j]] -= bk[[i, j]];
        }
    }
    let a_cl_h = hessenberg_reduce(&a_cl);
    let cl_eigs = eig_hessenberg(&a_cl_h)
        .map_err(|e| ScriptError::type_err(format!("lqr: closed-loop eig failed: {}", e)))?;

    let e_vec: CVector = Array1::from_vec(cl_eigs);

    Ok(Value::Tuple(vec![
        Value::Matrix(k_mat),
        Value::Matrix(p),
        Value::Vector(e_vec),
    ]))
}

/// Add two real polynomials: result = p1 + k * p2.
/// Both in descending-power order; p2 is right-aligned (zero-padded left) if shorter.
fn poly_add_scaled(p1: &[f64], p2: &[f64], k: f64) -> Vec<f64> {
    let n = p1.len();
    let m = p2.len();
    let mut result = p1.to_vec();
    let offset = n.saturating_sub(m);
    for (i, &c) in p2.iter().enumerate() {
        let idx = offset + i;
        if idx < n {
            result[idx] += k * c;
        }
    }
    result
}

/// Reorder `new_roots` to minimise total displacement from `prev_roots` (greedy nearest-neighbour).
/// This keeps root trajectories continuous across K steps.
fn pair_roots_by_proximity(new_roots: Vec<C64>, prev: &[C64]) -> Vec<C64> {
    let n = prev.len().min(new_roots.len());
    let mut used = vec![false; new_roots.len()];
    let mut result = new_roots.clone();
    for i in 0..n {
        let best_j = new_roots
            .iter()
            .enumerate()
            .filter(|(j, _)| !used[*j])
            .min_by(|(_, a), (_, b)| {
                let da = (*a - prev[i]).norm();
                let db = (*b - prev[i]).norm();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(j, _)| j)
            .unwrap_or(i);
        result[i] = new_roots[best_j];
        used[best_j] = true;
    }
    result
}

fn builtin_rlocus(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("rlocus", &args, 1)?;
    let (num, den) = match &args[0] {
        Value::TransferFn { num, den } => (num.clone(), den.clone()),
        other => {
            return Err(ScriptError::type_err(format!(
                "rlocus: expected tf, got {}",
                other.type_name()
            )))
        }
    };

    let n_poles = den.len().saturating_sub(1);
    if n_poles == 0 {
        return Err(ScriptError::runtime(
            "rlocus: system has no poles".to_string(),
        ));
    }
    let n_zeros = num.len().saturating_sub(1);
    if n_zeros >= n_poles {
        return Err(ScriptError::runtime(format!(
            "rlocus: TF must be proper (deg(num) < deg(den)), got {n_zeros} >= {n_poles}"
        )));
    }

    // Open-loop poles (K=0): roots of den, sorted by Im for stable initial ordering
    let ol_val = builtin_roots(vec![real_poly_to_value(&den)])?;
    let mut ol_poles: Vec<C64> = match ol_val {
        Value::Vector(v) => v.to_vec(),
        _ => {
            return Err(ScriptError::runtime(
                "rlocus: failed to compute poles".to_string(),
            ))
        }
    };
    ol_poles.sort_by(|a, b| a.im.partial_cmp(&b.im).unwrap_or(std::cmp::Ordering::Equal));

    // K sweep: log-spaced from 1e-3 to 1e4, 300 points
    let k_vals = logspace(1e-3, 1e4, 300);

    // trajectories[i] = sequence of (re, im) for root i across K
    let mut trajectories: Vec<Vec<(f64, f64)>> =
        ol_poles.iter().map(|p| vec![(p.re, p.im)]).collect();
    let mut prev_roots: Vec<C64> = ol_poles.clone();

    for &k in &k_vals {
        let combined = poly_add_scaled(&den, &num, k);
        let roots_val = match builtin_roots(vec![real_poly_to_value(&combined)]) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let roots: Vec<C64> = match roots_val {
            Value::Vector(v) => v.to_vec(),
            _ => continue,
        };
        if roots.len() != n_poles {
            continue;
        }
        let paired = pair_roots_by_proximity(roots, &prev_roots);
        for (i, r) in paired.iter().enumerate() {
            trajectories[i].push((r.re, r.im));
        }
        prev_roots = paired;
    }

    // Set up figure: full reset, then hold=true so all trajectories accumulate
    FIGURE.with(|fig| {
        let mut f = fig.borrow_mut();
        f.reset();
        let sp = f.current_mut();
        sp.title = "Root Locus".to_string();
        sp.xlabel = "Real".to_string();
        sp.ylabel = "Imaginary".to_string();
        f.hold = true;
    });

    for (i, traj) in trajectories.iter().enumerate() {
        let x: Vec<f64> = traj.iter().map(|&(re, _)| re).collect();
        let y: Vec<f64> = traj.iter().map(|&(_, im)| im).collect();
        push_xy_line(
            x,
            y,
            &format!("root {}", i + 1),
            "",
            Some(SeriesColor::cycle(i)),
            LineStyle::Solid,
        );
    }

    render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();
    Ok(Value::None)
}

fn builtin_margin(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("margin", &args, 1)?;
    let (num, den) = match &args[0] {
        Value::TransferFn { num, den } => (num.clone(), den.clone()),
        other => {
            return Err(ScriptError::type_err(format!(
                "margin: expected tf, got {}",
                other.type_name()
            )))
        }
    };

    // Dense grid for accurate crossing detection
    let poles = builtin_roots(vec![real_poly_to_value(&den)])?;
    let w_nat = match &poles {
        Value::Vector(v) if !v.is_empty() => {
            v.iter().map(|c| c.norm()).fold(0.0f64, f64::max).max(1.0)
        }
        _ => 1.0,
    };
    let w_vec = logspace((w_nat * 0.001).max(1e-4), w_nat * 1000.0, 1000);
    let (mag_db, phase_deg) = bode_compute(&num, &den, &w_vec);

    // Gain crossover: |H| = 0 dB
    let wcp = find_crossing(&w_vec, &mag_db, 0.0);

    // Phase crossover: phase = -180°
    let wcg = find_crossing(&w_vec, &phase_deg, -180.0);

    // Gain margin = 1 / |H(jWcg)|
    let gm = if let Some(wc) = wcg {
        let jw = Complex::new(0.0, wc);
        let h = poly_eval_c(&num, jw) / poly_eval_c(&den, jw);
        if h.norm() > 1e-30 {
            1.0 / h.norm()
        } else {
            f64::INFINITY
        }
    } else {
        f64::INFINITY
    };

    // Phase margin = 180° + ∠H(jWcp)
    let pm = if let Some(wc) = wcp {
        let jw = Complex::new(0.0, wc);
        let h = poly_eval_c(&num, jw) / poly_eval_c(&den, jw);
        180.0 + h.arg().to_degrees()
    } else {
        f64::INFINITY
    };

    Ok(Value::Tuple(vec![
        Value::Scalar(gm),
        Value::Scalar(pm),
        Value::Scalar(wcg.unwrap_or(f64::INFINITY)),
        Value::Scalar(wcp.unwrap_or(f64::INFINITY)),
    ]))
}

// ─── ML / activation functions ──────────────────────────────────────────────

/// softmax(v) — numerically-stable softmax over the real parts of a vector.
/// Returns a real-valued probability vector summing to 1.0.
fn builtin_softmax(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("softmax", &args, 1)?;
    match &args[0] {
        Value::Vector(v) if !v.is_empty() => {
            // Subtract max for numerical stability before exp
            let max_re = v.iter().map(|c| c.re).fold(f64::NEG_INFINITY, f64::max);
            let exps: Vec<f64> = v.iter().map(|c| (c.re - max_re).exp()).collect();
            let sum: f64 = exps.iter().sum();
            let result: CVector =
                Array1::from_iter(exps.iter().map(|&e| Complex::new(e / sum, 0.0)));
            Ok(Value::Vector(result))
        }
        Value::Scalar(_) => Ok(Value::Scalar(1.0)),
        _ => Err(ScriptError::type_err(
            "softmax: argument must be a non-empty vector or scalar".to_string(),
        )),
    }
}

/// relu(x) — rectified linear unit: max(0, x), element-wise.
fn builtin_relu(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value(
        "relu",
        args,
        |x: f64| x.max(0.0),
        |c: Complex<f64>| Complex::new(c.re.max(0.0), 0.0),
    )
}

fn gelu_scalar(x: f64) -> f64 {
    // Standard tanh approximation used by most deep-learning frameworks:
    //   GELU(x) = 0.5 * x * (1 + tanh(sqrt(2/π) * (x + 0.044715 * x³)))
    let c = (2.0_f64 / std::f64::consts::PI).sqrt();
    0.5 * x * (1.0 + (c * (x + 0.044715 * x.powi(3))).tanh())
}

/// gelu(x) — Gaussian error linear unit, element-wise.
fn builtin_gelu(args: Vec<Value>) -> Result<Value, ScriptError> {
    apply_scalar_fn_to_value("gelu", args, gelu_scalar, |c: Complex<f64>| {
        Complex::new(gelu_scalar(c.re), 0.0)
    })
}

/// layernorm(v) or layernorm(v, eps) — layer normalisation: (v - mean) / sqrt(var + eps).
/// Uses population variance (divides by N, not N-1).
fn builtin_layernorm(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("layernorm", &args, 1, 2)?;
    let eps = if args.len() == 2 {
        args[1].to_scalar().map_err(|e| ScriptError::type_err(e))?
    } else {
        1e-5
    };
    match &args[0] {
        Value::Vector(v) if !v.is_empty() => {
            let n = v.len() as f64;
            let mean: C64 = v.iter().copied().sum::<C64>() / n;
            let variance: f64 = v.iter().map(|&x| (x - mean).norm_sqr()).sum::<f64>() / n;
            let std_dev = (variance + eps).sqrt();
            let result: CVector = v.mapv(|c| (c - mean) / std_dev);
            Ok(Value::Vector(result))
        }
        Value::Scalar(s) => {
            // Single scalar: mean == s, variance == 0, result == 0 (no information)
            let _ = s;
            Ok(Value::Scalar(0.0))
        }
        _ => Err(ScriptError::type_err(
            "layernorm: argument must be a non-empty vector or scalar".to_string(),
        )),
    }
}

// ─── bar builtin ─────────────────────────────────────────────────────────────

/// bar(y)  or  bar(x, y)  or  bar(x, y, "title")  or  bar(y, "title")
/// bar(M)  or  bar(x, M)  or  bar(x, M, "title")  — grouped bar chart (each column = group)
/// bar(labels, y)  or  bar(labels, y, "title")  — categorical bar chart with string array labels
fn builtin_bar(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.is_empty() || args.len() > 3 {
        return Err(ScriptError::type_err(
            "bar: expected bar(y), bar(x,y), bar(labels,y), bar(M), bar(x,M), or bar(...,title)"
                .to_string(),
        ));
    }
    let mut args = args;
    flatten_column_matrix_args(&mut args);
    // Categorical bar chart: bar({"A","B","C"}, [10,20,30]) or bar(labels, y, "title")
    if let Value::StringArray(labels) = &args[0] {
        if args.len() < 2 {
            return Err(ScriptError::type_err(
                "bar: string array labels require a y-data argument".to_string(),
            ));
        }
        let y_data: Vec<f64> = to_real_vector(&args[1])?.to_vec();
        if labels.len() != y_data.len() {
            return Err(ScriptError::type_err(format!(
                "bar: labels length ({}) must match y-data length ({})",
                labels.len(),
                y_data.len()
            )));
        }
        let title = if args.len() > 2 {
            args[2].to_str().unwrap_or_default()
        } else {
            String::new()
        };
        let x_data: Vec<f64> = (1..=labels.len()).map(|i| i as f64).collect();
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            if !fig.hold {
                fig.current_mut().series.clear();
                fig.current_mut().title.clear();
            }
            let color = fig.next_color();
            let sp = fig.current_mut();
            if !title.is_empty() && sp.title.is_empty() {
                sp.title = title;
            }
            sp.x_labels = Some(labels.clone());
            sp.series.push(rustlab_plot::Series {
                label: "bar".to_string(),
                x_data,
                y_data,
                color,
                style: LineStyle::Solid,
                kind: rustlab_plot::PlotKind::Bar,
            });
        });
        render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))?;
        sync_figure_outputs();
        return Ok(Value::None);
    }
    // Check if the y-data argument is a matrix (grouped bar chart)
    let y_arg_idx = if args.len() >= 2 && !matches!(&args[1], Value::Str(_)) {
        1
    } else {
        0
    };
    if let Value::Matrix(m) = &args[y_arg_idx] {
        let x_data: Vec<f64> = if y_arg_idx == 1 {
            to_real_vector(&args[0])?.to_vec()
        } else {
            (0..m.nrows()).map(|i| i as f64).collect()
        };
        let title = if args.len() > y_arg_idx + 1 {
            args[y_arg_idx + 1].to_str().unwrap_or_default()
        } else {
            String::new()
        };
        // Each column is a group
        FIGURE.with(|fig| {
            let mut fig = fig.borrow_mut();
            if !fig.hold {
                fig.current_mut().series.clear();
                fig.current_mut().title.clear();
            }
            for col in 0..m.ncols() {
                let y_data: Vec<f64> = m.column(col).iter().map(|c| c.re).collect();
                let color = fig.next_color();
                let sp = fig.current_mut();
                if !title.is_empty() && sp.title.is_empty() {
                    sp.title = title.clone();
                }
                sp.series.push(rustlab_plot::Series {
                    label: format!("group{}", col + 1),
                    x_data: x_data.clone(),
                    y_data,
                    color,
                    style: LineStyle::Solid,
                    kind: rustlab_plot::PlotKind::Bar,
                });
            }
        });
        render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))?;
        sync_figure_outputs();
        return Ok(Value::None);
    }
    let (x_data, y_data, title) = extract_xy_with_title(&args, "bar")?;
    push_xy_bar(x_data, y_data, "bar", &title, None);
    render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();
    Ok(Value::None)
}

// ─── scatter builtin ──────────────────────────────────────────────────────────

/// scatter(x, y)  or  scatter(x, y, "title")
fn builtin_scatter(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() < 2 || args.len() > 3 {
        return Err(ScriptError::type_err(
            "scatter: expected scatter(x, y) or scatter(x, y, title)".to_string(),
        ));
    }
    let mut args = args;
    flatten_column_matrix_args(&mut args);
    let xv = to_real_vector(&args[0])?;
    let yv = to_real_vector(&args[1])?;
    let title = if args.len() == 3 {
        args[2].to_str().map_err(|e| ScriptError::type_err(e))?
    } else {
        String::new()
    };
    let x_data: Vec<f64> = xv.to_vec();
    let y_data: Vec<f64> = yv.to_vec();
    push_xy_scatter(x_data, y_data, "scatter", &title, None);
    render_figure_terminal().map_err(|e| ScriptError::runtime(e.to_string()))?;
    sync_figure_outputs();
    Ok(Value::None)
}

// ─── Shared extraction helpers ─────────────────────────────────────────────

/// Extract (x_data, y_data, title) from `bar(y)`, `bar(x,y)`, `bar(y,title)`,
/// `bar(x,y,title)` style argument lists.
fn extract_xy_with_title(
    args: &[Value],
    name: &str,
) -> Result<(Vec<f64>, Vec<f64>, String), ScriptError> {
    match args {
        // bar(y)
        [y] => {
            let yv = to_real_vector(y)?;
            let x_data: Vec<f64> = (0..yv.len()).map(|i| i as f64).collect();
            Ok((x_data, yv.to_vec(), String::new()))
        }
        // bar(x, y) or bar(y, "title")
        [a, b] => {
            if let Ok(title) = b.to_str() {
                let yv = to_real_vector(a)?;
                let x_data: Vec<f64> = (0..yv.len()).map(|i| i as f64).collect();
                Ok((x_data, yv.to_vec(), title))
            } else {
                let xv = to_real_vector(a)?;
                let yv = to_real_vector(b)?;
                Ok((xv.to_vec(), yv.to_vec(), String::new()))
            }
        }
        // bar(x, y, "title")
        [x, y, t] => {
            let xv = to_real_vector(x)?;
            let yv = to_real_vector(y)?;
            let title = t.to_str().map_err(|e| ScriptError::type_err(e))?;
            Ok((xv.to_vec(), yv.to_vec(), title))
        }
        _ => Err(ScriptError::type_err(format!(
            "{name}: wrong number of arguments"
        ))),
    }
}

// ─── Controls Bootcamp builtins ───────────────────────────────────────────────

/// logspace(a, b, n) — n log-spaced points from 10^a to 10^b.
fn builtin_logspace(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("logspace", &args, 3)?;
    let a = args[0]
        .to_scalar()
        .map_err(|_| ScriptError::type_err("logspace: a must be a real scalar".to_string()))?;
    let b = args[1]
        .to_scalar()
        .map_err(|_| ScriptError::type_err("logspace: b must be a real scalar".to_string()))?;
    let n = match &args[2] {
        Value::Scalar(s) => (*s as usize).max(1),
        other => {
            return Err(ScriptError::type_err(format!(
                "logspace: n must be a scalar, got {}",
                other.type_name()
            )))
        }
    };
    let vals: CVector = Array1::from_iter((0..n).map(|i| {
        let t = if n == 1 {
            0.0
        } else {
            i as f64 / (n - 1) as f64
        };
        Complex::new(10.0_f64.powf(a + t * (b - a)), 0.0)
    }));
    Ok(Value::Vector(vals))
}

/// lyap(A, Q) — solves A*X + X*A' + Q = 0 for X via Kronecker vectorization.
fn builtin_lyap(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("lyap", &args, 2)?;
    let a = to_cmatrix_arg(&args[0], "lyap", "A")?;
    let q = to_cmatrix_arg(&args[1], "lyap", "Q")?;
    let x = lyap_solve(&a, &q).map_err(|e| ScriptError::runtime(e))?;
    Ok(Value::Matrix(x))
}

/// Internal Lyapunov solver: A*X + X*A' + Q = 0.
fn lyap_solve(a: &CMatrix, q: &CMatrix) -> Result<CMatrix, String> {
    let n = a.nrows();
    if a.ncols() != n {
        return Err(format!("lyap: A must be square (got {}×{})", n, a.ncols()));
    }
    if q.nrows() != n || q.ncols() != n {
        return Err(format!(
            "lyap: Q must be {}×{} (got {}×{})",
            n,
            n,
            q.nrows(),
            q.ncols()
        ));
    }
    if n > 50 {
        eprintln!(
            "lyap: warning: n={} — Kronecker approach is O(n^4); consider smaller systems",
            n
        );
    }
    let n2 = n * n;
    let mut km: CMatrix = Array2::zeros((n2, n2));
    for i in 0..n {
        for j in 0..n {
            for k in 0..n {
                // AX term: K[i*n+k, j*n+k] += A[i,j]
                km[[i * n + k, j * n + k]] += a[[i, j]];
                // XA' term: K[i*n+k, i*n+j] += conj(A[k,j])
                km[[i * n + k, i * n + j]] += a[[k, j]].conj();
            }
        }
    }
    let mut q_col: CMatrix = Array2::zeros((n2, 1));
    for i in 0..n {
        for j in 0..n {
            q_col[[i * n + j, 0]] = q[[i, j]];
        }
    }
    let km_inv = matrix_inv(&km)
        .map_err(|e| format!("lyap: coefficient matrix singular ({e}); system may be unstable"))?;
    let x_col = mat_mul_cx(&km_inv, &q_col.mapv(|c| -c));
    let mut x: CMatrix = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            x[[i, j]] = x_col[[i * n + j, 0]];
        }
    }
    Ok(x)
}

/// gram(A, B, type) — controllability ("c") or observability ("o") Gramian via lyap.
/// For "c": A*W + W*A' + B*B' = 0  (B is n×m input matrix)
/// For "o": A'*W + W*A + C'*C = 0  (C is p×n output matrix)
/// If C is passed as a vector it is treated as a 1×n row (1 output).
fn builtin_gram(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("gram", &args, 3)?;
    let a = to_cmatrix_arg(&args[0], "gram", "A")?;
    let kind = args[2].to_str().map_err(|e| ScriptError::type_err(e))?;
    let n = a.nrows();

    let w = match kind.as_str() {
        "c" => {
            let b = to_cmatrix_arg(&args[1], "gram", "B")?;
            let bt: CMatrix = b.t().mapv(|c| c.conj()).to_owned();
            lyap_solve(&a, &mat_mul_cx(&b, &bt)).map_err(|e| ScriptError::runtime(e))?
        }
        "o" => {
            // C may arrive as a column vector (from script [1,0]) — transpose to row
            let c_raw = to_cmatrix_arg(&args[1], "gram", "C")?;
            let c: CMatrix = if c_raw.nrows() == n && c_raw.ncols() != n {
                // Looks like it came in as a column — treat as 1×n row
                c_raw.t().mapv(|x| x.conj()).to_owned()
            } else {
                c_raw
            };
            let at: CMatrix = a.t().mapv(|x| x.conj()).to_owned();
            let ct: CMatrix = c.t().mapv(|x| x.conj()).to_owned();
            lyap_solve(&at, &mat_mul_cx(&ct, &c)).map_err(|e| ScriptError::runtime(e))?
        }
        other => {
            return Err(ScriptError::type_err(format!(
                "gram: type must be \"c\" or \"o\", got {:?}",
                other
            )))
        }
    };
    Ok(Value::Matrix(w))
}

/// Internal CARE solver: A'P + PA - PBR⁻¹B'P + Q = 0 via Hamiltonian eigendecomposition.
fn solve_care(a: &CMatrix, b: &CMatrix, q: &CMatrix, r: &CMatrix) -> Result<CMatrix, ScriptError> {
    let n = a.nrows();
    let r_inv =
        matrix_inv(r).map_err(|e| ScriptError::type_err(format!("care: R is singular: {}", e)))?;
    let bt: CMatrix = b.t().mapv(|c| c.conj()).to_owned();
    let g = mat_mul_cx(&mat_mul_cx(b, &r_inv), &bt);

    let two_n = 2 * n;
    let mut ham: CMatrix = Array2::zeros((two_n, two_n));
    for i in 0..n {
        for j in 0..n {
            ham[[i, j]] = a[[i, j]];
            ham[[i, n + j]] = -g[[i, j]];
            ham[[n + i, j]] = -q[[i, j]];
            ham[[n + i, n + j]] = -a[[j, i]].conj();
        }
    }
    let all_eigs = eig_hessenberg(&hessenberg_reduce(&ham))
        .map_err(|e| ScriptError::type_err(format!("care: Hamiltonian eig failed: {}", e)))?;
    let mut stable: Vec<C64> = all_eigs.iter().filter(|e| e.re < -1e-10).cloned().collect();
    if stable.len() < n {
        return Err(ScriptError::type_err(format!(
            "care: only {} stable eigenvalues (need {}); system not stabilizable",
            stable.len(),
            n
        )));
    }
    stable.sort_by(|a, b| a.re.partial_cmp(&b.re).unwrap_or(std::cmp::Ordering::Equal));

    let mut v_mat: CMatrix = Array2::zeros((two_n, n));
    for (col, &lam) in stable[..n].iter().enumerate() {
        let v = inverse_iteration_cx(&ham, lam, 40)?;
        for i in 0..two_n {
            v_mat[[i, col]] = v[i];
        }
    }
    let mut v1: CMatrix = Array2::zeros((n, n));
    let mut v2: CMatrix = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            v1[[i, j]] = v_mat[[i, j]];
            v2[[i, j]] = v_mat[[n + i, j]];
        }
    }
    let p_cx = mat_mul_cx(
        &v2,
        &matrix_inv(&v1).map_err(|e| {
            ScriptError::type_err(format!("care: eigenvector matrix singular: {}", e))
        })?,
    );
    let mut p: CMatrix = Array2::zeros((n, n));
    for i in 0..n {
        for j in 0..n {
            let v = (p_cx[[i, j]] + p_cx[[j, i]].conj()) / 2.0;
            p[[i, j]] = Complex::new(v.re, 0.0);
        }
    }
    Ok(p)
}

/// care(A, B, Q, R) — solves A'P + PA - PBR⁻¹B'P + Q = 0 for P.
fn builtin_care(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("care", &args, 4)?;
    let a = to_cmatrix_arg(&args[0], "care", "A")?;
    let b = to_cmatrix_arg(&args[1], "care", "B")?;
    let q = to_cmatrix_arg(&args[2], "care", "Q")?;
    let r = to_cmatrix_arg(&args[3], "care", "R")?;
    Ok(Value::Matrix(solve_care(&a, &b, &q, &r)?))
}

/// dare(A, B, Q, R) — solves P = A'PA - A'PB*(R+B'PB)⁻¹*B'PA + Q via value iteration.
fn builtin_dare(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("dare", &args, 4)?;
    let a = to_cmatrix_arg(&args[0], "dare", "A")?;
    let b = to_cmatrix_arg(&args[1], "dare", "B")?;
    let q = to_cmatrix_arg(&args[2], "dare", "Q")?;
    let r = to_cmatrix_arg(&args[3], "dare", "R")?;
    let n = a.nrows();
    if a.ncols() != n {
        return Err(ScriptError::type_err("dare: A must be square".to_string()));
    }
    let at: CMatrix = a.t().mapv(|c| c.conj()).to_owned();
    let bt: CMatrix = b.t().mapv(|c| c.conj()).to_owned();
    let mut p = q.clone();
    for _ in 0..1000 {
        let pb = mat_mul_cx(&p, &b);
        let s_inv = matrix_inv(&(r.clone() + mat_mul_cx(&bt, &pb)))
            .map_err(|e| ScriptError::type_err(format!("dare: (R+B'PB) singular: {}", e)))?;
        let pa = mat_mul_cx(&p, &a);
        let k = mat_mul_cx(&s_inv, &mat_mul_cx(&bt, &pa));
        let p_new = mat_mul_cx(&at, &pa) - mat_mul_cx(&mat_mul_cx(&at, &pb), &k) + q.clone();
        let diff: f64 = (0..n)
            .flat_map(|i| (0..n).map(move |j| (i, j)))
            .map(|(i, j)| (p_new[[i, j]] - p[[i, j]]).norm())
            .fold(0.0_f64, f64::max);
        p = p_new;
        if diff < 1e-10 {
            break;
        }
    }
    Ok(Value::Matrix(p))
}

/// place(A, B, poles) — Ackermann's formula (SISO only).
/// Returns K (length-n vector) such that eig(A - B*K) ≈ poles.
fn builtin_place(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("place", &args, 3)?;
    let a = to_cmatrix_arg(&args[0], "place", "A")?;
    let b = to_cmatrix_arg(&args[1], "place", "B")?;
    let poles = args[2].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let n = a.nrows();
    if a.ncols() != n {
        return Err(ScriptError::type_err("place: A must be square".to_string()));
    }
    if b.ncols() != 1 {
        return Err(ScriptError::type_err(format!(
            "place: only SISO supported (B must be n×1, got n×{})",
            b.ncols()
        )));
    }
    if b.nrows() != n {
        return Err(ScriptError::type_err(format!(
            "place: B rows={} but A is {}×{}",
            b.nrows(),
            n,
            n
        )));
    }
    if poles.len() != n {
        return Err(ScriptError::type_err(format!(
            "place: {} poles but A is {}×{}",
            poles.len(),
            n,
            n
        )));
    }

    // Controllability matrix
    let mut cols: Vec<CMatrix> = vec![b.clone()];
    for _ in 1..n {
        cols.push(a.dot(cols.last().unwrap()));
    }
    let ctrb_data: Vec<C64> = (0..n)
        .flat_map(|r| cols.iter().map(move |col| col[[r, 0]]))
        .collect();
    let ctrb_inv = matrix_inv(
        &Array2::from_shape_vec((n, n), ctrb_data)
            .map_err(|e| ScriptError::runtime(e.to_string()))?,
    )
    .map_err(|e| ScriptError::type_err(format!("place: not controllable: {}", e)))?;

    // Characteristic polynomial from desired poles
    let mut poly: Vec<C64> = vec![Complex::new(1.0, 0.0)];
    for &r in poles.iter() {
        let mut new_poly = vec![Complex::new(0.0, 0.0); poly.len() + 1];
        for (k, &c) in poly.iter().enumerate() {
            new_poly[k] += c;
            new_poly[k + 1] -= r * c;
        }
        poly = new_poly;
    }

    // Evaluate p(A) via Horner
    let mut pa: CMatrix = Array2::eye(n);
    for k in 1..=n {
        pa = mat_mul_cx(&pa, &a);
        let ck = poly[k];
        for i in 0..n {
            pa[[i, i]] += ck;
        }
    }

    // K = e_n' * inv(ctrb) * p(A)
    let mut en: CMatrix = Array2::zeros((1, n));
    en[[0, n - 1]] = Complex::new(1.0, 0.0);
    let k_mat = mat_mul_cx(&mat_mul_cx(&en, &ctrb_inv), &pa);
    Ok(Value::Vector(Array1::from_iter(
        (0..n).map(|j| k_mat[[0, j]]),
    )))
}

/// freqresp(A, B, C, D, w) — H(jω) = C*(jω*I-A)^{-1}*B + D evaluated at each ω in w.
/// Returns complex vector (SISO) or complex matrix outputs×freqs (MIMO).
/// C may be passed as a vector (treated as a 1×n row output matrix).
fn builtin_freqresp(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("freqresp", &args, 5)?;
    let a = to_cmatrix_arg(&args[0], "freqresp", "A")?;
    let b = to_cmatrix_arg(&args[1], "freqresp", "B")?;
    let c_raw = to_cmatrix_arg(&args[2], "freqresp", "C")?;
    let n = a.nrows();
    // If C came in as a vector it becomes n×1; treat as 1×n row instead.
    let c = if c_raw.nrows() == n && c_raw.ncols() == 1 {
        c_raw.t().mapv(|x| x.conj()).to_owned()
    } else {
        c_raw
    };
    let d = to_cmatrix_arg(&args[3], "freqresp", "D")?;
    let w_val = args[4].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let np = c.nrows();
    let nw = w_val.len();
    if a.ncols() != n {
        return Err(ScriptError::type_err(
            "freqresp: A must be square".to_string(),
        ));
    }
    let eye_n: CMatrix = Array2::eye(n);
    let mut h_mat: CMatrix = Array2::zeros((np, nw));
    for (k, &w_c) in w_val.iter().enumerate() {
        let jw = Complex::new(0.0, w_c.re);
        let mut jwia: CMatrix = eye_n.mapv(|x| x * jw);
        for i in 0..n {
            for j in 0..n {
                jwia[[i, j]] -= a[[i, j]];
            }
        }
        let jwia_inv = matrix_inv(&jwia).map_err(|e| {
            ScriptError::runtime(format!("freqresp: singular at ω={:.4}: {}", w_c.re, e))
        })?;
        let cb = mat_mul_cx(&c, &mat_mul_cx(&jwia_inv, &b));
        for p in 0..np {
            h_mat[[p, k]] = cb[[p, 0]] + d[[p, 0]];
        }
    }
    if np == 1 {
        Ok(Value::Vector(h_mat.row(0).to_owned()))
    } else {
        Ok(Value::Matrix(h_mat))
    }
}

/// svd(A) — SVD via symmetric eigendecomposition of A'A (real matrices only).
/// Returns Tuple [U, sigma_vector, V] where A ≈ U * diag(sigma) * V'.
fn builtin_svd(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("svd", &args, 1)?;
    let m = to_cmatrix_arg(&args[0], "svd", "A")?;
    let max_im: f64 = m.iter().map(|c| c.im.abs()).fold(0.0_f64, f64::max);
    let max_re: f64 = m.iter().map(|c| c.re.abs()).fold(0.0_f64, f64::max);
    if max_im > 1e-10 * max_re.max(1e-300) {
        eprintln!(
            "svd: warning: imaginary part discarded (max |im|={:.3e})",
            max_im
        );
    }
    let rows = m.nrows();
    let cols = m.ncols();
    let ar: Vec<Vec<f64>> = (0..rows)
        .map(|i| (0..cols).map(|j| m[[i, j]].re).collect())
        .collect();
    let (u_r, sv, v_r) = svd_via_ata(&ar, rows, cols);
    let ns = rows.min(cols);
    Ok(Value::Tuple(vec![
        Value::Matrix(Array2::from_shape_fn((rows, rows), |(i, j)| {
            Complex::new(u_r[i][j], 0.0)
        })),
        Value::Vector(Array1::from_iter(
            sv[..ns].iter().map(|&s| Complex::new(s, 0.0)),
        )),
        Value::Matrix(Array2::from_shape_fn((cols, cols), |(i, j)| {
            Complex::new(v_r[i][j], 0.0)
        })),
    ]))
}

/// SVD via symmetric Jacobi eigendecomposition of A'A.
/// For a rows×cols real matrix A:
///   1. Compute S = A'A  (cols×cols, symmetric PSD)
///   2. Find eigendecomposition S = V * diag(lambda) * V' via symmetric Jacobi
///   3. Singular values sigma_i = sqrt(lambda_i), sorted descending
///   4. Left singular vectors: u_i = A * v_i / sigma_i
///   5. Fill remaining U columns via Gram-Schmidt
fn svd_via_ata(
    a: &[Vec<f64>],
    rows: usize,
    cols: usize,
) -> (Vec<Vec<f64>>, Vec<f64>, Vec<Vec<f64>>) {
    // Step 1: Compute S = A'A  (cols×cols)
    let mut s = vec![vec![0.0f64; cols]; cols];
    for i in 0..cols {
        for j in 0..cols {
            s[i][j] = (0..rows).map(|k| a[k][i] * a[k][j]).sum();
        }
    }

    // Step 2: Symmetric Jacobi eigendecomposition of S
    // Accumulate eigenvectors in V (cols×cols identity initially)
    let mut v: Vec<Vec<f64>> = (0..cols)
        .map(|i| {
            let mut row = vec![0.0f64; cols];
            row[i] = 1.0;
            row
        })
        .collect();

    let tol = 1e-14;
    for _ in 0..500 * cols * cols {
        let mut converged = true;
        for p in 0..cols {
            for q in (p + 1)..cols {
                let spq = s[p][q];
                if spq.abs() <= tol {
                    continue;
                }
                let spp = s[p][p];
                let sqq = s[q][q];
                let diff = sqq - spp;
                let theta = 0.5 * diff / spq;
                let t = if theta >= 0.0 {
                    1.0 / (theta + (1.0 + theta * theta).sqrt())
                } else {
                    -1.0 / (-theta + (1.0 + theta * theta).sqrt())
                };
                let c = 1.0 / (1.0 + t * t).sqrt();
                let s_rot = t * c;
                // Update S: symmetric Jacobi rotation
                s[p][p] = spp - t * spq;
                s[q][q] = sqq + t * spq;
                s[p][q] = 0.0;
                s[q][p] = 0.0;
                for r in 0..cols {
                    if r == p || r == q {
                        continue;
                    }
                    let srp = s[r][p];
                    let srq = s[r][q];
                    let new_rp = c * srp - s_rot * srq;
                    let new_rq = s_rot * srp + c * srq;
                    s[r][p] = new_rp;
                    s[p][r] = new_rp;
                    s[r][q] = new_rq;
                    s[q][r] = new_rq;
                }
                // Accumulate V
                for i in 0..cols {
                    let vip = v[i][p];
                    let viq = v[i][q];
                    v[i][p] = c * vip - s_rot * viq;
                    v[i][q] = s_rot * vip + c * viq;
                }
                converged = false;
            }
        }
        if converged {
            break;
        }
    }

    // Step 3: Extract eigenvalues (diagonal of S) and sort descending
    let mut pairs: Vec<(f64, usize)> = (0..cols).map(|j| (s[j][j].max(0.0).sqrt(), j)).collect();
    pairs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut sv = vec![0.0f64; cols];
    let mut v_out: Vec<Vec<f64>> = vec![vec![0.0f64; cols]; cols];
    for (new_j, &(sigma, old_j)) in pairs.iter().enumerate() {
        sv[new_j] = sigma;
        for i in 0..cols {
            v_out[i][new_j] = v[i][old_j];
        }
    }

    // Step 4: Compute U = A * V / sigma (left singular vectors)
    let mut u: Vec<Vec<f64>> = vec![vec![0.0f64; rows]; rows];
    let rank = pairs.iter().filter(|&&(s, _)| s > 1e-12).count();
    for j in 0..rank {
        let sigma = sv[j];
        // u_j = A * v_j / sigma
        for i in 0..rows {
            let val: f64 = (0..cols).map(|k| a[i][k] * v_out[k][j]).sum();
            u[i][j] = val / sigma;
        }
    }

    // Step 5: Fill remaining U columns via modified Gram-Schmidt
    let mut basis: Vec<Vec<f64>> = (0..rank)
        .map(|j| (0..rows).map(|i| u[i][j]).collect())
        .collect();
    let mut extra_idx = rank;
    for e in 0..rows {
        if extra_idx >= rows {
            break;
        }
        let mut cand = vec![0.0f64; rows];
        cand[e] = 1.0;
        for bv in &basis {
            let dot: f64 = cand.iter().zip(bv).map(|(a, b)| a * b).sum();
            for i in 0..rows {
                cand[i] -= dot * bv[i];
            }
        }
        let norm: f64 = cand.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-10 {
            for x in &mut cand {
                *x /= norm;
            }
            for i in 0..rows {
                u[i][extra_idx] = cand[i];
            }
            basis.push(cand);
            extra_idx += 1;
        }
    }

    (u, sv, v_out)
}

// ─── Streaming DSP ────────────────────────────────────────────────────────────

/// `state_init(n)` — allocate a FIR history buffer of n zeros.
/// n should be length(h) - 1 where h is the filter coefficient vector.
fn builtin_state_init(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("state_init", &args, 1)?;
    let n = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let buf = vec![C64::new(0.0, 0.0); n];
    Ok(Value::FirState(Arc::new(Mutex::new(buf))))
}

/// `filter_stream(frame, h, state)` — overlap-save FIR frame processing.
/// Returns `[output_frame, state]` (same Arc, not a copy).
///
/// Algorithm:
///   extended = [history..., frame...]   (length M-1 + N)
///   output[i] = Σ_k h[k] * extended[i + M-1 - k]   for i in 0..N
///   history  ← last M-1 samples of extended (= frame[N-M+1 .. N])
fn builtin_filter_stream(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("filter_stream", &args, 3)?;

    let frame = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let h = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let state = match &args[2] {
        Value::FirState(arc) => Arc::clone(arc),
        other => {
            return Err(ScriptError::type_err(format!(
                "filter_stream: expected fir_state for arg 3, got {}",
                other.type_name()
            )))
        }
    };

    let m = h.len(); // number of taps
    let n = frame.len(); // frame size

    if m == 0 {
        return Err(ScriptError::runtime(
            "filter_stream: h must be non-empty".to_string(),
        ));
    }
    if n == 0 {
        return Err(ScriptError::runtime(
            "filter_stream: frame must be non-empty".to_string(),
        ));
    }

    let history_len = m - 1;
    let mut history = state.lock().unwrap();

    if history.len() != history_len {
        return Err(ScriptError::runtime(format!(
            "filter_stream: state length {} does not match length(h)-1 = {} \
             (hint: use state_init(length(h)-1))",
            history.len(),
            history_len
        )));
    }

    // Build extended input: [history | frame]
    let mut extended = Vec::with_capacity(history_len + n);
    extended.extend_from_slice(&history);
    extended.extend(frame.iter().copied());

    // Compute N valid output samples via direct convolution inner product
    let mut output = Vec::with_capacity(n);
    for i in 0..n {
        let mut sum = C64::new(0.0, 0.0);
        for (k, &hk) in h.iter().enumerate() {
            sum += hk * extended[i + history_len - k];
        }
        output.push(sum);
    }

    // Update history: last M-1 samples of extended = frame[N-M+1 .. N]
    // (equivalently extended[N .. N+M-1])
    if history_len > 0 {
        history.copy_from_slice(&extended[n..n + history_len]);
    }
    drop(history); // release lock before building return value

    let out_vec: CVector = Array1::from_vec(output);
    let arc_clone = Arc::clone(&state);
    Ok(Value::Tuple(vec![
        Value::Vector(out_vec),
        Value::FirState(arc_clone),
    ]))
}

// ─── stdin/stdout Audio I/O ──────────────────────────────────────────────────

/// `audio_in(sr, n)` — create an audio input metadata handle (no hardware opened).
fn builtin_audio_in(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("audio_in", &args, 2)?;
    let sample_rate = args[0].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let frame_size = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
    Ok(Value::AudioIn {
        sample_rate,
        frame_size,
    })
}

/// `audio_out(sr, n)` — create an audio output metadata handle (no hardware opened).
fn builtin_audio_out(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("audio_out", &args, 2)?;
    let sample_rate = args[0].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    let frame_size = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
    Ok(Value::AudioOut {
        sample_rate,
        frame_size,
    })
}

/// `audio_read(adc)` — read one frame of f32-LE PCM from stdin.
/// Blocks until the full frame is available.
fn builtin_audio_read(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("audio_read", &args, 1)?;
    let frame_size = match &args[0] {
        Value::AudioIn { frame_size, .. } => *frame_size,
        other => {
            return Err(ScriptError::type_err(format!(
                "audio_read: expected audio_in, got {}",
                other.type_name()
            )))
        }
    };
    let mut buf = vec![0u8; frame_size * 4];
    match std::io::stdin().lock().read_exact(&mut buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(ScriptError::AudioEof);
        }
        Err(e) => return Err(ScriptError::runtime(format!("audio_read: {e}"))),
    };
    let cvec: CVector = Array1::from_iter(buf.chunks_exact(4).map(|b| {
        let s = f32::from_le_bytes(b.try_into().unwrap());
        C64::new(s as f64, 0.0)
    }));
    Ok(Value::Vector(cvec))
}

/// `audio_write(dac, frame)` — write one frame of f32-LE PCM to stdout.
/// Only the real part of each sample is written.
fn builtin_audio_write(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("audio_write", &args, 2)?;
    match &args[0] {
        Value::AudioOut { .. } => {}
        other => {
            return Err(ScriptError::type_err(format!(
                "audio_write: expected audio_out, got {}",
                other.type_name()
            )))
        }
    };
    let frame = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let mut out = std::io::stdout().lock();
    for c in frame.iter() {
        out.write_all(&(c.re as f32).to_le_bytes())
            .map_err(|e| ScriptError::runtime(format!("audio_write: {e}")))?;
    }
    out.flush()
        .map_err(|e| ScriptError::runtime(format!("audio_write flush: {e}")))?;
    Ok(Value::None)
}

// ─── Live plotting ─────────────────────────────────────────────────────────

/// `figure_live(rows, cols)` — open a persistent live terminal plot.
/// Returns a `Value::LiveFigure` handle.  Errors if stdout is not a tty.
fn builtin_figure_live(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("figure_live", &args, 2)?;
    let rows = args[0].to_usize().map_err(|e| ScriptError::runtime(e))?;
    let cols = args[1].to_usize().map_err(|e| ScriptError::runtime(e))?;

    // --plot none: refuse before either backend is consulted — the user
    // explicitly opted out of interactive plotting, regardless of whether a
    // viewer happens to be running on this host.
    if rustlab_plot::plot_context() == rustlab_plot::PlotContext::Headless {
        return Err(ScriptError::runtime(
            rustlab_plot::PlotError::HeadlessDisabled.to_string(),
        ));
    }

    // When the viewer feature is enabled, try to connect to a running
    // rustlab-viewer first.  Fall back to ratatui if the viewer is not up.
    #[cfg(feature = "viewer")]
    {
        if let Some(vf) = rustlab_plot::ViewerFigure::connect(rows, cols) {
            let boxed: Box<dyn LivePlot> = Box::new(vf);
            return Ok(Value::LiveFigure(Arc::new(Mutex::new(Some(boxed)))));
        }
    }

    let fig = LiveFigure::new(rows, cols).map_err(|e| ScriptError::runtime(e.to_string()))?;
    let boxed: Box<dyn LivePlot> = Box::new(fig);
    Ok(Value::LiveFigure(Arc::new(Mutex::new(Some(boxed)))))
}

/// `plot_update(fig, panel, y)` or `plot_update(fig, panel, x, y)` —
/// replace the data in the given 1-based panel.  Does not redraw.
fn builtin_plot_update(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("plot_update", &args, 3, 4)?;
    let Value::LiveFigure(fig) = &args[0] else {
        return Err(ScriptError::runtime(format!(
            "plot_update: expected live_figure, got {}",
            args[0].type_name()
        )));
    };
    let panel = args[1]
        .to_usize()
        .map_err(|e| ScriptError::runtime(e))?
        .saturating_sub(1); // 1-based → 0-based
    let (x, y) = if args.len() == 4 {
        let x = args[2]
            .to_cvector()
            .map_err(|e| ScriptError::runtime(e))?
            .iter()
            .map(|c| c.re)
            .collect::<Vec<_>>();
        let y = args[3]
            .to_cvector()
            .map_err(|e| ScriptError::runtime(e))?
            .iter()
            .map(|c| c.re)
            .collect::<Vec<_>>();
        (x, y)
    } else {
        let y = args[2]
            .to_cvector()
            .map_err(|e| ScriptError::runtime(e))?
            .iter()
            .map(|c| c.re)
            .collect::<Vec<_>>();
        let x = (1..=y.len()).map(|i| i as f64).collect::<Vec<_>>();
        (x, y)
    };
    fig.lock()
        .unwrap()
        .as_mut()
        .ok_or_else(|| ScriptError::runtime("plot_update: figure is closed".to_string()))?
        .update_panel(panel, x, y);
    Ok(Value::None)
}

/// `plot_limits(fig, panel, xlim, ylim)` — set fixed axis limits on a live panel.
/// Pass `[lo, hi]` vectors.  Use `[-200, 0]` for typical dB range.
fn builtin_plot_limits(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("plot_limits", &args, 4)?;
    let Value::LiveFigure(fig) = &args[0] else {
        return Err(ScriptError::runtime(format!(
            "plot_limits: expected live_figure, got {}",
            args[0].type_name()
        )));
    };
    let panel = args[1]
        .to_usize()
        .map_err(|e| ScriptError::runtime(e))?
        .saturating_sub(1);
    let xlim_v = args[2].to_cvector().map_err(|e| ScriptError::runtime(e))?;
    let ylim_v = args[3].to_cvector().map_err(|e| ScriptError::runtime(e))?;
    let xlim = if xlim_v.len() >= 2 {
        (Some(xlim_v[0].re), Some(xlim_v[1].re))
    } else {
        (None, None)
    };
    let ylim = if ylim_v.len() >= 2 {
        (Some(ylim_v[0].re), Some(ylim_v[1].re))
    } else {
        (None, None)
    };
    fig.lock()
        .unwrap()
        .as_mut()
        .ok_or_else(|| ScriptError::runtime("plot_limits: figure is closed".to_string()))?
        .set_panel_limits(panel, xlim, ylim);
    Ok(Value::None)
}

/// `plot_labels(fig, panel, title, xlabel, ylabel)` — set title and axis labels
/// on a live figure panel.
fn builtin_plot_labels(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("plot_labels", &args, 5)?;
    let Value::LiveFigure(fig) = &args[0] else {
        return Err(ScriptError::runtime(format!(
            "plot_labels: expected live_figure, got {}",
            args[0].type_name()
        )));
    };
    let panel = args[1]
        .to_usize()
        .map_err(|e| ScriptError::runtime(e))?
        .saturating_sub(1);
    let title = args[2].to_str().map_err(|e| ScriptError::type_err(e))?;
    let xlabel = args[3].to_str().map_err(|e| ScriptError::type_err(e))?;
    let ylabel = args[4].to_str().map_err(|e| ScriptError::type_err(e))?;
    fig.lock()
        .unwrap()
        .as_mut()
        .ok_or_else(|| ScriptError::runtime("plot_labels: figure is closed".to_string()))?
        .set_panel_labels(panel, &title, &xlabel, &ylabel);
    Ok(Value::None)
}

/// `figure_draw(fig)` — flush all panel data to the terminal in one refresh.
fn builtin_figure_draw(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("figure_draw", &args, 1)?;
    let Value::LiveFigure(fig) = &args[0] else {
        return Err(ScriptError::runtime(format!(
            "figure_draw: expected live_figure, got {}",
            args[0].type_name()
        )));
    };
    let result = fig
        .lock()
        .unwrap()
        .as_mut()
        .ok_or_else(|| ScriptError::runtime("figure_draw: figure is closed".to_string()))?
        .redraw();
    match result {
        Ok(()) => Ok(Value::None),
        Err(rustlab_plot::PlotError::Interrupted) => Err(ScriptError::Interrupted),
        Err(e) => Err(ScriptError::runtime(e.to_string())),
    }
}

/// `figure_close(fig)` — restore the terminal and release the figure.
/// After this call the figure handle is inert; further draw/update calls error.
fn builtin_figure_close(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("figure_close", &args, 1)?;
    let Value::LiveFigure(fig) = &args[0] else {
        return Err(ScriptError::runtime(format!(
            "figure_close: expected live_figure, got {}",
            args[0].type_name()
        )));
    };
    // .take() drops the LiveFigure, firing Drop → disable_raw_mode + LeaveAlternateScreen.
    fig.lock().unwrap().take();
    Ok(Value::None)
}

/// `mag2db(X)` — convert magnitude to dB: 20·log10(|X|), floored at −200 dB.
/// The 1e-10 floor prevents −inf from appearing in output, mapping silence to −200 dB.
fn builtin_mag2db(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("mag2db", &args, 1)?;
    match &args[0] {
        Value::Scalar(x) => Ok(Value::Scalar(20.0 * x.abs().max(1e-10).log10())),
        Value::Complex(c) => Ok(Value::Scalar(20.0 * c.norm().max(1e-10).log10())),
        Value::Vector(v) => {
            let out: CVector = v
                .iter()
                .map(|c| Complex::new(20.0 * c.norm().max(1e-10).log10(), 0.0))
                .collect();
            Ok(Value::Vector(out))
        }
        Value::Matrix(m) => {
            let out = m.mapv(|c| Complex::new(20.0 * c.norm().max(1e-10).log10(), 0.0));
            Ok(Value::Matrix(out))
        }
        other => Err(ScriptError::runtime(format!(
            "mag2db: expected numeric, got {}",
            other.type_name()
        ))),
    }
}

// ─── Sparse builtins ──────────────────────────────────────────────────────────

/// `sparse(I, J, V, m, n)` — build sparse matrix from index/value vectors (1-based).
/// `sparse(A)` — convert dense matrix/vector/scalar to sparse.
fn builtin_sparse(args: Vec<Value>) -> Result<Value, ScriptError> {
    if args.len() == 1 {
        // Dense → sparse conversion
        return match &args[0] {
            Value::Vector(v) => Ok(Value::SparseVector(SparseVec::from_dense(v))),
            Value::Matrix(m) => Ok(Value::SparseMatrix(SparseMat::from_dense(m))),
            Value::Scalar(n) => {
                let c = Complex::new(*n, 0.0);
                let m = Array2::from_elem((1, 1), c);
                Ok(Value::SparseMatrix(SparseMat::from_dense(&m)))
            }
            Value::Complex(c) => {
                let m = Array2::from_elem((1, 1), *c);
                Ok(Value::SparseMatrix(SparseMat::from_dense(&m)))
            }
            Value::SparseVector(_) | Value::SparseMatrix(_) => Ok(args.into_iter().next().unwrap()),
            other => Err(ScriptError::type_err(format!(
                "sparse: cannot convert {} to sparse",
                other.type_name()
            ))),
        };
    }
    check_args("sparse", &args, 5)?;
    let i_vec = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let j_vec = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let v_vec = args[2].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let m = args[3].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let n = args[4].to_usize().map_err(|e| ScriptError::type_err(e))?;
    if i_vec.len() != j_vec.len() || i_vec.len() != v_vec.len() {
        return Err(ScriptError::runtime(
            "sparse: I, J, V must have the same length".to_string(),
        ));
    }
    let mut entries = Vec::with_capacity(i_vec.len());
    for k in 0..i_vec.len() {
        let ri = i_vec[k].re as usize;
        let ci = j_vec[k].re as usize;
        if ri < 1 || ri > m {
            return Err(ScriptError::runtime(format!(
                "sparse: row index {} out of range [1, {}]",
                ri, m
            )));
        }
        if ci < 1 || ci > n {
            return Err(ScriptError::runtime(format!(
                "sparse: col index {} out of range [1, {}]",
                ci, n
            )));
        }
        entries.push((ri - 1, ci - 1, v_vec[k]));
    }
    Ok(Value::SparseMatrix(SparseMat::new(m, n, entries)))
}

/// `sparsevec(I, V, n)` — build sparse vector from index/value vectors (1-based).
fn builtin_sparsevec(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("sparsevec", &args, 3)?;
    let i_vec = args[0].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let v_vec = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;
    let n = args[2].to_usize().map_err(|e| ScriptError::type_err(e))?;
    if i_vec.len() != v_vec.len() {
        return Err(ScriptError::runtime(
            "sparsevec: I and V must have the same length".to_string(),
        ));
    }
    let mut entries = Vec::with_capacity(i_vec.len());
    for k in 0..i_vec.len() {
        let idx = i_vec[k].re as usize;
        if idx < 1 || idx > n {
            return Err(ScriptError::runtime(format!(
                "sparsevec: index {} out of range [1, {}]",
                idx, n
            )));
        }
        entries.push((idx - 1, v_vec[k]));
    }
    Ok(Value::SparseVector(SparseVec::new(n, entries)))
}

/// `speye(n)` — n×n sparse identity matrix.
fn builtin_speye(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("speye", &args, 1)?;
    let n = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let entries: Vec<_> = (0..n).map(|i| (i, i, Complex::new(1.0, 0.0))).collect();
    Ok(Value::SparseMatrix(SparseMat {
        rows: n,
        cols: n,
        entries,
    }))
}

/// `spzeros(m, n)` — m×n all-zero sparse matrix.
fn builtin_spzeros(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("spzeros", &args, 2)?;
    let m = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let n = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
    Ok(Value::SparseMatrix(SparseMat {
        rows: m,
        cols: n,
        entries: Vec::new(),
    }))
}

/// `nnz(S)` — number of non-zero entries (for dense, returns numel).
fn builtin_nnz(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("nnz", &args, 1)?;
    let count = match &args[0] {
        Value::SparseVector(sv) => sv.nnz(),
        Value::SparseMatrix(sm) => sm.nnz(),
        Value::Vector(v) => v.len(),
        Value::Matrix(m) => m.nrows() * m.ncols(),
        Value::Scalar(_) | Value::Complex(_) => 1,
        other => {
            return Err(ScriptError::type_err(format!(
                "nnz: unsupported type {}",
                other.type_name()
            )))
        }
    };
    Ok(Value::Scalar(count as f64))
}

/// `issparse(x)` — returns 1 if x is sparse, 0 otherwise.
fn builtin_issparse(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("issparse", &args, 1)?;
    let is = matches!(&args[0], Value::SparseVector(_) | Value::SparseMatrix(_));
    Ok(Value::Scalar(if is { 1.0 } else { 0.0 }))
}

/// `full(S)` — convert sparse to dense. Dense inputs pass through.
fn builtin_full(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("full", &args, 1)?;
    match args.into_iter().next().unwrap() {
        Value::SparseVector(sv) => Ok(Value::Vector(sv.to_dense())),
        Value::SparseMatrix(sm) => Ok(Value::Matrix(sm.to_dense())),
        other => Ok(other), // identity for dense
    }
}

/// `nonzeros(S)` — return a vector of the non-zero values in storage order.
fn builtin_nonzeros(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("nonzeros", &args, 1)?;
    match &args[0] {
        Value::SparseVector(sv) => {
            let vals: Vec<C64> = sv.entries.iter().map(|&(_, v)| v).collect();
            Ok(Value::Vector(Array1::from_vec(vals)))
        }
        Value::SparseMatrix(sm) => {
            let vals: Vec<C64> = sm.entries.iter().map(|&(_, _, v)| v).collect();
            Ok(Value::Vector(Array1::from_vec(vals)))
        }
        other => Err(ScriptError::type_err(format!(
            "nonzeros: expected sparse, got {}",
            other.type_name()
        ))),
    }
}

/// `find(X)` — 1-based positions of nonzero elements.
///
/// For dense vectors, returns a vector of 1-based element indices.
/// For dense matrices, returns a vector of 1-based **column-major** linear
/// indices (octave/matlab convention): element `M(i, j)` lands at linear
/// index `(j - 1) * nrows + i`.
/// For sparse vectors, returns the tuple `[I, V]`.
/// For sparse matrices, returns the tuple `[I, J, V]`.
/// Nargout-aware dispatch for `find`:
///   nargout == 1 (default): linear (column-major) indices for dense input;
///                           `[I, V]` tuple for sparse vectors,
///                           `[I, J, V]` for sparse matrices.
///   nargout == 2 on dense matrix: `[I, J]` row+col subscripts (1-based).
///   nargout == 2 on dense vector: `[I, V]` indices + values.
///   nargout >= 3 on dense matrix: `[I, J, V]` row + col + nonzero values.
fn builtin_find_nargout(args: Vec<Value>, nargout: usize) -> Result<Value, ScriptError> {
    if nargout < 2 {
        return builtin_find(args);
    }
    check_args("find", &args, 1)?;
    match &args[0] {
        Value::Vector(v) => {
            let mut idx: Vec<C64> = Vec::new();
            let mut vals: Vec<C64> = Vec::new();
            for (i, &c) in v.iter().enumerate() {
                if c.re != 0.0 || c.im != 0.0 {
                    idx.push(Complex::new((i + 1) as f64, 0.0));
                    vals.push(c);
                }
            }
            Ok(Value::Tuple(vec![
                Value::Vector(Array1::from_vec(idx)),
                Value::Vector(Array1::from_vec(vals)),
            ]))
        }
        Value::Matrix(m) => {
            let nrows = m.nrows();
            let ncols = m.ncols();
            let mut rows: Vec<C64> = Vec::new();
            let mut cols: Vec<C64> = Vec::new();
            let mut vals: Vec<C64> = Vec::new();
            for c in 0..ncols {
                for r in 0..nrows {
                    let z = m[[r, c]];
                    if z.re != 0.0 || z.im != 0.0 {
                        rows.push(Complex::new((r + 1) as f64, 0.0));
                        cols.push(Complex::new((c + 1) as f64, 0.0));
                        vals.push(z);
                    }
                }
            }
            let row_v = Value::Vector(Array1::from_vec(rows));
            let col_v = Value::Vector(Array1::from_vec(cols));
            let val_v = Value::Vector(Array1::from_vec(vals));
            if nargout >= 3 {
                Ok(Value::Tuple(vec![row_v, col_v, val_v]))
            } else {
                Ok(Value::Tuple(vec![row_v, col_v]))
            }
        }
        // Sparse paths: existing single-output forms already produce tuples.
        Value::SparseVector(_) | Value::SparseMatrix(_) => builtin_find(args),
        Value::Scalar(n) => {
            // Trivial 1-element case: return the same shape as the dense
            // single-output form, in tuple form.
            if *n != 0.0 {
                Ok(Value::Tuple(vec![
                    Value::Vector(Array1::from_vec(vec![Complex::new(1.0, 0.0)])),
                    Value::Vector(Array1::from_vec(vec![Complex::new(*n, 0.0)])),
                ]))
            } else {
                Ok(Value::Tuple(vec![
                    Value::Vector(Array1::from_vec(vec![])),
                    Value::Vector(Array1::from_vec(vec![])),
                ]))
            }
        }
        other => Err(ScriptError::type_err(format!(
            "find: expected vector, matrix, or sparse; got {}",
            other.type_name()
        ))),
    }
}

fn builtin_find(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("find", &args, 1)?;
    match &args[0] {
        Value::Vector(v) => {
            let indices: Vec<C64> = v
                .iter()
                .enumerate()
                .filter(|(_, c)| c.re != 0.0 || c.im != 0.0)
                .map(|(i, _)| Complex::new((i + 1) as f64, 0.0))
                .collect();
            Ok(Value::Vector(Array1::from_vec(indices)))
        }
        Value::Matrix(m) => {
            let nrows = m.nrows();
            let ncols = m.ncols();
            let mut indices: Vec<C64> = Vec::new();
            // Walk column-major so the resulting linear indices match
            // octave/matlab's `M(:)` traversal order.
            for c in 0..ncols {
                for r in 0..nrows {
                    let z = m[[r, c]];
                    if z.re != 0.0 || z.im != 0.0 {
                        let lin = c * nrows + r + 1;
                        indices.push(Complex::new(lin as f64, 0.0));
                    }
                }
            }
            Ok(Value::Vector(Array1::from_vec(indices)))
        }
        Value::SparseVector(sv) => {
            let indices: Vec<C64> = sv
                .entries
                .iter()
                .map(|&(i, _)| Complex::new((i + 1) as f64, 0.0))
                .collect();
            let values: Vec<C64> = sv.entries.iter().map(|&(_, v)| v).collect();
            Ok(Value::Tuple(vec![
                Value::Vector(Array1::from_vec(indices)),
                Value::Vector(Array1::from_vec(values)),
            ]))
        }
        Value::SparseMatrix(sm) => {
            let rows: Vec<C64> = sm
                .entries
                .iter()
                .map(|&(r, _, _)| Complex::new((r + 1) as f64, 0.0))
                .collect();
            let cols: Vec<C64> = sm
                .entries
                .iter()
                .map(|&(_, c, _)| Complex::new((c + 1) as f64, 0.0))
                .collect();
            let vals: Vec<C64> = sm.entries.iter().map(|&(_, _, v)| v).collect();
            Ok(Value::Tuple(vec![
                Value::Vector(Array1::from_vec(rows)),
                Value::Vector(Array1::from_vec(cols)),
                Value::Vector(Array1::from_vec(vals)),
            ]))
        }
        Value::Scalar(n) => {
            // Scalar: 1-element "vector"; index 1 if nonzero, empty otherwise.
            if *n != 0.0 {
                Ok(Value::Vector(Array1::from_vec(vec![Complex::new(1.0, 0.0)])))
            } else {
                Ok(Value::Vector(Array1::from_vec(vec![])))
            }
        }
        other => Err(ScriptError::type_err(format!(
            "find: expected vector, matrix, sparse vector, or sparse matrix; got {}",
            other.type_name()
        ))),
    }
}

/// `spsolve(A, b)` — solve A*x = b where A is sparse.
/// Internally converts to dense and uses Gaussian elimination.
/// `spsolve(A, b)` or `spsolve(A, b, mode)` — solve `A x = b` where `A` is sparse.
///
/// `mode` is one of `"auto"` (default), `"cholesky"`, or `"lu"`.
/// - `"auto"` detects Hermitian-positive-definite structure and routes
///   to the hand-rolled sparse Cholesky; otherwise it uses the
///   hand-rolled sparse LU with partial pivoting. Either path stays
///   sparse end-to-end and uses the `AmdOrdering` fill-reducing
///   permutation by default.
/// - `"cholesky"` forces the SPD path; returns an error if the input is
///   not Hermitian positive definite.
/// - `"lu"` forces the sparse LU path. Useful when you know the matrix
///   isn't Hermitian and want to skip the SPD pre-check.
///
/// Dense `Value::Matrix` input is dispatched through the legacy dense
/// Gaussian-elimination path — sparse factorization is reserved for
/// sparse input. (Sparse-from-dense conversion is cheap; users who want
/// the sparse paths on a dense matrix can call `sparse(A)` first.)
fn builtin_spsolve(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("spsolve", &args, 2, 3)?;
    let mode = if args.len() == 3 {
        match &args[2] {
            Value::Str(s) => s.as_str(),
            other => {
                return Err(ScriptError::type_err(format!(
                    "spsolve: mode must be a string (\"auto\"|\"cholesky\"|\"lu\"), got {}",
                    other.type_name()
                )));
            }
        }
    } else {
        "auto"
    };
    if !matches!(mode, "auto" | "cholesky" | "lu") {
        return Err(ScriptError::type_err(format!(
            "spsolve: unknown mode \"{mode}\"; expected \"auto\", \"cholesky\", or \"lu\""
        )));
    }

    let b = args[1].to_cvector().map_err(|e| ScriptError::type_err(e))?;

    let sparse_input: Option<&SparseMat> = match &args[0] {
        Value::SparseMatrix(sm) => Some(sm),
        Value::Matrix(_) => None,
        other => {
            return Err(ScriptError::type_err(format!(
                "spsolve: A must be a sparse or dense matrix, got {}",
                other.type_name()
            )));
        }
    };

    if let Some(sm) = sparse_input {
        if sm.rows != sm.cols {
            return Err(ScriptError::type_err(format!(
                "spsolve: A must be square (got {}×{})",
                sm.rows, sm.cols
            )));
        }
        if sm.rows != b.len() {
            return Err(ScriptError::type_err(format!(
                "spsolve: A is {}×{} but b has length {}",
                sm.rows,
                sm.cols,
                b.len()
            )));
        }

        // SPD path: explicit "cholesky", or "auto" when SPD detected.
        let try_cholesky = match mode {
            "auto" => sm.is_spd_estimate(1e-10),
            "cholesky" => true,
            "lu" => false,
            _ => unreachable!(),
        };

        if try_cholesky {
            match try_sparse_cholesky(sm, &b) {
                Ok(x) => return Ok(spsolve_pack(x)),
                Err(e) => {
                    if mode == "cholesky" {
                        return Err(ScriptError::type_err(format!("spsolve: {e}")));
                    }
                    // "auto" silently falls through to sparse LU.
                }
            }
        }

        // Sparse LU path: explicit "lu", or "auto" fallback when Cholesky is unavailable.
        match try_sparse_lu(sm, &b) {
            Ok(x) => return Ok(spsolve_pack(x)),
            Err(e) => {
                return Err(ScriptError::type_err(format!("spsolve: {e}")));
            }
        }
    }

    // Dense input falls through to the legacy dense path.
    let a_dense = match &args[0] {
        Value::SparseMatrix(_) => unreachable!(),
        Value::Matrix(m) => m.clone(),
        _ => unreachable!(),
    };
    let x = dense_lu_solve(&a_dense, &b)?;
    Ok(spsolve_pack(x))
}

/// Try the sparse Cholesky path. Detects "essentially real" inputs and
/// routes to the `f64` solver to skip the 4× cost of complex factorization.
fn try_sparse_cholesky(sm: &SparseMat, b: &CVector) -> Result<CVector, String> {
    use rustlab_core::sparse_solve::{AmdOrdering, SparseChol, SparseCsc, SparseSolveError};

    let all_real = sm.entries.iter().all(|(_, _, v)| v.im.abs() < 1e-12)
        && b.iter().all(|c| c.im.abs() < 1e-12);

    if all_real {
        let csc: SparseCsc<f64> = sm
            .to_csc()
            .map_err(|e: SparseSolveError| e.to_string())?;
        let chol = SparseChol::factor(&csc, &AmdOrdering).map_err(|e| e.to_string())?;
        let b_real: Vec<f64> = b.iter().map(|c| c.re).collect();
        let x_real = chol.solve(&b_real).map_err(|e| e.to_string())?;
        Ok(Array1::from_iter(
            x_real.into_iter().map(|v| Complex::new(v, 0.0)),
        ))
    } else {
        let csc: SparseCsc<C64> = sm
            .to_csc()
            .map_err(|e: SparseSolveError| e.to_string())?;
        let chol = SparseChol::factor(&csc, &AmdOrdering).map_err(|e| e.to_string())?;
        let b_vec: Vec<C64> = b.iter().copied().collect();
        let x_vec = chol.solve(&b_vec).map_err(|e| e.to_string())?;
        Ok(Array1::from_vec(x_vec))
    }
}

/// Sparse LU with partial pivoting. Same real-vs-complex auto-routing
/// as the Cholesky path. Pivoting threshold is the standard 0.1 (Trefethen).
fn try_sparse_lu(sm: &SparseMat, b: &CVector) -> Result<CVector, String> {
    use rustlab_core::sparse_solve::{AmdOrdering, SparseCsc, SparseLU, SparseSolveError};

    let all_real = sm.entries.iter().all(|(_, _, v)| v.im.abs() < 1e-12)
        && b.iter().all(|c| c.im.abs() < 1e-12);

    if all_real {
        let csc: SparseCsc<f64> = sm
            .to_csc()
            .map_err(|e: SparseSolveError| e.to_string())?;
        let lu = SparseLU::factor(&csc, &AmdOrdering, 0.1).map_err(|e| e.to_string())?;
        let b_real: Vec<f64> = b.iter().map(|c| c.re).collect();
        let x_real = lu.solve(&b_real).map_err(|e| e.to_string())?;
        Ok(Array1::from_iter(
            x_real.into_iter().map(|v| Complex::new(v, 0.0)),
        ))
    } else {
        let csc: SparseCsc<C64> = sm
            .to_csc()
            .map_err(|e: SparseSolveError| e.to_string())?;
        let lu = SparseLU::factor(&csc, &AmdOrdering, 0.1).map_err(|e| e.to_string())?;
        let b_vec: Vec<C64> = b.iter().copied().collect();
        let x_vec = lu.solve(&b_vec).map_err(|e| e.to_string())?;
        Ok(Array1::from_vec(x_vec))
    }
}

/// Dense Gaussian elimination with partial pivoting — the original
/// `spsolve` implementation, retained as a fallback for non-SPD problems
/// until Phase 3 ships a sparse LU.
fn dense_lu_solve(a: &CMatrix, b: &CVector) -> Result<CVector, ScriptError> {
    let n = a.nrows();
    if n != a.ncols() {
        return Err(ScriptError::type_err(format!(
            "spsolve: A must be square (got {}×{})",
            n,
            a.ncols()
        )));
    }
    if n != b.len() {
        return Err(ScriptError::type_err(format!(
            "spsolve: A is {}×{} but b has length {}",
            n,
            n,
            b.len()
        )));
    }
    let mut aug: Array2<C64> = Array2::zeros((n, n + 1));
    for i in 0..n {
        for j in 0..n {
            aug[[i, j]] = a[[i, j]];
        }
        aug[[i, n]] = b[i];
    }
    for k in 0..n {
        let mut max_idx = k;
        let mut max_val = aug[[k, k]].norm();
        for i in k + 1..n {
            let v = aug[[i, k]].norm();
            if v > max_val {
                max_val = v;
                max_idx = i;
            }
        }
        if max_idx != k {
            for j in 0..n + 1 {
                let tmp = aug[[k, j]];
                aug[[k, j]] = aug[[max_idx, j]];
                aug[[max_idx, j]] = tmp;
            }
        }
        if aug[[k, k]].norm() < 1e-14 {
            return Err(ScriptError::type_err(
                "spsolve: matrix is singular or nearly singular".to_string(),
            ));
        }
        for i in k + 1..n {
            let factor = aug[[i, k]] / aug[[k, k]];
            for j in k..n + 1 {
                let sub = factor * aug[[k, j]];
                aug[[i, j]] -= sub;
            }
        }
    }
    let mut x: CVector = Array1::zeros(n);
    for i in (0..n).rev() {
        let mut s = aug[[i, n]];
        for j in i + 1..n {
            s -= aug[[i, j]] * x[j];
        }
        x[i] = s / aug[[i, i]];
    }
    Ok(x)
}

/// Wrap an `spsolve` result vector. Length-1 returns are unboxed to
/// `Scalar` (real) or `Complex` so callers can use them in scalar
/// context — matches existing rustlab convention.
fn spsolve_pack(x: CVector) -> Value {
    if x.len() == 1 {
        let c = x[0];
        if c.im.abs() < 1e-12 {
            Value::Scalar(c.re)
        } else {
            Value::Complex(c)
        }
    } else {
        Value::Vector(x)
    }
}

/// `spdiags(V, D, m, n)` — place diagonals into an m×n sparse matrix.
/// V is a vector (single diagonal) or matrix (one column per diagonal).
/// D is a scalar or vector of diagonal offsets (0 = main, >0 super, <0 sub).
fn builtin_spdiags(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("spdiags", &args, 4)?;
    let m = args[2].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let n = args[3].to_usize().map_err(|e| ScriptError::type_err(e))?;

    // Parse diagonal offsets
    let diags: Vec<i64> = match &args[1] {
        Value::Scalar(d) => vec![*d as i64],
        Value::Vector(v) => v.iter().map(|c| c.re as i64).collect(),
        other => {
            return Err(ScriptError::type_err(format!(
                "spdiags: D must be a scalar or vector, got {}",
                other.type_name()
            )))
        }
    };

    // Parse values: vector for single diagonal, matrix for multiple (one column per diag)
    let mut entries: Vec<(usize, usize, C64)> = Vec::new();
    match &args[0] {
        Value::Vector(v) => {
            if diags.len() != 1 {
                return Err(ScriptError::runtime(
                    "spdiags: when V is a vector, D must be a single diagonal offset".to_string(),
                ));
            }
            let d = diags[0];
            for (idx, &val) in v.iter().enumerate() {
                let (r, c) = if d >= 0 {
                    (idx, idx + d as usize)
                } else {
                    (idx + (-d) as usize, idx)
                };
                if r < m && c < n {
                    entries.push((r, c, val));
                }
            }
        }
        Value::Matrix(mat) => {
            if mat.ncols() != diags.len() {
                return Err(ScriptError::runtime(format!(
                    "spdiags: V has {} columns but D has {} offsets",
                    mat.ncols(),
                    diags.len()
                )));
            }
            for (col_idx, &d) in diags.iter().enumerate() {
                for row_idx in 0..mat.nrows() {
                    let val = mat[[row_idx, col_idx]];
                    let (r, c) = if d >= 0 {
                        (row_idx, row_idx + d as usize)
                    } else {
                        (row_idx + (-d) as usize, row_idx)
                    };
                    if r < m && c < n {
                        entries.push((r, c, val));
                    }
                }
            }
        }
        Value::Scalar(s) => {
            if diags.len() != 1 {
                return Err(ScriptError::runtime(
                    "spdiags: when V is a scalar, D must be a single diagonal offset".to_string(),
                ));
            }
            let d = diags[0];
            let val = Complex::new(*s, 0.0);
            let diag_len = if d >= 0 {
                (m).min(n.saturating_sub(d as usize))
            } else {
                (n).min(m.saturating_sub((-d) as usize))
            };
            for idx in 0..diag_len {
                let (r, c) = if d >= 0 {
                    (idx, idx + d as usize)
                } else {
                    (idx + (-d) as usize, idx)
                };
                entries.push((r, c, val));
            }
        }
        other => {
            return Err(ScriptError::type_err(format!(
                "spdiags: V must be a scalar, vector, or matrix, got {}",
                other.type_name()
            )))
        }
    }

    Ok(Value::SparseMatrix(SparseMat::new(m, n, entries)))
}

// ─── Laplacian stencil builder and index sugar ──────────────────────────────
//
// Node-ordering convention: **column-major** (Octave/rustlab convention). For
// a grid `V(i, j)` with `i ∈ 1..=ny` (row) and `j ∈ 1..=nx` (col), the flat
// index is `k = (j-1)*ny + i` (1-based). This composes naturally with the
// existing `reshape(V_flat, ny, nx)` and `V_grid(:)` idioms, so users can
// write `V = spsolve(L, rhs); V_grid = reshape(V, ny, nx)` without a
// transpose. The `ij2k` / `k2ij` sugar takes `ny` (the row-count) as the
// divider — **not** `nx`.

/// `laplacian_2d(nx, ny [, dx, dy])` — sparse 5-point Laplacian stencil with
/// homogeneous-Dirichlet boundary conditions. Approximates `+∇²` so Poisson
/// `∇²V = -rho/eps0` solves as `V = spsolve(L, -rho/eps0)`.
/// Common arg-parsing helper for the laplacian_* family. Extracts an
/// optional trailing string argument as a `BoundaryCondition`, defaulting
/// to Dirichlet.
fn parse_bc_arg(name: &str, value: Option<&Value>) -> Result<rustlab_dsp::BoundaryCondition, ScriptError> {
    match value {
        None => Ok(rustlab_dsp::BoundaryCondition::Dirichlet),
        Some(Value::Str(s)) => rustlab_dsp::BoundaryCondition::from_str(s.as_str())
            .map_err(|e| ScriptError::type_err(format!("{name}: {e}"))),
        Some(other) => Err(ScriptError::type_err(format!(
            "{name}: bc must be a string (\"dirichlet\"|\"neumann\"|\"periodic\"), got {}",
            other.type_name()
        ))),
    }
}

/// `laplacian_2d(nx, ny [, dx, dy] [, bc])` — sparse 5-point Laplacian on
/// a uniform grid with the requested boundary condition.
fn builtin_laplacian_2d(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("laplacian_2d", &args, 2, 5)?;
    let nx = args[0]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("laplacian_2d: nx: {e}")))?;
    let ny = args[1]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("laplacian_2d: ny: {e}")))?;

    // Argument shape:
    //   2 args: (nx, ny)
    //   3 args: (nx, ny, bc)
    //   4 args: (nx, ny, dx, dy)
    //   5 args: (nx, ny, dx, dy, bc)
    let (dx, dy, bc) = match args.len() {
        2 => (1.0, 1.0, rustlab_dsp::BoundaryCondition::Dirichlet),
        3 => {
            let bc = parse_bc_arg("laplacian_2d", Some(&args[2]))?;
            (1.0, 1.0, bc)
        }
        4 => {
            let dx = args[2]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_2d: dx: {e}")))?;
            let dy = args[3]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_2d: dy: {e}")))?;
            (dx, dy, rustlab_dsp::BoundaryCondition::Dirichlet)
        }
        5 => {
            let dx = args[2]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_2d: dx: {e}")))?;
            let dy = args[3]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_2d: dy: {e}")))?;
            let bc = parse_bc_arg("laplacian_2d", Some(&args[4]))?;
            (dx, dy, bc)
        }
        _ => unreachable!(),
    };

    let l = rustlab_dsp::laplacian::laplacian_2d_bc(nx, ny, dx, dy, bc)
        .map_err(|e| ScriptError::type_err(e.to_string()))?;
    Ok(Value::SparseMatrix(l))
}

/// `laplacian_1d(n [, dx] [, bc])` — sparse tridiagonal Laplacian.
fn builtin_laplacian_1d(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("laplacian_1d", &args, 1, 3)?;
    let n = args[0]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("laplacian_1d: n: {e}")))?;
    let (dx, bc) = match args.len() {
        1 => (1.0, rustlab_dsp::BoundaryCondition::Dirichlet),
        2 => match &args[1] {
            Value::Str(_) => (1.0, parse_bc_arg("laplacian_1d", Some(&args[1]))?),
            _ => {
                let dx = args[1]
                    .to_scalar()
                    .map_err(|e| ScriptError::type_err(format!("laplacian_1d: dx: {e}")))?;
                (dx, rustlab_dsp::BoundaryCondition::Dirichlet)
            }
        },
        3 => {
            let dx = args[1]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_1d: dx: {e}")))?;
            let bc = parse_bc_arg("laplacian_1d", Some(&args[2]))?;
            (dx, bc)
        }
        _ => unreachable!(),
    };
    let l = rustlab_dsp::laplacian::laplacian_1d(n, dx, bc)
        .map_err(|e| ScriptError::type_err(e.to_string()))?;
    Ok(Value::SparseMatrix(l))
}

/// `laplacian_3d(nx, ny, nz [, dx, dy, dz] [, bc])` — sparse 7-point Laplacian.
fn builtin_laplacian_3d(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("laplacian_3d", &args, 3, 7)?;
    let nx = args[0]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("laplacian_3d: nx: {e}")))?;
    let ny = args[1]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("laplacian_3d: ny: {e}")))?;
    let nz = args[2]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("laplacian_3d: nz: {e}")))?;
    let (dx, dy, dz, bc) = match args.len() {
        3 => (1.0, 1.0, 1.0, rustlab_dsp::BoundaryCondition::Dirichlet),
        4 => {
            let bc = parse_bc_arg("laplacian_3d", Some(&args[3]))?;
            (1.0, 1.0, 1.0, bc)
        }
        6 => {
            let dx = args[3]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_3d: dx: {e}")))?;
            let dy = args[4]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_3d: dy: {e}")))?;
            let dz = args[5]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_3d: dz: {e}")))?;
            (dx, dy, dz, rustlab_dsp::BoundaryCondition::Dirichlet)
        }
        7 => {
            let dx = args[3]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_3d: dx: {e}")))?;
            let dy = args[4]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_3d: dy: {e}")))?;
            let dz = args[5]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_3d: dz: {e}")))?;
            let bc = parse_bc_arg("laplacian_3d", Some(&args[6]))?;
            (dx, dy, dz, bc)
        }
        _ => {
            return Err(ScriptError::type_err(format!(
                "laplacian_3d: expected (nx, ny, nz), (nx, ny, nz, bc), \
                 (nx, ny, nz, dx, dy, dz), or (nx, ny, nz, dx, dy, dz, bc); \
                 got {} args",
                args.len()
            )));
        }
    };
    let l = rustlab_dsp::laplacian::laplacian_3d(nx, ny, nz, dx, dy, dz, bc)
        .map_err(|e| ScriptError::type_err(e.to_string()))?;
    Ok(Value::SparseMatrix(l))
}

/// `laplacian_eps_2d(eps_map [, dx, dy] [, bc])` — variable-coefficient
/// Laplacian `∇·(ε∇)` on a uniform grid via flux-conservative
/// discretization with harmonic-mean half-cell coefficients.
fn builtin_laplacian_eps_2d(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args_range("laplacian_eps_2d", &args, 1, 4)?;
    let eps_map = match &args[0] {
        Value::Matrix(m) => m,
        other => {
            return Err(ScriptError::type_err(format!(
                "laplacian_eps_2d: eps_map must be a matrix (ny x nx), got {}",
                other.type_name()
            )));
        }
    };
    let (dx, dy, bc) = match args.len() {
        1 => (1.0, 1.0, rustlab_dsp::BoundaryCondition::Dirichlet),
        2 => {
            let bc = parse_bc_arg("laplacian_eps_2d", Some(&args[1]))?;
            (1.0, 1.0, bc)
        }
        3 => {
            let dx = args[1]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_eps_2d: dx: {e}")))?;
            let dy = args[2]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_eps_2d: dy: {e}")))?;
            (dx, dy, rustlab_dsp::BoundaryCondition::Dirichlet)
        }
        4 => {
            let dx = args[1]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_eps_2d: dx: {e}")))?;
            let dy = args[2]
                .to_scalar()
                .map_err(|e| ScriptError::type_err(format!("laplacian_eps_2d: dy: {e}")))?;
            let bc = parse_bc_arg("laplacian_eps_2d", Some(&args[3]))?;
            (dx, dy, bc)
        }
        _ => unreachable!(),
    };
    let l = rustlab_dsp::laplacian::laplacian_eps_2d(eps_map, dx, dy, bc)
        .map_err(|e| ScriptError::type_err(e.to_string()))?;
    Ok(Value::SparseMatrix(l))
}

/// `ij2k(i, j, ny)` — convert 1-based grid indices `(i, j)` to flat
/// column-major index `k = (j-1)*ny + i`. Note: the third argument is `ny`
/// (the row count), not `nx`.
fn builtin_ij2k(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("ij2k", &args, 3)?;
    let i = args[0]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("ij2k: i: {e}")))?;
    let j = args[1]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("ij2k: j: {e}")))?;
    let ny = args[2]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("ij2k: ny: {e}")))?;
    if i < 1 || i > ny || j < 1 || ny == 0 {
        return Err(ScriptError::type_err(format!(
            "ij2k: out of range (i={i}, j={j}, ny={ny})"
        )));
    }
    let k = (j - 1) * ny + i;
    Ok(Value::Scalar(k as f64))
}

/// `ijk2k(i, j, kk, ny, nx)` — convert 1-based 3-D grid indices to flat
/// column-major-of-pages index `k = ((kk-1)*nx + (j-1))*ny + i`. The
/// fourth and fifth arguments are `ny` (rows) and `nx` (cols), in that
/// order — matching the meshgrid / Tensor3 convention.
fn builtin_ijk2k(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("ijk2k", &args, 5)?;
    let i = args[0]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("ijk2k: i: {e}")))?;
    let j = args[1]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("ijk2k: j: {e}")))?;
    let kk = args[2]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("ijk2k: kk: {e}")))?;
    let ny = args[3]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("ijk2k: ny: {e}")))?;
    let nx = args[4]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("ijk2k: nx: {e}")))?;
    if i < 1 || i > ny || j < 1 || j > nx || kk < 1 || ny == 0 || nx == 0 {
        return Err(ScriptError::type_err(format!(
            "ijk2k: out of range (i={i}, j={j}, kk={kk}, ny={ny}, nx={nx})"
        )));
    }
    let k = ((kk - 1) * nx + (j - 1)) * ny + i;
    Ok(Value::Scalar(k as f64))
}

/// `k2ijk(k, ny, nx)` — inverse of `ijk2k`. Returns a tuple `[i, j, kk]`
/// of 1-based 3-D grid indices.
fn builtin_k2ijk(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("k2ijk", &args, 3)?;
    let k = args[0]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("k2ijk: k: {e}")))?;
    let ny = args[1]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("k2ijk: ny: {e}")))?;
    let nx = args[2]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("k2ijk: nx: {e}")))?;
    if k < 1 || ny == 0 || nx == 0 {
        return Err(ScriptError::type_err(format!(
            "k2ijk: out of range (k={k}, ny={ny}, nx={nx})"
        )));
    }
    let k0 = k - 1;
    let i = (k0 % ny) + 1;
    let rest = k0 / ny;
    let j = (rest % nx) + 1;
    let kk = (rest / nx) + 1;
    Ok(Value::Tuple(vec![
        Value::Scalar(i as f64),
        Value::Scalar(j as f64),
        Value::Scalar(kk as f64),
    ]))
}

/// `k2ij(k, ny)` — inverse of `ij2k`. Returns a tuple `[i, j]` of 1-based
/// grid indices, destructurable via `[i, j] = k2ij(k, ny)`.
fn builtin_k2ij(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("k2ij", &args, 2)?;
    let k = args[0]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("k2ij: k: {e}")))?;
    let ny = args[1]
        .to_usize()
        .map_err(|e| ScriptError::type_err(format!("k2ij: ny: {e}")))?;
    if k < 1 || ny == 0 {
        return Err(ScriptError::type_err(format!(
            "k2ij: out of range (k={k}, ny={ny})"
        )));
    }
    let k0 = k - 1;
    let i = (k0 % ny) + 1;
    let j = (k0 / ny) + 1;
    Ok(Value::Tuple(vec![
        Value::Scalar(i as f64),
        Value::Scalar(j as f64),
    ]))
}

/// `sprand(m, n, density)` — random sparse matrix with approximately density*m*n non-zeros.
fn builtin_sprand(args: Vec<Value>) -> Result<Value, ScriptError> {
    check_args("sprand", &args, 3)?;
    let m = args[0].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let n = args[1].to_usize().map_err(|e| ScriptError::type_err(e))?;
    let density = args[2].to_scalar().map_err(|e| ScriptError::type_err(e))?;
    if density < 0.0 || density > 1.0 {
        return Err(ScriptError::runtime(
            "sprand: density must be in [0, 1]".to_string(),
        ));
    }
    let val_dist = Uniform::new(0.0_f64, 1.0);
    let total = m * n;
    let target_nnz = (density * total as f64).round() as usize;
    let entries: Vec<(usize, usize, C64)> = crate::eval::rng::with_rng(|rng| {
        let mut entries: Vec<(usize, usize, C64)> = Vec::with_capacity(target_nnz);
        if density >= 0.5 {
            // For high density, iterate all positions and keep with probability=density
            for r in 0..m {
                for c in 0..n {
                    if rng.gen::<f64>() < density {
                        entries.push((r, c, Complex::new(val_dist.sample(rng), 0.0)));
                    }
                }
            }
        } else {
            // For low density, sample positions directly
            use std::collections::HashSet;
            let mut positions = HashSet::new();
            while positions.len() < target_nnz && positions.len() < total {
                let r = rng.gen_range(0..m);
                let c = rng.gen_range(0..n);
                if positions.insert((r, c)) {
                    entries.push((r, c, Complex::new(val_dist.sample(rng), 0.0)));
                }
            }
        }
        entries
    });

    Ok(Value::SparseMatrix(SparseMat::new(m, n, entries)))
}
