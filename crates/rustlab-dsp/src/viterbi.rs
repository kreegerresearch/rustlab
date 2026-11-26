//! Convolutional-code trellises and the Viterbi decoder behind `poly2trellis`
//! and `vitdec` (MATLAB conventions).
//!
//! **Trellis.** A rate-k/n encoder with constraint lengths `K[i]` has one
//! shift register of `K[i] - 1` bits per input. The state packs the
//! registers with input 1's register in the lowest bits and the last input's
//! in the highest; inside a register the newest bit is the most significant.
//! Input symbol bit 1 (most significant) is input 1, and output symbol bit 1
//! is output 1. A generator `g[i][j]` (octal in MATLAB) is read as `K[i]`
//! bits whose most significant bit taps the current input. A feedback
//! polynomial `f[i]` makes input `i` recursive: the bit shifted in is
//! `u ⊕ parity(f & register)`, and the generators see that bit in place of
//! `u`. These are the tables of MATLAB's and Octave's `poly2trellis`.
//!
//! **Decoder.** The path metric is a score to maximize: the correlation
//! `Σ c_j y_j` with `c = +1` for a code bit 0 (`unquant`), minus the
//! Hamming distance (`hard`), or minus the level cost (`soft`: level `v`
//! costs `v` against a 0 and `2^nsdec - 1 - v` against a 1). Unreachable
//! states start at `-inf`. A state keeps the first predecessor (in
//! increasing state order) with the strictly highest score, so a tie goes to
//! the lower state. Each step's decisions are kept for `tblen` steps; the
//! symbol of step `t - tblen` is read by tracing back from the best state
//! after step `t` (highest metric, lowest state on a tie). `trunc` and
//! `term` trace the last `tblen` steps back from the best state and from
//! state 0. `cont` emits nothing for the last steps and starts with `tblen`
//! zero symbols. With `tblen` at least the number of steps, `term` is one
//! traceback of the whole frame from state 0.

use crate::error::DspError;

fn bad(msg: impl Into<String>) -> DspError {
    DspError::InvalidParameter(msg.into())
}

/// A trellis in MATLAB's form, with outputs as plain integers.
#[derive(Clone, Debug, PartialEq)]
pub struct Trellis {
    /// Input bits per step (`numInputSymbols = 2^k`).
    pub k: usize,
    /// Output bits per step (`numOutputSymbols = 2^n`).
    pub n: usize,
    pub num_states: usize,
    /// `next[s * 2^k + sym]`.
    pub next: Vec<usize>,
    /// `out[s * 2^k + sym]`, the output symbol as an integer (not octal).
    pub out: Vec<usize>,
}

/// The value of an octal number written with decimal digits (`171` → 121).
pub fn octal_value(x: f64) -> Result<u64, DspError> {
    if !(x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x < 1e18) {
        return Err(bad(format!("{x} is not an octal number")));
    }
    let (mut d, mut v, mut p) = (x as u64, 0u64, 1u64);
    while d > 0 {
        let digit = d % 10;
        if digit > 7 {
            return Err(bad(format!("{x} is not an octal number (digit {digit})")));
        }
        v += digit * p;
        p *= 8;
        d /= 10;
    }
    Ok(v)
}

/// `n` written in octal digits, as a number (`121` → 171).
pub fn octal_digits(n: u64) -> f64 {
    format!("{n:o}").parse().expect("octal digits")
}

const MAX_MEMORY_BITS: usize = 24;

/// MATLAB `poly2trellis(K, codegen, feedback)`. `codegen[i][j]` and
/// `feedback[i]` are already octal-decoded.
pub fn poly2trellis(
    constraint: &[usize],
    codegen: &[Vec<u64>],
    feedback: Option<&[u64]>,
) -> Result<Trellis, DspError> {
    let k = constraint.len();
    if k == 0 || codegen.len() != k {
        return Err(bad(format!(
            "codegen must have one row per constraint length ({k}), got {}",
            codegen.len()
        )));
    }
    let n = codegen[0].len();
    if n == 0 || codegen.iter().any(|r| r.len() != n) {
        return Err(bad("codegen rows must have the same number of outputs"));
    }
    if k > 16 || n > 16 {
        return Err(bad("at most 16 inputs and 16 outputs"));
    }
    if let Some(f) = feedback {
        if f.len() != k {
            return Err(bad(format!(
                "feedback must have {k} entries, got {}",
                f.len()
            )));
        }
    }
    let mut offset = Vec::with_capacity(k);
    let mut memory = 0usize;
    for (i, &ki) in constraint.iter().enumerate() {
        if ki == 0 {
            return Err(bad("constraint lengths must be positive integers"));
        }
        let limit = 1u64 << ki;
        for (j, &g) in codegen[i].iter().enumerate() {
            if g >= limit {
                return Err(bad(format!(
                    "generator {} (input {}, output {}) needs more than constraint length {ki}",
                    octal_digits(g),
                    i + 1,
                    j + 1
                )));
            }
        }
        if let Some(f) = feedback {
            if f[i] >= limit {
                return Err(bad(format!(
                    "feedback {} needs more than constraint length {ki}",
                    octal_digits(f[i])
                )));
            }
        }
        offset.push(memory);
        memory += ki - 1;
    }
    if memory > MAX_MEMORY_BITS {
        return Err(bad(format!(
            "{memory} memory bits is more than {MAX_MEMORY_BITS} (2^{MAX_MEMORY_BITS} states)"
        )));
    }
    let num_states = 1usize << memory;
    let nsym = 1usize << k;
    let mut next = vec![0; num_states * nsym];
    let mut out = vec![0; num_states * nsym];
    for s in 0..num_states {
        for sym in 0..nsym {
            let mut ns = 0usize;
            let mut o = 0usize;
            let mut full = vec![0u64; k];
            for i in 0..k {
                let m = constraint[i] - 1;
                let reg = ((s >> offset[i]) & ((1usize << m) - 1)) as u64;
                let mut u = ((sym >> (k - 1 - i)) & 1) as u64;
                if let Some(f) = feedback {
                    u ^= u64::from((f[i] & reg).count_ones() & 1);
                }
                full[i] = (u << m) | reg;
                let nreg = if m == 0 { 0 } else { (full[i] >> 1) as usize };
                ns |= nreg << offset[i];
            }
            for j in 0..n {
                let bit = (0..k).fold(0u32, |acc, i| {
                    acc ^ ((codegen[i][j] & full[i]).count_ones() & 1)
                });
                o |= (bit as usize) << (n - 1 - j);
            }
            next[s * nsym + sym] = ns;
            out[s * nsym + sym] = o;
        }
    }
    Ok(Trellis {
        k,
        n,
        num_states,
        next,
        out,
    })
}

impl Trellis {
    /// Checks the tables of a trellis given as a struct.
    pub fn validate(&self) -> Result<(), DspError> {
        let nsym = 1usize << self.k;
        if self.num_states == 0 || !self.num_states.is_power_of_two() {
            return Err(bad("numStates must be a power of 2"));
        }
        if self.next.len() != self.num_states * nsym || self.out.len() != self.next.len() {
            return Err(bad(
                "nextStates and outputs must be numStates × numInputSymbols",
            ));
        }
        if self.next.iter().any(|&s| s >= self.num_states) {
            return Err(bad("nextStates entries must be states 0 to numStates-1"));
        }
        if self.out.iter().any(|&o| o >= 1usize << self.n) {
            return Err(bad(
                "outputs entries must be symbols 0 to numOutputSymbols-1",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpMode {
    Trunc,
    Term,
    Cont,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecType {
    Hard,
    Soft { nsdec: u32 },
    Unquant,
}

/// Predecessors of every state, in increasing (state, symbol) order.
struct Preds {
    start: Vec<usize>,
    prev: Vec<usize>,
    sym: Vec<usize>,
    out: Vec<usize>,
    /// Every state has exactly two predecessors.
    two: bool,
}

impl Preds {
    fn of(t: &Trellis) -> Result<Preds, DspError> {
        let nsym = 1usize << t.k;
        let mut count = vec![0usize; t.num_states];
        for &ns in &t.next {
            count[ns] += 1;
        }
        if count.contains(&0) {
            return Err(bad("every state needs a predecessor"));
        }
        if count.iter().any(|&c| c > 256) {
            return Err(bad("a state has more than 256 predecessors"));
        }
        let mut start = vec![0usize; t.num_states + 1];
        for s in 0..t.num_states {
            start[s + 1] = start[s] + count[s];
        }
        let total = t.next.len();
        let (mut prev, mut sym, mut out) = (vec![0; total], vec![0; total], vec![0; total]);
        let mut fill = start.clone();
        for s in 0..t.num_states {
            for x in 0..nsym {
                let ns = t.next[s * nsym + x];
                let p = fill[ns];
                prev[p] = s;
                sym[p] = x;
                out[p] = t.out[s * nsym + x];
                fill[ns] += 1;
            }
        }
        let two = count.iter().all(|&c| c == 2);
        Ok(Preds {
            start,
            prev,
            sym,
            out,
            two,
        })
    }
}

/// Branch scores of every output symbol for one step's `n` received values.
fn branch_scores(dec: DecType, y: &[f64], n: usize, scores: &mut [f64]) {
    for (o, sc) in scores.iter_mut().enumerate() {
        let bit = |j: usize| (o >> (n - 1 - j)) & 1;
        *sc = match dec {
            // The order of the sum is the script decoder's `C * Y`, so the
            // whole-frame result is bit for bit the same.
            DecType::Unquant => {
                let c = |j: usize| if bit(j) == 0 { 1.0 } else { -1.0 };
                let mut acc = c(0) * y[0];
                for j in 1..n {
                    acc += c(j) * y[j];
                }
                acc
            }
            DecType::Hard => -((0..n).filter(|&j| bit(j) as f64 != y[j]).count() as f64),
            DecType::Soft { nsdec } => {
                let top = ((1u64 << nsdec) - 1) as f64;
                -(0..n)
                    .map(|j| if bit(j) == 0 { y[j] } else { top - y[j] })
                    .sum::<f64>()
            }
        };
    }
}

/// Validates received values for `dec`.
fn check_inputs(code: &[f64], dec: DecType) -> Result<(), DspError> {
    match dec {
        DecType::Hard => {
            if let Some(x) = code.iter().find(|&&x| x != 0.0 && x != 1.0) {
                return Err(bad(format!("hard decisions must be 0 or 1, got {x}")));
            }
        }
        DecType::Soft { nsdec } => {
            let top = ((1u64 << nsdec) - 1) as f64;
            if let Some(x) = code
                .iter()
                .find(|&&x| !(x >= 0.0 && x <= top && x.fract() == 0.0))
            {
                return Err(bad(format!(
                    "soft decisions with nsdec = {nsdec} must be integers 0 to {top}, got {x}"
                )));
            }
        }
        DecType::Unquant => {
            if let Some(x) = code.iter().find(|x| !x.is_finite()) {
                return Err(bad(format!("unquantized inputs must be finite, got {x}")));
            }
        }
    }
    Ok(())
}

/// Decision rows: the index of the winning predecessor of every state, one
/// row per step, kept in a ring of `rows` steps.
enum Decisions {
    /// One bit per state.
    Bits { words: usize, data: Vec<u64> },
    /// One byte per state.
    Bytes { states: usize, data: Vec<u8> },
}

impl Decisions {
    fn new(two: bool, states: usize, rows: usize) -> Decisions {
        if two {
            let words = states.div_ceil(64);
            Decisions::Bits {
                words,
                data: vec![0; words * rows],
            }
        } else {
            Decisions::Bytes {
                states,
                data: vec![0; states * rows],
            }
        }
    }

    #[inline]
    fn get(&self, row: usize, s: usize) -> usize {
        match self {
            Decisions::Bits { words, data } => {
                ((data[row * words + s / 64] >> (s % 64)) & 1) as usize
            }
            Decisions::Bytes { states, data } => data[row * states + s] as usize,
        }
    }
}

/// MATLAB `vitdec(code, trellis, tblen, opmode, dectype[, nsdec])`: the
/// decoded input bits, `k` per step.
pub fn vitdec(
    code: &[f64],
    t: &Trellis,
    tblen: usize,
    mode: OpMode,
    dec: DecType,
) -> Result<Vec<u8>, DspError> {
    t.validate()?;
    let n = t.n;
    if !code.len().is_multiple_of(n) {
        return Err(bad(format!(
            "the code length {} must be a multiple of {n}, the output bits per step",
            code.len()
        )));
    }
    if tblen == 0 {
        return Err(bad("tblen must be a positive integer"));
    }
    if let DecType::Soft { nsdec } = dec {
        if !(1..=16).contains(&nsdec) {
            return Err(bad("nsdec must be an integer from 1 to 16"));
        }
    }
    check_inputs(code, dec)?;
    let preds = Preds::of(t)?;
    let steps = code.len() / n;
    let s_count = t.num_states;
    let rows = tblen.min(steps) + 1;
    let mut dec_rows = Decisions::new(preds.two, s_count, rows);

    let mut pm = vec![f64::NEG_INFINITY; s_count];
    pm[0] = 0.0;
    let mut next = vec![0.0; s_count];
    let mut scores = vec![0.0; 1usize << n];
    let mut syms = vec![0u32; steps];

    // Two-branch tables, flat for the inner loop: entry `2 s + d` is the
    // predecessor (and its output symbol) that decision bit `d` picks.
    let (pp, oo): (Vec<u32>, Vec<u32>) = if preds.two {
        (
            preds.prev.iter().map(|&p| p as u32).collect(),
            preds.out.iter().map(|&o| o as u32).collect(),
        )
    } else {
        Default::default()
    };

    // Walk back from the state after step `from_step` through `hops` rows;
    // write the symbols of steps at or above `write_from`. Returns the
    // symbol of the last row visited.
    let trace = |dec_rows: &Decisions,
                 from_step: usize,
                 hops: usize,
                 mut s: usize,
                 syms: &mut [u32],
                 write_from: usize| {
        let mut row = from_step % rows;
        let mut last = 0;
        for h in 0..hops {
            let step = from_step - h;
            let i = match dec_rows {
                Decisions::Bits { words, data } => {
                    2 * s + ((data[row * words + (s >> 6)] >> (s & 63)) & 1) as usize
                }
                Decisions::Bytes { .. } => preds.start[s] + dec_rows.get(row, s),
            };
            if step >= write_from || h + 1 == hops {
                last = preds.sym[i];
                if step >= write_from {
                    syms[step] = last as u32;
                }
            }
            s = if preds.two {
                pp[i] as usize
            } else {
                preds.prev[i]
            };
            row = if row == 0 { rows - 1 } else { row - 1 };
        }
        last
    };

    // The branch a state took at a ring row: an index into `preds`.
    let branch = |dec_rows: &Decisions, row: usize, s: usize| match dec_rows {
        Decisions::Bits { words, data } => {
            2 * s + ((data[row * words + (s >> 6)] >> (s & 63)) & 1) as usize
        }
        Decisions::Bytes { .. } => preds.start[s] + dec_rows.get(row, s),
    };

    // `path[τ % rows]` is the state after step τ on the last traceback,
    // for every τ in its window of `tblen + 1` steps. A new traceback that
    // reaches the same state at the same step follows the old path from
    // there on (the predecessor of a state at a step is fixed), so it stops
    // and reads the state after step `t - tblen` from `path`.
    let mut path = vec![u32::MAX; rows];
    let mut traced: Option<usize> = None;
    let mut window_symbol = |dec_rows: &Decisions, path: &mut Vec<u32>, t: usize, mut s: usize| {
        let low = t - tblen;
        let mut tau = t;
        let at_low = loop {
            let slot = tau % rows;
            if traced.is_some_and(|p| tau <= p) && path[slot] == s as u32 {
                break path[low % rows] as usize;
            }
            path[slot] = s as u32;
            if tau == low {
                break s;
            }
            let i = branch(dec_rows, slot, s);
            s = if preds.two {
                pp[i] as usize
            } else {
                preds.prev[i]
            };
            tau -= 1;
        };
        traced = Some(t);
        preds.sym[branch(dec_rows, low % rows, at_low)] as u32
    };

    let best_state = |pm: &[f64]| {
        let mut b = 0;
        for s in 1..pm.len() {
            if pm[s] > pm[b] {
                b = s;
            }
        }
        b
    };

    for step in 0..steps {
        branch_scores(dec, &code[step * n..(step + 1) * n], n, &mut scores);
        let row = step % rows;
        // Best state after this step: the first with the highest metric.
        let (mut best_v, mut best_s) = (f64::NEG_INFINITY, 0usize);
        match &mut dec_rows {
            Decisions::Bits { words, data } => {
                debug_assert!(pp.iter().all(|&p| (p as usize) < pm.len()));
                debug_assert!(oo.iter().all(|&o| (o as usize) < scores.len()));
                let dst = &mut data[row * *words..(row + 1) * *words];
                for (w, word) in dst.iter_mut().enumerate() {
                    let lo = w * 64;
                    let mut bits = 0u64;
                    for (j, nx) in next[lo..(lo + 64).min(s_count)].iter_mut().enumerate() {
                        let s = lo + j;
                        // SAFETY: `pp` and `oo` have 2 · s_count entries,
                        // `s < s_count`, every `pp` entry is a state (below
                        // `pm.len()`, from `Trellis::validate`), and every
                        // `oo` entry an output symbol (below `scores.len()`).
                        let (a, b) = unsafe {
                            let pa = *pp.get_unchecked(2 * s) as usize;
                            let pb = *pp.get_unchecked(2 * s + 1) as usize;
                            let oa = *oo.get_unchecked(2 * s) as usize;
                            let ob = *oo.get_unchecked(2 * s + 1) as usize;
                            (
                                *pm.get_unchecked(pa) + *scores.get_unchecked(oa),
                                *pm.get_unchecked(pb) + *scores.get_unchecked(ob),
                            )
                        };
                        let take = b > a;
                        bits |= u64::from(take) << j;
                        let v = if take { b } else { a };
                        *nx = v;
                        if v > best_v {
                            best_v = v;
                            best_s = s;
                        }
                    }
                    *word = bits;
                }
            }
            Decisions::Bytes { states, data } => {
                let base = row * *states;
                for s in 0..s_count {
                    let (lo, hi) = (preds.start[s], preds.start[s + 1]);
                    let mut best = pm[preds.prev[lo]] + scores[preds.out[lo]];
                    let mut idx = 0;
                    for i in lo + 1..hi {
                        let v = pm[preds.prev[i]] + scores[preds.out[i]];
                        if v > best {
                            best = v;
                            idx = i - lo;
                        }
                    }
                    data[base + s] = idx as u8;
                    next[s] = best;
                    if best > best_v {
                        best_v = best;
                        best_s = s;
                    }
                }
            }
        }
        std::mem::swap(&mut pm, &mut next);
        if step >= tblen {
            syms[step - tblen] = window_symbol(&dec_rows, &mut path, step, best_s);
        }
    }

    let k = t.k;
    let mut bits = vec![0u8; steps * k];
    let emitted = steps.saturating_sub(tblen);
    match mode {
        OpMode::Cont => {
            // Output t is the symbol of step t - tblen; the first tblen are 0.
            for (j, &sym) in syms[..emitted].iter().enumerate() {
                write_symbol(&mut bits, j + tblen.min(steps), sym as usize, k);
            }
        }
        OpMode::Trunc | OpMode::Term => {
            if steps > 0 {
                let start = if mode == OpMode::Term {
                    0
                } else {
                    best_state(&pm)
                };
                let hops = steps - emitted;
                trace(&dec_rows, steps - 1, hops, start, &mut syms, emitted);
            }
            for (j, &sym) in syms.iter().enumerate() {
                write_symbol(&mut bits, j, sym as usize, k);
            }
        }
    }
    Ok(bits)
}

fn write_symbol(bits: &mut [u8], step: usize, sym: usize, k: usize) {
    for b in 0..k {
        bits[step * k + b] = ((sym >> (k - 1 - b)) & 1) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(t: &Trellis, u: &[u8]) -> Vec<f64> {
        let (k, n) = (t.k, t.n);
        let mut s = 0;
        let mut c = Vec::new();
        for chunk in u.chunks(k) {
            let sym = chunk.iter().fold(0, |a, &b| 2 * a + b as usize);
            let o = t.out[s * (1 << k) + sym];
            for j in 0..n {
                c.push(((o >> (n - 1 - j)) & 1) as f64);
            }
            s = t.next[s * (1 << k) + sym];
        }
        c
    }

    fn bits(seed: u64, len: usize) -> Vec<u8> {
        let mut x = seed;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x & 1) as u8
            })
            .collect()
    }

    #[test]
    fn octal_round_trip() {
        assert_eq!(octal_value(171.0).unwrap(), 0o171);
        assert_eq!(octal_digits(0o133), 133.0);
        assert!(octal_value(19.0).is_err());
        assert!(octal_value(1.5).is_err());
    }

    #[test]
    fn k7_tables_are_the_shift_register() {
        let t = poly2trellis(&[7], &[vec![0o171, 0o133]], None).unwrap();
        assert_eq!(t.num_states, 64);
        for s in 0..64 {
            assert_eq!(t.next[2 * s], s >> 1);
            assert_eq!(t.next[2 * s + 1], (s >> 1) | 32);
        }
    }

    /// A single bit error in a long noiseless block is corrected in every
    /// mode and window length, and the outputs have one bit per input.
    #[test]
    fn corrects_isolated_errors_in_every_mode() {
        let t = poly2trellis(&[7], &[vec![0o171, 0o133]], None).unwrap();
        let mut u = bits(7, 2000);
        u.extend([0; 6]);
        let mut c = encode(&t, &u);
        for p in (0..c.len()).step_by(97) {
            c[p] = 1.0 - c[p];
        }
        for tb in [5 * 7, 96, 10_000] {
            for mode in [OpMode::Trunc, OpMode::Term] {
                assert_eq!(
                    vitdec(&c, &t, tb, mode, DecType::Hard).unwrap(),
                    u,
                    "{mode:?} {tb}"
                );
            }
            let d = vitdec(&c, &t, tb, OpMode::Cont, DecType::Hard).unwrap();
            let lag = tb.min(u.len());
            assert!(d[..lag].iter().all(|&b| b == 0));
            assert_eq!(&d[lag..], &u[..u.len() - lag]);
        }
    }

    /// The generic (byte-decision) path decides as the two-branch path does.
    #[test]
    fn generic_path_matches_the_two_branch_path() {
        let t = poly2trellis(&[5], &[vec![0o23, 0o35]], None).unwrap();
        let mut c = encode(&t, &bits(3, 500));
        for p in (0..c.len()).step_by(11) {
            c[p] = 1.0 - c[p];
        }
        let y: Vec<f64> = c.iter().map(|&b| 1.0 - 2.0 * b).collect();
        let fast = vitdec(&y, &t, 25, OpMode::Trunc, DecType::Unquant).unwrap();
        // Same trellis with every branch listed twice: no longer two
        // predecessors per state, so the byte path runs.
        let mut t2 = t.clone();
        t2.k = 2;
        t2.next = t.next.iter().flat_map(|&s| [s, s]).collect();
        t2.out = t.out.iter().flat_map(|&o| [o, o]).collect();
        let slow = vitdec(&y, &t2, 25, OpMode::Trunc, DecType::Unquant).unwrap();
        let slow_first: Vec<u8> = slow.chunks(2).map(|p| p[0]).collect();
        assert_eq!(fast, slow_first);
    }

    /// The textbook decoder the module notes describe: every decision kept,
    /// a full traceback for every output.
    fn naive(code: &[f64], t: &Trellis, tblen: usize, mode: OpMode, dec: DecType) -> Vec<u8> {
        let (n, k, ns) = (t.n, t.k, t.num_states);
        let nsym = 1 << k;
        let steps = code.len() / n;
        let mut pm = vec![f64::NEG_INFINITY; ns];
        pm[0] = 0.0;
        // back[step][state] = (prev, sym)
        let mut back = vec![vec![(0usize, 0usize); ns]; steps];
        let mut scores = vec![0.0; 1 << n];
        let mut best_at = Vec::new();
        for step in 0..steps {
            branch_scores(dec, &code[step * n..(step + 1) * n], n, &mut scores);
            let mut next = vec![f64::NEG_INFINITY; ns];
            let mut seen = vec![false; ns];
            for s in 0..ns {
                for x in 0..nsym {
                    let d = t.next[s * nsym + x];
                    let v = pm[s] + scores[t.out[s * nsym + x]];
                    if !seen[d] || v > next[d] {
                        seen[d] = true;
                        next[d] = v;
                        back[step][d] = (s, x);
                    }
                }
            }
            pm = next;
            let mut b = 0;
            for s in 1..ns {
                if pm[s] > pm[b] {
                    b = s;
                }
            }
            best_at.push(b);
        }
        let walk = |from: usize, mut s: usize, to: usize| {
            let mut sym = 0;
            for step in (to..=from).rev() {
                let (p, x) = back[step][s];
                sym = x;
                s = p;
            }
            sym
        };
        let mut syms = vec![0; steps];
        for step in tblen..steps {
            syms[step - tblen] = walk(step, best_at[step], step - tblen);
        }
        let emitted = steps.saturating_sub(tblen);
        let mut out = vec![0u8; steps * k];
        if mode == OpMode::Cont {
            for j in 0..emitted {
                write_symbol(&mut out, j + tblen.min(steps), syms[j], k);
            }
            return out;
        }
        let start = if mode == OpMode::Term {
            0
        } else {
            best_at[steps - 1]
        };
        for step in emitted..steps {
            syms[step] = walk(steps - 1, start, step);
        }
        for (j, &x) in syms.iter().enumerate() {
            write_symbol(&mut out, j, x, k);
        }
        out
    }

    /// Noisy decodes equal the naive decoder bit for bit: every mode, window
    /// lengths short and long, hard / soft / unquant, two-branch and
    /// four-branch trellises.
    #[test]
    fn window_decoding_matches_the_naive_decoder() {
        let codes = [
            poly2trellis(&[7], &[vec![0o171, 0o133]], None).unwrap(),
            poly2trellis(&[5], &[vec![0o37, 0o33]], Some(&[0o37])).unwrap(),
            poly2trellis(&[3, 3], &[vec![0o7, 0o5, 0], vec![0, 0o7, 0o5]], None).unwrap(),
        ];
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut unif = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 11) as f64 / (1u64 << 53) as f64
        };
        for t in &codes {
            let u = bits(11, 600 * t.k);
            let c = encode(t, &u);
            let y: Vec<f64> = c
                .iter()
                .map(|&b| 1.0 - 2.0 * b + 1.6 * (unif() - 0.5) * 2.0)
                .collect();
            let h: Vec<f64> = y.iter().map(|&v| if v < 0.0 { 1.0 } else { 0.0 }).collect();
            let q: Vec<f64> = y
                .iter()
                .map(|&v| ((1.0 - v) * 1.75).round().clamp(0.0, 7.0))
                .collect();
            for (input, dec) in [
                (&y, DecType::Unquant),
                (&h, DecType::Hard),
                (&q, DecType::Soft { nsdec: 3 }),
            ] {
                for tb in [1, 2, 7, 35, 96, 5000] {
                    for mode in [OpMode::Trunc, OpMode::Term, OpMode::Cont] {
                        assert_eq!(
                            vitdec(input, t, tb, mode, dec).unwrap(),
                            naive(input, t, tb, mode, dec),
                            "k {} {dec:?} tblen {tb} {mode:?}",
                            t.k
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn rejects_bad_inputs() {
        let t = poly2trellis(&[3], &[vec![0o7, 0o5]], None).unwrap();
        assert!(vitdec(&[0.0, 1.0, 1.0], &t, 2, OpMode::Trunc, DecType::Hard).is_err());
        assert!(vitdec(&[0.0, 2.0], &t, 2, OpMode::Trunc, DecType::Hard).is_err());
        assert!(vitdec(
            &[0.0, 8.0],
            &t,
            2,
            OpMode::Trunc,
            DecType::Soft { nsdec: 3 }
        )
        .is_err());
        assert!(vitdec(&[0.0, f64::NAN], &t, 2, OpMode::Trunc, DecType::Unquant).is_err());
        assert!(poly2trellis(&[3], &[vec![0o17, 0o5]], None).is_err());
    }
}
