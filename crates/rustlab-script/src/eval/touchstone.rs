//! Touchstone file reader/writer for S-parameter network data.
//!
//! Spec target: Touchstone v1.1 — `.s1p`/`.s2p`/`.s3p`/`.s4p` with parameter
//! type `S` (Y/Z/H/G deferred until conversion builtins land in Phase 2).
//!
//! Header form:
//!     # <freq-unit> <param-type> <format> R <Z0>
//! where freq-unit ∈ {Hz, kHz, MHz, GHz} (case-insensitive),
//! format ∈ {RI, MA, DB}, R defaults to 50.
//!
//! Data ordering wart: for n_ports = 2 the network parameters in each record
//! are written **column-major** (S11 S21 S12 S22). For n_ports ≥ 3 they are
//! written **row-major** (S11 S12 S13 … S21 S22 S23 … S31 …). The reader
//! handles both per the spec; the writer follows the same convention.

use ndarray::Array3;
use num_complex::Complex;
use rustlab_core::C64;
use std::path::Path;

/// Parsed Touchstone network data, all units normalised (freqs in Hz, S as
/// complex). `parameters` shape is `[n_freqs, n_ports, n_ports]` so
/// `parameters[[k, i, j]]` is S_{i+1, j+1} at the k-th frequency.
#[derive(Debug, Clone)]
pub struct TouchstoneData {
    pub n_ports: usize,
    pub frequencies: Vec<f64>,
    pub parameters: Array3<C64>,
    pub z0: f64,
    /// Optional 2-port noise-parameter block. Present only when the source
    /// file carried noise data after the S-parameter rows. None for n_ports
    /// != 2 or when the file has no noise block.
    pub noise: Option<NoiseData>,
}

/// Per-frequency noise parameters for a 2-port. Frequencies may differ from
/// (and need not be a subset of) the S-parameter frequency grid — VNAs
/// commonly export far fewer noise samples than gain samples.
#[derive(Debug, Clone)]
pub struct NoiseData {
    pub frequencies: Vec<f64>,
    /// Minimum noise figure NFmin in dB.
    pub nf_min_db: Vec<f64>,
    /// Optimum source reflection coefficient Γopt (complex).
    pub gamma_opt: Vec<C64>,
    /// Equivalent normalised noise resistance Rn/Z0.
    pub rn_normalised: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FreqUnit {
    Hz,
    Khz,
    Mhz,
    Ghz,
}

impl FreqUnit {
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "hz" => Ok(FreqUnit::Hz),
            "khz" => Ok(FreqUnit::Khz),
            "mhz" => Ok(FreqUnit::Mhz),
            "ghz" => Ok(FreqUnit::Ghz),
            other => Err(format!("unknown frequency unit '{other}'")),
        }
    }

    fn scale(self) -> f64 {
        match self {
            FreqUnit::Hz => 1.0,
            FreqUnit::Khz => 1e3,
            FreqUnit::Mhz => 1e6,
            FreqUnit::Ghz => 1e9,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Format {
    Ri,
    Ma,
    Db,
}

impl Format {
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_uppercase().as_str() {
            "RI" => Ok(Format::Ri),
            "MA" => Ok(Format::Ma),
            "DB" => Ok(Format::Db),
            other => Err(format!(
                "unknown data format '{other}' (expected RI, MA, or DB)"
            )),
        }
    }

    fn pair_to_complex(self, a: f64, b: f64) -> C64 {
        match self {
            Format::Ri => Complex::new(a, b),
            Format::Ma => Complex::from_polar(a, b.to_radians()),
            Format::Db => {
                let mag = 10f64.powf(a / 20.0);
                Complex::from_polar(mag, b.to_radians())
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Header {
    freq_unit: FreqUnit,
    param_type: char, // 'S' only for Phase 1
    format: Format,
    z0: f64,
}

impl Default for Header {
    fn default() -> Self {
        Header {
            freq_unit: FreqUnit::Ghz, // Touchstone default per spec
            param_type: 'S',
            format: Format::Ma,
            z0: 50.0,
        }
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Read a Touchstone file. Port count is inferred from the file extension
/// (`.sNp`), so the path must keep the standard suffix.
pub fn read_touchstone(path: &Path) -> Result<TouchstoneData, String> {
    let n_ports = n_ports_from_extension(path)?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("touchstone read: {e}"))?;
    read_touchstone_str(&text, n_ports)
}

/// Parse Touchstone text given an explicit port count. Used by both file
/// loading and unit tests.
///
/// Recognises v1.1 layout plus v2 `[Version]`/`[Reference]`/`[Network Data]`
/// keyword lines in tolerance mode (they're consumed and ignored when their
/// presence doesn't change the v1-compatible interpretation). For 2-port
/// files, also parses the optional noise-parameter block that follows the
/// S-parameter rows.
pub fn read_touchstone_str(text: &str, n_ports: usize) -> Result<TouchstoneData, String> {
    if n_ports == 0 {
        return Err("touchstone: n_ports must be >= 1".to_string());
    }
    let mut header = Header::default();
    let mut saw_header = false;
    let mut tokens: Vec<f64> = Vec::new();
    // Optional explicit Z0 from `[Reference]` (scalar form only). When
    // present and non-conflicting, overrides the header default.
    let mut v2_reference_z0: Option<f64> = None;

    for raw_line in text.lines() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            header = parse_header(rest.trim())?;
            saw_header = true;
            continue;
        }
        // Touchstone v2 bracketed keyword lines. Accept the ones whose
        // presence doesn't change the v1-compatible parse and reject the
        // ones that would (per-port [Reference] vector, [Mixed-Mode-Order]
        // tables).
        if line.starts_with('[') {
            handle_v2_keyword(line, &mut v2_reference_z0)?;
            continue;
        }
        for tok in line.split_ascii_whitespace() {
            let v: f64 = tok
                .parse()
                .map_err(|_| format!("touchstone: cannot parse number '{tok}'"))?;
            tokens.push(v);
        }
    }

    if !saw_header {
        return Err("touchstone: missing '# ...' header line".to_string());
    }
    if header.param_type != 'S' {
        return Err(format!(
            "touchstone: parameter type '{}' not supported in Phase 1 (S only)",
            header.param_type
        ));
    }
    if let Some(z) = v2_reference_z0 {
        // v2 `[Reference]` is authoritative when present; override the
        // header default (which defaulted to 50 if the `R` token was missing).
        header.z0 = z;
    }

    let freq_scale = header.freq_unit.scale();
    let s_record_size = 1 + 2 * n_ports * n_ports;

    // Read S-parameter records until we hit the noise block (only valid for
    // 2-port, signaled by a strictly-decreasing frequency transition) or
    // exhaust the token stream.
    let mut frequencies: Vec<f64> = Vec::new();
    let mut s_slabs: Vec<Vec<C64>> = Vec::new(); // each entry is n_ports*n_ports complex values
    let mut cursor = 0usize;
    while cursor + s_record_size <= tokens.len() {
        let freq_token = tokens[cursor] * freq_scale;
        if !frequencies.is_empty() {
            let prev = *frequencies.last().unwrap();
            if freq_token <= prev {
                // Either noise block (2-port) or a sort-order violation.
                if n_ports == 2 {
                    break;
                }
                return Err(format!(
                    "touchstone: frequencies must be strictly increasing (saw {freq_token} <= {prev} at adjacent S-block samples)"
                ));
            }
        }
        frequencies.push(freq_token);
        let mut slab = Vec::with_capacity(n_ports * n_ports);
        for pair_idx in 0..(n_ports * n_ports) {
            let a = tokens[cursor + 1 + 2 * pair_idx];
            let b = tokens[cursor + 1 + 2 * pair_idx + 1];
            slab.push(header.format.pair_to_complex(a, b));
        }
        s_slabs.push(slab);
        cursor += s_record_size;
    }
    if frequencies.is_empty() {
        if tokens.is_empty() {
            return Err("touchstone: no data points".to_string());
        }
        return Err(format!(
            "touchstone: data has {} tokens but at least {} are required for one {}-port S record",
            tokens.len(),
            s_record_size,
            n_ports
        ));
    }

    let n_freqs = frequencies.len();
    let mut parameters: Array3<C64> = Array3::zeros((n_freqs, n_ports, n_ports));
    for (k, slab) in s_slabs.iter().enumerate() {
        for (pair_idx, z) in slab.iter().enumerate() {
            let (i, j) = storage_to_ij(pair_idx, n_ports);
            parameters[[k, i, j]] = *z;
        }
    }

    // Remaining tokens, if any, are noise data (2-port only).
    let noise = if cursor < tokens.len() {
        if n_ports != 2 {
            return Err(format!(
                "touchstone: {} extra tokens after the S-block but n_ports={n_ports} — noise block only supported for 2-port",
                tokens.len() - cursor
            ));
        }
        let leftover = &tokens[cursor..];
        if leftover.len() % 5 != 0 {
            return Err(format!(
                "touchstone: leftover token count {} is not a multiple of 5 — either the noise block is malformed, or the S-block frequencies are not strictly increasing (saw a decreasing transition and tried to switch to noise mode)",
                leftover.len()
            ));
        }
        let n_noise = leftover.len() / 5;
        let mut noise_freqs = Vec::with_capacity(n_noise);
        let mut nf_min_db = Vec::with_capacity(n_noise);
        let mut gamma_opt = Vec::with_capacity(n_noise);
        let mut rn = Vec::with_capacity(n_noise);
        for k in 0..n_noise {
            let base = k * 5;
            let f = leftover[base] * freq_scale;
            if let Some(prev) = noise_freqs.last() {
                if !(f > *prev) {
                    return Err(format!(
                        "touchstone: noise frequencies must be strictly increasing (saw {f} <= {prev})"
                    ));
                }
            }
            noise_freqs.push(f);
            nf_min_db.push(leftover[base + 1]);
            let mag = leftover[base + 2];
            let ang_deg = leftover[base + 3];
            gamma_opt.push(C64::from_polar(mag, ang_deg.to_radians()));
            rn.push(leftover[base + 4]);
        }
        Some(NoiseData {
            frequencies: noise_freqs,
            nf_min_db,
            gamma_opt,
            rn_normalised: rn,
        })
    } else {
        None
    };

    Ok(TouchstoneData {
        n_ports,
        frequencies,
        parameters,
        z0: header.z0,
        noise,
    })
}

/// Tolerantly handle a v2 bracketed keyword line. Accept the keywords that
/// don't change the v1-compatible parse; reject the ones that genuinely
/// require new code (per-port `[Reference]` lists, `[Mixed-Mode-Order]`
/// tables). Side-effect: writes the parsed `[Reference]` scalar into
/// `out_z0` when the line is a single-value reference.
fn handle_v2_keyword(line: &str, out_z0: &mut Option<f64>) -> Result<(), String> {
    let trimmed = line.trim_start_matches('[');
    let end = trimmed
        .find(']')
        .ok_or_else(|| format!("touchstone: malformed bracketed keyword '{line}'"))?;
    let keyword = trimmed[..end].trim();
    let rest = trimmed[end + 1..].trim();
    match keyword.to_ascii_lowercase().as_str() {
        "version" => {
            // Accept any 2.x; just sanity-check it parses as a number.
            if !rest.is_empty()
                && !rest
                    .split_ascii_whitespace()
                    .next()
                    .map(|t| t.parse::<f64>().is_ok())
                    .unwrap_or(false)
            {
                return Err(format!(
                    "touchstone: [Version] value '{rest}' is not a number"
                ));
            }
            Ok(())
        }
        "number of ports" | "number of frequencies" | "number of noise frequencies" => {
            // Informational only — we discover the same numbers from the file
            // contents. Skip without validation.
            Ok(())
        }
        "two-port data order" | "two-port order" => {
            // Tolerate either ordering keyword; the v1-style column-major
            // assumption already matches `12_21` (the spec default).
            Ok(())
        }
        "network data" | "noise data" | "end" | "begin information" | "end information" => {
            Ok(())
        }
        "matrix format" => Ok(()), // "full" matches what we already do
        "reference" => {
            // Accept the single-value form: `[Reference] 50`. Per-port lists
            // (e.g. `[Reference] 50 75 50 50`) require per-port Z0 storage
            // which Phase 6 explicitly doesn't add — error clearly.
            let vals: Vec<f64> = rest
                .split_ascii_whitespace()
                .filter_map(|t| t.parse::<f64>().ok())
                .collect();
            match vals.len() {
                0 => Ok(()),
                1 => {
                    if !(vals[0] > 0.0) {
                        return Err(format!(
                            "touchstone: [Reference] {} must be positive",
                            vals[0]
                        ));
                    }
                    *out_z0 = Some(vals[0]);
                    Ok(())
                }
                _ => Err(format!(
                    "touchstone: per-port [Reference] list not supported (got {} values); use a single scalar",
                    vals.len()
                )),
            }
        }
        "mixed-mode-order" => Err(
            "touchstone: [Mixed-Mode-Order] tables are not supported — use a single-ended file"
                .to_string(),
        ),
        other => Err(format!(
            "touchstone: unrecognised keyword '[{other}]' (open an issue if this is a v2 file you need)"
        )),
    }
}

/// Write a Touchstone file using the lossless RI format and Hz frequency
/// unit (the unambiguous choice for round-tripping).
pub fn write_touchstone(path: &Path, data: &TouchstoneData) -> Result<(), String> {
    let text = render_touchstone(data);
    std::fs::write(path, text).map_err(|e| format!("touchstone write: {e}"))
}

fn render_touchstone(data: &TouchstoneData) -> String {
    let mut out = String::new();
    out.push_str(&format!("! Written by rustlab\n"));
    out.push_str(&format!("# Hz S RI R {}\n", data.z0));

    let n = data.n_ports;
    for k in 0..data.frequencies.len() {
        // Each frequency record starts on a fresh line. Use 15 sig-figs so
        // round-tripping a Touchstone file is effectively lossless against
        // f64 (16-17 sig-figs of mantissa).
        out.push_str(&format!("{:.15e}", data.frequencies[k]));
        for pair_idx in 0..(n * n) {
            let (i, j) = storage_to_ij(pair_idx, n);
            let z = data.parameters[[k, i, j]];
            out.push_str(&format!(" {:.15e} {:.15e}", z.re, z.im));
            // For n >= 3, break the line after every row of S to match the
            // conventional Touchstone layout. (Reader doesn't care, but human
            // readers and many third-party tools expect this.)
            if n >= 3 && (pair_idx + 1) % n == 0 && pair_idx + 1 < n * n {
                out.push_str("\n            ");
            }
        }
        out.push('\n');
    }
    out
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn n_ports_from_extension(path: &Path) -> Result<usize, String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .ok_or_else(|| {
            format!(
                "touchstone: cannot infer port count — path '{}' has no .sNp extension",
                path.display()
            )
        })?;
    // Accept any case of .sNp where N is a positive integer (commonly 1..16).
    if let Some(rest) = ext.strip_prefix('s') {
        if let Some(num) = rest.strip_suffix('p') {
            if let Ok(n) = num.parse::<usize>() {
                if n >= 1 {
                    return Ok(n);
                }
            }
        }
    }
    Err(format!(
        "touchstone: extension '.{ext}' is not of the form .sNp"
    ))
}

fn strip_comment(line: &str) -> &str {
    match line.find('!') {
        Some(i) => &line[..i],
        None => line,
    }
}

fn parse_header(s: &str) -> Result<Header, String> {
    // `<unit> <type> <format> R <z0>`  — any subset may be omitted; missing
    // fields take their default value.
    let mut hdr = Header::default();
    let mut toks = s.split_ascii_whitespace().peekable();

    if let Some(t) = toks.peek() {
        if FreqUnit::from_str(t).is_ok() {
            hdr.freq_unit = FreqUnit::from_str(toks.next().unwrap()).unwrap();
        }
    }
    if let Some(&t) = toks.peek() {
        let up = t.to_ascii_uppercase();
        if up.len() == 1 && "SYZHG".contains(up.chars().next().unwrap()) {
            hdr.param_type = up.chars().next().unwrap();
            toks.next();
        }
    }
    if let Some(t) = toks.peek() {
        if Format::from_str(t).is_ok() {
            hdr.format = Format::from_str(toks.next().unwrap()).unwrap();
        }
    }
    // Optional `R <z0>`
    if let Some(&t) = toks.peek() {
        if t.eq_ignore_ascii_case("r") {
            toks.next();
            let z = toks.next().ok_or_else(|| {
                "touchstone header: 'R' specifier missing reference impedance value".to_string()
            })?;
            hdr.z0 = z
                .parse::<f64>()
                .map_err(|_| format!("touchstone header: bad Z0 value '{z}'"))?;
            if !(hdr.z0 > 0.0) {
                return Err(format!(
                    "touchstone header: reference impedance must be positive (got {})",
                    hdr.z0
                ));
            }
        }
    }
    Ok(hdr)
}

/// Translate a storage-order pair index (0-based) into the (i, j) cell of the
/// S matrix the spec assigns it to.
///
/// For 2-port: column-major (S11, S21, S12, S22). Storage indices 0..4
/// map to (0,0), (1,0), (0,1), (1,1).
///
/// For n != 2: row-major. Storage index k maps to (k / n, k % n).
fn storage_to_ij(pair_idx: usize, n_ports: usize) -> (usize, usize) {
    if n_ports == 2 {
        // Column-major: i = idx % 2, j = idx / 2
        (pair_idx % 2, pair_idx / 2)
    } else {
        (pair_idx / n_ports, pair_idx % n_ports)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array3;

    fn arr_eq(a: &Array3<C64>, b: &Array3<C64>, tol: f64) {
        assert_eq!(a.shape(), b.shape(), "shape mismatch");
        for (x, y) in a.iter().zip(b.iter()) {
            assert!(
                (x.re - y.re).abs() < tol && (x.im - y.im).abs() < tol,
                "values differ: {x} vs {y}"
            );
        }
    }

    fn synth_data(n_ports: usize, n_freqs: usize) -> TouchstoneData {
        let mut params: Array3<C64> = Array3::zeros((n_freqs, n_ports, n_ports));
        let mut freqs = Vec::with_capacity(n_freqs);
        for k in 0..n_freqs {
            freqs.push(1.0e9 + k as f64 * 1.0e8); // 1, 1.1, 1.2, ... GHz
            for i in 0..n_ports {
                for j in 0..n_ports {
                    // Distinct value per (k, i, j) to catch index swaps.
                    let re = (k * 100 + i * 10 + j) as f64 * 0.01;
                    let im = -((k * 100 + i * 10 + j) as f64 * 0.005);
                    params[[k, i, j]] = Complex::new(re, im);
                }
            }
        }
        TouchstoneData {
            n_ports,
            frequencies: freqs,
            parameters: params,
            z0: 50.0,
            noise: None,
        }
    }

    #[test]
    fn round_trip_ri_2port() {
        let d = synth_data(2, 5);
        let text = render_touchstone(&d);
        let r = read_touchstone_str(&text, 2).expect("parse");
        assert_eq!(r.n_ports, 2);
        assert_eq!(r.frequencies, d.frequencies);
        assert_eq!(r.z0, 50.0);
        arr_eq(&r.parameters, &d.parameters, 1e-12);
    }

    #[test]
    fn round_trip_ri_1port() {
        let d = synth_data(1, 3);
        let text = render_touchstone(&d);
        let r = read_touchstone_str(&text, 1).unwrap();
        arr_eq(&r.parameters, &d.parameters, 1e-12);
    }

    #[test]
    fn round_trip_ri_3port() {
        let d = synth_data(3, 4);
        let text = render_touchstone(&d);
        let r = read_touchstone_str(&text, 3).unwrap();
        arr_eq(&r.parameters, &d.parameters, 1e-12);
    }

    #[test]
    fn round_trip_ri_4port() {
        let d = synth_data(4, 2);
        let text = render_touchstone(&d);
        let r = read_touchstone_str(&text, 4).unwrap();
        arr_eq(&r.parameters, &d.parameters, 1e-12);
    }

    #[test]
    fn parse_ma_format_2port() {
        // Construct a known S matrix and write a MA-format Touchstone string
        // by hand, then ensure the parser recovers it. Use a 2-port at one
        // frequency to keep the bytes auditable.
        //
        // S11 = 0.5 ∠ 30°,  S21 = 1.0 ∠ -45°,
        // S12 = 0.1 ∠ 90°,  S22 = 0.7 ∠ 180°.
        //
        // Column-major (S11, S21, S12, S22).
        let text = "\
# GHz S MA R 50
1.0   0.5 30   1.0 -45   0.1 90   0.7 180
";
        let r = read_touchstone_str(text, 2).unwrap();
        assert_eq!(r.frequencies, vec![1e9]);
        let s11 = r.parameters[[0, 0, 0]];
        let s21 = r.parameters[[0, 1, 0]];
        let s12 = r.parameters[[0, 0, 1]];
        let s22 = r.parameters[[0, 1, 1]];
        let exp_s11 = Complex::from_polar(0.5, 30f64.to_radians());
        let exp_s21 = Complex::from_polar(1.0, (-45f64).to_radians());
        let exp_s12 = Complex::from_polar(0.1, 90f64.to_radians());
        let exp_s22 = Complex::from_polar(0.7, 180f64.to_radians());
        for (a, b) in [
            (s11, exp_s11),
            (s21, exp_s21),
            (s12, exp_s12),
            (s22, exp_s22),
        ] {
            assert!((a - b).norm() < 1e-12, "{a} vs {b}");
        }
    }

    #[test]
    fn parse_db_format_1port() {
        // S11 = -20 dB ∠ 0°  →  |S11| = 0.1
        let text = "\
! Sample DB-format 1-port
# MHz S DB R 75
100  -20.0 0.0
";
        let r = read_touchstone_str(text, 1).unwrap();
        assert_eq!(r.z0, 75.0);
        assert!((r.frequencies[0] - 100e6).abs() < 1e-3);
        let s = r.parameters[[0, 0, 0]];
        assert!((s.re - 0.1).abs() < 1e-12);
        assert!(s.im.abs() < 1e-12);
    }

    #[test]
    fn comments_and_blank_lines_ignored() {
        let text = "\
! this is a comment
# Hz S RI R 50

! header is above; data follows
1.0e9   0.1 0.0   0.0 0.0   0.0 0.0   0.2 0.0
! trailing comment
";
        let r = read_touchstone_str(text, 2).unwrap();
        assert_eq!(r.frequencies, vec![1e9]);
    }

    #[test]
    fn three_port_record_can_span_multiple_lines() {
        // 3-port → 1 + 18 = 19 numbers per record; split arbitrarily across
        // lines.
        let text = "\
# Hz S RI R 50
1.0e9
   1.0 0.0   2.0 0.0   3.0 0.0
   4.0 0.0   5.0 0.0   6.0 0.0
   7.0 0.0   8.0 0.0   9.0 0.0
";
        let r = read_touchstone_str(text, 3).unwrap();
        // Row-major: S11=1, S12=2, S13=3, S21=4, ...
        assert_eq!(r.parameters[[0, 0, 0]].re, 1.0);
        assert_eq!(r.parameters[[0, 0, 1]].re, 2.0);
        assert_eq!(r.parameters[[0, 0, 2]].re, 3.0);
        assert_eq!(r.parameters[[0, 1, 0]].re, 4.0);
        assert_eq!(r.parameters[[0, 2, 2]].re, 9.0);
    }

    #[test]
    fn missing_header_errors() {
        let text = "1.0e9   0.1 0.0   0.0 0.0   0.0 0.0   0.2 0.0\n";
        let err = read_touchstone_str(text, 2).unwrap_err();
        assert!(err.contains("header"));
    }

    #[test]
    fn mismatched_data_count_errors() {
        // 2-port wants 9 numbers per record; provide 8.
        let text = "# Hz S RI R 50\n1e9 1 0 0 0 0 0 0 0\n";
        let r = read_touchstone_str(text, 2).unwrap();
        assert_eq!(r.frequencies.len(), 1);
        let bad = "# Hz S RI R 50\n1e9 1 0 0 0 0 0 0\n";
        let err = read_touchstone_str(bad, 2).unwrap_err();
        assert!(err.contains("required for one"), "msg: {err}");
    }

    #[test]
    fn non_monotonic_freqs_error() {
        // For a 2-port with no plausible noise interpretation, the decreasing
        // transition produces a leftover-not-multiple-of-5 error that
        // explicitly calls out the sort-order issue. For n != 2, the error is
        // more direct.
        let text = "\
# Hz S RI R 50
2.0e9   0 0   0 0   0 0   0 0
1.0e9   0 0   0 0   0 0   0 0
";
        let err = read_touchstone_str(text, 2).unwrap_err();
        assert!(
            err.contains("strictly increasing") || err.contains("not a multiple of 5"),
            "msg: {err}"
        );
    }

    #[test]
    fn non_monotonic_freqs_3port_errors_directly() {
        // For non-2-port, no noise-block ambiguity: emit the sort error directly.
        let text = "\
# Hz S RI R 50
2.0e9   0 0  0 0  0 0  0 0  0 0  0 0  0 0  0 0  0 0
1.0e9   0 0  0 0  0 0  0 0  0 0  0 0  0 0  0 0  0 0
";
        let err = read_touchstone_str(text, 3).unwrap_err();
        assert!(err.contains("strictly increasing"), "msg: {err}");
    }

    #[test]
    fn header_defaults_when_fields_omitted() {
        // `# RI` alone: unit defaults to GHz, type S, R 50.
        let text = "\
# RI
1.0  0.1 0.0  0.2 0.0  0.3 0.0  0.4 0.0
";
        let r = read_touchstone_str(text, 2).unwrap();
        assert_eq!(r.frequencies, vec![1e9]); // 1 GHz default
        assert_eq!(r.z0, 50.0);
    }

    #[test]
    fn v2_compatible_keyword_lines_accepted() {
        // [Version] 2.0 plus [Reference] scalar should now parse cleanly.
        let text = "\
[Version] 2.0
[Number of Ports] 2
[Reference] 75
# GHz S MA R 50
1.0  0.5 30   1.0 -45   0.05 90   0.4 170
[End]
";
        let data = read_touchstone_str(text, 2).unwrap();
        // [Reference] 75 overrides the # R 50 header default.
        assert_eq!(data.z0, 75.0);
        assert_eq!(data.frequencies, vec![1e9]);
    }

    #[test]
    fn v2_mixed_mode_order_still_rejected() {
        let text = "\
[Version] 2.0
[Mixed-Mode-Order] D1 D2 C1 C2
# Hz S RI R 50
1.0e9 0 0 0 0 0 0 0 0
";
        let err = read_touchstone_str(text, 2).unwrap_err();
        assert!(err.contains("Mixed-Mode-Order"), "msg: {err}");
    }

    #[test]
    fn v2_per_port_reference_list_rejected_clearly() {
        let text = "\
[Version] 2.0
[Reference] 50 75 50 50
# Hz S RI R 50
1.0e9 0 0 0 0 0 0 0 0
";
        let err = read_touchstone_str(text, 2).unwrap_err();
        assert!(err.contains("per-port"), "msg: {err}");
    }

    #[test]
    fn noise_block_parses_when_present_in_s2p() {
        // 2-port S block (2 freqs), then 2-row noise block: each noise row
        // is `freq  Fmin_dB  |Γopt|  ∠Γopt°  Rn`.
        let text = "\
! Synthetic 2-port with noise data
# GHz S MA R 50
1.0   0.1 0    0.9 -45   0.05 90   0.2 180
2.0   0.1 0    0.9 -55   0.05 80   0.2 175
! noise block follows
1.0   1.5   0.4  20    0.10
2.0   2.0   0.3  25    0.12
";
        let data = read_touchstone_str(text, 2).unwrap();
        let noise = data.noise.expect("noise block missing");
        assert_eq!(noise.frequencies, vec![1e9, 2e9]);
        assert!((noise.nf_min_db[0] - 1.5).abs() < 1e-12);
        assert!((noise.nf_min_db[1] - 2.0).abs() < 1e-12);
        // |Γopt| = 0.4, ∠ = 20° → check via magnitude and angle.
        assert!((noise.gamma_opt[0].norm() - 0.4).abs() < 1e-12);
        assert!((noise.gamma_opt[0].arg().to_degrees() - 20.0).abs() < 1e-9);
        assert!((noise.rn_normalised[0] - 0.10).abs() < 1e-12);
        assert!((noise.rn_normalised[1] - 0.12).abs() < 1e-12);
    }

    #[test]
    fn noise_block_with_subset_frequency_grid() {
        // Noise frequencies need not match the S grid; the parser must
        // accept any monotonic subset.
        let text = "\
# GHz S MA R 50
1.0   0.1 0    0.9 -45   0.05 90   0.2 180
2.0   0.1 0    0.9 -55   0.05 80   0.2 175
3.0   0.1 0    0.9 -65   0.05 70   0.2 170
1.5   1.8   0.35 22   0.11
";
        let data = read_touchstone_str(text, 2).unwrap();
        let noise = data.noise.unwrap();
        assert_eq!(noise.frequencies.len(), 1);
        assert!((noise.frequencies[0] - 1.5e9).abs() < 1e-3);
    }

    #[test]
    fn noise_block_for_non_2port_rejects() {
        // 3-port with extra tokens after the S block. Spec doesn't define
        // noise blocks outside 2-port, so reject loudly.
        let text = "\
# Hz S RI R 50
1.0e9   1 0  2 0  3 0  4 0  5 0  6 0  7 0  8 0  9 0
2.0e9   1 0  2 0  3 0  4 0  5 0  6 0  7 0  8 0  9 0
1.5e9   1.0  0.4  20   0.10
";
        let err = read_touchstone_str(text, 3).unwrap_err();
        assert!(err.contains("noise block"), "msg: {err}");
    }

    #[test]
    fn extension_inference() {
        use std::path::PathBuf;
        assert_eq!(n_ports_from_extension(&PathBuf::from("x.s1p")).unwrap(), 1);
        assert_eq!(n_ports_from_extension(&PathBuf::from("x.S2P")).unwrap(), 2);
        assert_eq!(n_ports_from_extension(&PathBuf::from("x.s4p")).unwrap(), 4);
        assert!(n_ports_from_extension(&PathBuf::from("x.txt")).is_err());
    }

    #[test]
    fn round_trip_via_disk_2port() {
        let dir = std::env::temp_dir();
        let p = dir.join("rustlab_touchstone_roundtrip.s2p");
        let d = synth_data(2, 7);
        write_touchstone(&p, &d).unwrap();
        let r = read_touchstone(&p).unwrap();
        arr_eq(&r.parameters, &d.parameters, 1e-12);
        let _ = std::fs::remove_file(&p);
    }
}
