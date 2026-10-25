//! Real `f64` matrix multiply on the Arm Scalable Matrix Extension (SME).
//!
//! SME adds a matrix unit with a two-dimensional register, ZA, and an
//! outer-product instruction: `fmopa za0.d, …, z0.d, z1.d` adds the 8 × 8
//! outer product of two 8-element vectors to tile `za0` (at a 512-bit
//! streaming vector length, which is what this kernel requires). On an
//! Apple M4 one thread reaches about 490 GFLOP/s this way, against about
//! 50 for the vector-unit GEMM.
//!
//! The method is the textbook one:
//!
//! 1. Pack A into panels of 16 rows and B into panels of 16 columns, each
//!    stored k-major and zero-padded at the ragged edge, so the kernel never
//!    needs edge masks. Packing reads any strides, so transposed views cost
//!    the same as row-major operands.
//! 2. For each 16 × 16 block of C, clear four ZA tiles (the block's four
//!    8 × 8 quarters) and, for every k, load one 16-element column of the A
//!    panel and one 16-element row of the B panel and issue four `fmopa`s.
//! 3. Store the tiles into a row-major buffer padded to multiples of 16,
//!    and copy the `m × n` result out of it.
//!
//! Each element of C is therefore `Σ_k a_ik · b_kj` accumulated in k order
//! with a fused multiply-add per term, starting from zero. That is a fixed
//! order, so results are deterministic, and it differs from
//! `matrixmultiply` only in the last digits.
//!
//! Rust code must not run in streaming mode, so the whole product, loops
//! included, is one `asm!` block between `smstart` and `smstop`.
//! `smstart` and `smstop` zero the vector and predicate registers, which is
//! why every `v` register is declared clobbered.

use crate::types::{CMatrix, C64};
use ndarray::{Array2, ArrayView2};
use std::sync::OnceLock;

/// Rows and columns of C per kernel block: two 8-element vectors.
const BLOCK: usize = 16;

/// True when this process may use the SME kernel: the CPU has SME with
/// `f64` outer products, the streaming vector length is 512 bits, and
/// `RUSTLAB_NO_SME` is unset. Checked once.
pub(crate) fn available() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RUSTLAB_NO_SME").is_none() && detect())
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
fn detect() -> bool {
    use std::ffi::{c_char, c_int, c_void};
    extern "C" {
        // libSystem; always linked on macOS.
        fn sysctlbyname(
            name: *const c_char,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> c_int;
    }
    let flag = |name: &std::ffi::CStr| {
        let mut v: c_int = 0;
        let mut len = std::mem::size_of::<c_int>();
        // SAFETY: `v` and `len` are valid for writes of the sizes given; no
        // new value is set.
        let r = unsafe {
            sysctlbyname(name.as_ptr(), (&mut v as *mut c_int).cast(), &mut len, std::ptr::null_mut(), 0)
        };
        r == 0 && v == 1
    };
    if !(flag(c"hw.optional.arm.FEAT_SME") && flag(c"hw.optional.arm.FEAT_SME_F64F64")) {
        return false;
    }
    // `rdsvl` reads the streaming vector length in bytes without entering
    // streaming mode. The kernel's offsets assume 64 (8 doubles).
    let svl: u64;
    // SAFETY: SME is present (checked above), and `rdsvl` only reads.
    unsafe {
        std::arch::asm!(
            ".arch_extension sme",
            "rdsvl {svl}, #1",
            svl = out(reg) svl,
            options(nomem, nostack, preserves_flags),
        );
    }
    svl == 64
}

#[cfg(not(all(target_arch = "aarch64", target_os = "macos")))]
fn detect() -> bool {
    false
}

/// `a · b` on the matrix unit, for real `f64` operands. The caller checks
/// [`available`] and that the inner dimensions agree. Row-major result.
pub(crate) fn gemm(a: &ArrayView2<f64>, b: &ArrayView2<f64>) -> Array2<f64> {
    let (m, n) = (a.nrows(), b.ncols());
    let out = product(Src::of_f64(a), Src::of_f64(b), |c, np| unpack(c, np, m, n, |x| x)).expect("real operands");
    Array2::from_shape_vec((m, n), out).expect("length matches the shape")
}

/// `a · b` for complex-stored operands whose imaginary parts are all zero,
/// read straight from the complex storage (no real copy first). `None` when
/// either operand has a nonzero (or NaN) imaginary part; the caller then
/// takes the complex path. The result is row-major with zero imaginary
/// parts.
pub(crate) fn gemm_real_c64(a: &ArrayView2<C64>, b: &ArrayView2<C64>) -> Option<CMatrix> {
    let (m, n) = (a.nrows(), b.ncols());
    let out = product(Src::of_c64(a), Src::of_c64(b), |c, np| unpack(c, np, m, n, |x| C64::new(x, 0.0)))?;
    Some(Array2::from_shape_vec((m, n), out).expect("length matches the shape"))
}

/// A matrix operand as a raw base pointer plus strides, in `f64` words.
/// Complex storage is read as interleaved words with `step` 2: the real
/// part at offset 0 and the imaginary part at offset 1.
struct Src {
    ptr: *const f64,
    rows: usize,
    cols: usize,
    row_stride: isize,
    col_stride: isize,
    complex: bool,
}

impl Src {
    fn of_f64(a: &ArrayView2<f64>) -> Src {
        let s = a.strides();
        Src { ptr: a.as_ptr(), rows: a.nrows(), cols: a.ncols(), row_stride: s[0], col_stride: s[1], complex: false }
    }

    fn of_c64(a: &ArrayView2<C64>) -> Src {
        let s = a.strides();
        // `Complex<f64>` is `#[repr(C)]` (re, im): element strides double.
        Src {
            ptr: a.as_ptr().cast::<f64>(),
            rows: a.nrows(),
            cols: a.ncols(),
            row_stride: s[0] * 2,
            col_stride: s[1] * 2,
            complex: true,
        }
    }
}

/// Per-thread packing and output buffers, reused across calls. A fresh
/// multi-megabyte allocation per product costs a page fault per page on
/// first touch, which was half the time of a 512³ product. Buffers larger
/// than [`KEEP_MAX`] words are released after the call.
#[derive(Default)]
struct Scratch {
    a: Vec<f64>,
    b: Vec<f64>,
    c: Vec<f64>,
}

/// 4 M words (32 MB) per buffer; enough for a 2048 × 2048 operand.
const KEEP_MAX: usize = 1 << 22;

thread_local! {
    static SCRATCH: std::cell::RefCell<Scratch> = std::cell::RefCell::new(Scratch::default());
}

/// `a · b` padded to `mp × np` (the dimensions rounded up to 16), handed
/// to `finish` as a row-major slice with `np` words per row. `None` when an
/// operand has a nonzero imaginary part.
fn product<T>(a: Src, b: Src, finish: impl FnOnce(&[f64], usize) -> T) -> Option<T> {
    let (m, k, n) = (a.rows, a.cols, b.cols);
    debug_assert_eq!(k, b.rows);
    let (mb, nb) = (m.div_ceil(BLOCK), n.div_ceil(BLOCK));
    let np = nb * BLOCK;
    SCRATCH.with_borrow_mut(|s| {
        // The kernel writes every word of the padded output, so a reused
        // buffer is not cleared.
        let need = mb * BLOCK * np;
        if s.c.len() < need {
            s.c.resize(need, 0.0);
        }
        if m > 0 && n > 0 && k > 0 {
            // A's panels run down its rows, B's across its columns.
            if !pack(&a, m, a.row_stride, a.col_stride, k, &mut s.a)
                || !pack(&b, n, b.col_stride, b.row_stride, k, &mut s.b)
            {
                return None;
            }
            kernel(&s.a, &s.b, &mut s.c[..need], k, mb, nb, np);
        } else {
            s.c[..need].fill(0.0);
        }
        let out = finish(&s.c[..need], np);
        for v in [&mut s.a, &mut s.b, &mut s.c] {
            if v.capacity() > KEEP_MAX {
                *v = Vec::new();
            }
        }
        Some(out)
    })
}

/// Panels of 16 along one dimension (`len` long, `along` stride), k-major:
/// `out[p·16k + kk·16 + r] = src[(p·16 + r)·along + kk·kstride]`, zero past
/// `len`. For A that is 16 rows per panel, for B 16 columns. False when a
/// complex source has a nonzero imaginary part.
fn pack(src: &Src, len: usize, along: isize, kstride: isize, k: usize, out: &mut Vec<f64>) -> bool {
    /// k-steps per pass over a panel: the writes of one pass (64 × 16
    /// words, 8 KB) stay in L1 while each of the 16 lines is read in turn.
    const KB: usize = 64;
    let panels = len.div_ceil(BLOCK);
    // Every word is written below (padding included), so a reused buffer
    // is not cleared.
    let need = panels * BLOCK * k;
    if out.len() < need {
        out.resize(need, 0.0);
    }
    let mut nonreal = false;
    let im_off = usize::from(src.complex);
    for (p, panel) in out[..need].chunks_exact_mut(BLOCK * k).enumerate() {
        let r0 = p * BLOCK;
        let live = BLOCK.min(len - r0);
        let word = |r: usize, kk: usize| {
            // SAFETY: (r0 + r, kk) is inside the view whose strides these
            // are, so the word (and, for complex storage, the next one) is
            // inside its allocation.
            let at = unsafe { src.ptr.offset((r0 + r) as isize * along + kk as isize * kstride) };
            unsafe { (*at, *at.add(im_off)) }
        };
        if along.unsigned_abs() <= kstride.unsigned_abs() {
            // The panel's 16 lines are adjacent in memory (B row-major, or
            // A transposed): read each k-step's 16 words in order. Walking
            // down k instead would stride by a whole row, and a 4 KB row
            // maps every read to the same L1 set.
            for kk in 0..k {
                let dst = &mut panel[kk * BLOCK..(kk + 1) * BLOCK];
                for (r, d) in dst[..live].iter_mut().enumerate() {
                    let (re, im) = word(r, kk);
                    nonreal |= src.complex && im != 0.0;
                    *d = re;
                }
                dst[live..].fill(0.0);
            }
            continue;
        }
        // Each line is contiguous along k (A row-major): read a line 64
        // k-steps at a time; the 8 KB of writes per pass stay in L1.
        for k0 in (0..k).step_by(KB) {
            let k1 = (k0 + KB).min(k);
            for r in 0..BLOCK {
                for kk in k0..k1 {
                    panel[kk * BLOCK + r] = if r < live {
                        let (re, im) = word(r, kk);
                        nonreal |= src.complex && im != 0.0;
                        re
                    } else {
                        0.0
                    };
                }
            }
        }
    }
    !nonreal
}

/// The leading `m × n` of a padded product, converted element by element.
fn unpack<T>(c: &[f64], np: usize, m: usize, n: usize, f: impl Fn(f64) -> T) -> Vec<T> {
    let mut out = Vec::with_capacity(m * n);
    for row in c.chunks_exact(np.max(1)).take(m) {
        out.extend(row[..n].iter().map(|&x| f(x)));
    }
    out
}

/// `c[ib·16 + r, jb·16 + s] = Σ_kk pa(ib)[kk, r] · pb(jb)[kk, s]` for every
/// block, with `c` row-major at `ldc` elements per row.
#[cfg(target_arch = "aarch64")]
fn kernel(pa: &[f64], pb: &[f64], c: &mut [f64], k: usize, mb: usize, nb: usize, ldc: usize) {
    assert!(pa.len() >= mb * BLOCK * k && pb.len() >= nb * BLOCK * k);
    assert!(c.len() >= (mb * BLOCK - 1) * ldc + nb * BLOCK && ldc >= nb * BLOCK);
    assert!(k > 0 && mb > 0 && nb > 0);
    // SAFETY: `available()` established SME with f64 outer products and a
    // 64-byte streaming vector length, which the offsets below assume. The
    // asserts above bound every load (16k doubles per panel) and store
    // (16 rows of 16 doubles per block) inside the slices. No Rust code
    // runs between `smstart` and `smstop`.
    unsafe {
        std::arch::asm!(
            ".arch_extension sme",
            ".arch_extension sme-f64f64",
            "smstart",
            "ptrue p0.d",
            "lsl x6, x6, #3",            // ldc in bytes
            "mov x7, #0",                // jb
            "10:",
            "mov x8, #0",                // ib
            "20:",
            "zero {{za}}",
            "mul x9, x8, x3",            // A panel: pa + ib·16k·8
            "add x9, x0, x9, lsl #7",
            "mul x10, x7, x3",           // B panel: pb + jb·16k·8
            "add x10, x1, x10, lsl #7",
            "mov x11, x3",
            "30:",
            "ld1d {{z0.d}}, p0/z, [x9]",
            "ld1d {{z1.d}}, p0/z, [x9, #1, mul vl]",
            "ld1d {{z2.d}}, p0/z, [x10]",
            "ld1d {{z3.d}}, p0/z, [x10, #1, mul vl]",
            "fmopa za0.d, p0/m, p0/m, z0.d, z2.d", // rows 0-7,  cols 0-7
            "fmopa za1.d, p0/m, p0/m, z1.d, z2.d", // rows 8-15, cols 0-7
            "fmopa za2.d, p0/m, p0/m, z0.d, z3.d", // rows 0-7,  cols 8-15
            "fmopa za3.d, p0/m, p0/m, z1.d, z3.d", // rows 8-15, cols 8-15
            "add x9, x9, #128",
            "add x10, x10, #128",
            "subs x11, x11, #1",
            "b.ne 30b",
            // Block origin: c + ib·16·ldc + jb·16·8; x14 is its row 8.
            "mul x13, x8, x6",
            "lsl x13, x13, #4",
            "add x13, x13, x7, lsl #7",
            "add x13, x2, x13",
            "add x14, x13, x6, lsl #3",
            "mov w12, #0",
            "40:",
            "st1d {{za0h.d[w12, 0]}}, p0, [x13]",
            "add x15, x13, #64",
            "st1d {{za2h.d[w12, 0]}}, p0, [x15]",
            "st1d {{za1h.d[w12, 0]}}, p0, [x14]",
            "add x15, x14, #64",
            "st1d {{za3h.d[w12, 0]}}, p0, [x15]",
            "add x13, x13, x6",
            "add x14, x14, x6",
            "add w12, w12, #1",
            "cmp w12, #8",
            "b.lt 40b",
            "add x8, x8, #1",
            "cmp x8, x4",
            "b.lt 20b",
            "add x7, x7, #1",
            "cmp x7, x5",
            "b.lt 10b",
            "smstop",
            in("x0") pa.as_ptr(),
            in("x1") pb.as_ptr(),
            in("x2") c.as_mut_ptr(),
            in("x3") k,
            in("x4") mb,
            in("x5") nb,
            in("x6") ldc,
            out("x7") _, out("x8") _, out("x9") _, out("x10") _, out("x11") _,
            out("x12") _, out("x13") _, out("x14") _, out("x15") _,
            out("v8") _, out("v9") _, out("v10") _, out("v11") _,
            out("v12") _, out("v13") _, out("v14") _, out("v15") _,
            out("p0") _,
            clobber_abi("C"),
            options(nostack),
        );
    }
}

#[cfg(not(target_arch = "aarch64"))]
fn kernel(_: &[f64], _: &[f64], _: &mut [f64], _: usize, _: usize, _: usize, _: usize) {
    unreachable!("available() is false off aarch64")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(m: usize, n: usize, seed: u64) -> Array2<f64> {
        let mut s = seed;
        Array2::from_shape_fn((m, n), |_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((s >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
        })
    }

    /// The kernel's order exactly: zero, then one fused multiply-add per k.
    fn fma_reference(a: &ArrayView2<f64>, b: &ArrayView2<f64>) -> Array2<f64> {
        Array2::from_shape_fn((a.nrows(), b.ncols()), |(i, j)| {
            (0..a.ncols()).fold(0.0f64, |acc, p| a[[i, p]].mul_add(b[[p, j]], acc))
        })
    }

    fn assert_bits(x: &Array2<f64>, y: &Array2<f64>, what: &str) {
        assert_eq!(x.shape(), y.shape(), "{what}");
        for (p, q) in x.iter().zip(y.iter()) {
            assert!(p.to_bits() == q.to_bits(), "{what}: {p} vs {q}");
        }
    }

    #[test]
    fn matches_the_fused_k_order_bit_for_bit() {
        if !available() {
            eprintln!("SME not available here; kernel not exercised");
            return;
        }
        let dims = [1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 129];
        for &m in &dims {
            for &n in &dims {
                for &k in &[1, 2, 8, 33, 128] {
                    let a = filled(m, k, (m * 1000 + k) as u64);
                    let b = filled(k, n, (n * 1000 + k + 7) as u64);
                    assert_bits(&gemm(&a.view(), &b.view()), &fma_reference(&a.view(), &b.view()), &format!("{m}x{k}x{n}"));
                }
            }
        }
    }

    #[test]
    fn strided_and_transposed_operands() {
        if !available() {
            return;
        }
        let at = filled(40, 37, 1);
        let bt = filled(23, 40, 2);
        let want = fma_reference(&at.t(), &bt.t());
        assert_bits(&gemm(&at.t(), &bt.t()), &want, "A' * B'");
        let big = filled(80, 74, 3);
        let a = big.slice(ndarray::s![..;2, ..;2]); // 40 × 37, strided
        let b = filled(37, 19, 4);
        assert_bits(&gemm(&a, &b.view()), &fma_reference(&a, &b.view()), "strided A");
    }

    #[test]
    fn large_and_special_values() {
        if !available() {
            return;
        }
        let a = filled(512, 512, 5);
        let b = filled(512, 512, 6);
        assert_bits(&gemm(&a.view(), &b.view()), &fma_reference(&a.view(), &b.view()), "512^3");

        let mut a = filled(20, 9, 7);
        let b = filled(9, 18, 8);
        a[[3, 4]] = f64::NAN;
        a[[5, 0]] = f64::INFINITY;
        let c = gemm(&a.view(), &b.view());
        assert!(c.row(3).iter().all(|x| x.is_nan()));
        assert!(c.row(5).iter().all(|x| x.is_infinite() || x.is_nan()));
        assert!(c.row(6).iter().all(|x| x.is_finite()));
        // Small integers multiply exactly in any order.
        let ai = Array2::from_shape_fn((33, 17), |(i, j)| ((i * 3 + j) % 11) as f64 - 5.0);
        let bi = Array2::from_shape_fn((17, 21), |(i, j)| ((i + 2 * j) % 7) as f64 - 3.0);
        assert_eq!(gemm(&ai.view(), &bi.view()), ai.dot(&bi));
    }

    #[test]
    fn empty_dimensions() {
        let a = Array2::<f64>::zeros((3, 0));
        let b = Array2::<f64>::zeros((0, 4));
        assert_eq!(gemm(&a.view(), &b.view()), Array2::<f64>::zeros((3, 4)));
        let a = Array2::<f64>::zeros((0, 5));
        let b = Array2::<f64>::zeros((5, 2));
        assert_eq!(gemm(&a.view(), &b.view()).dim(), (0, 2));
    }

    #[test]
    fn concurrent_calls_agree() {
        if !available() {
            return;
        }
        let a = filled(129, 128, 9);
        let b = filled(128, 512, 10);
        let want = gemm(&a.view(), &b.view());
        std::thread::scope(|s| {
            let hs: Vec<_> = (0..8).map(|_| s.spawn(|| gemm(&a.view(), &b.view()))).collect();
            for h in hs {
                assert_bits(&h.join().unwrap(), &want, "thread");
            }
        });
    }

    /// Timing breakdown; run with
    /// `cargo test --release -p rustlab-core --lib sme::tests::breakdown -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn breakdown() {
        if !available() {
            return;
        }
        for &(m, k, n) in &[(512, 512, 512), (129, 128, 512), (129, 128, 128), (129, 32, 129), (32, 32, 32)] {
            let a = filled(m, k, 1);
            let b = filled(k, n, 2);
            let (mb, nb) = (m.div_ceil(BLOCK), n.div_ceil(BLOCK));
            let reps = (4e9 / (2.0 * (m * n * k) as f64)).max(20.0) as usize;
            let time = |f: &mut dyn FnMut()| {
                f();
                let t = std::time::Instant::now();
                for _ in 0..reps {
                    f();
                }
                t.elapsed().as_secs_f64() / reps as f64
            };
            let (sa, sb) = (Src::of_f64(&a.view()), Src::of_f64(&b.view()));
            let (mut pa, mut pb) = (Vec::new(), Vec::new());
            assert!(pack(&sa, m, sa.row_stride, sa.col_stride, k, &mut pa));
            assert!(pack(&sb, n, sb.col_stride, sb.row_stride, k, &mut pb));
            let mut cp = vec![0.0; mb * BLOCK * nb * BLOCK];
            let tk = time(&mut || kernel(&pa, &pb, &mut cp, k, mb, nb, nb * BLOCK));
            let ta = time(&mut || {
                let mut x = std::mem::take(&mut pa);
                pack(&sa, m, sa.row_stride, sa.col_stride, k, &mut x);
                pa = x;
            });
            let tb = time(&mut || {
                let mut y = std::mem::take(&mut pb);
                pack(&sb, n, sb.col_stride, sb.row_stride, k, &mut y);
                pb = y;
            });
            let tp = ta + tb;
            println!("  pack A {:.1} us ({} el), pack B {:.1} us ({} el)", ta * 1e6, m * k, tb * 1e6, k * n);
            let tg = time(&mut || {
                std::hint::black_box(gemm(&a.view(), &b.view()));
            });
            let f = 2.0 * (m * n * k) as f64;
            println!(
                "{m}x{k}x{n}: kernel {:.1} us ({:.0} GFLOP/s), pack {:.1} us, gemm {:.1} us ({:.0} GFLOP/s)",
                tk * 1e6, f / tk / 1e9, tp * 1e6, tg * 1e6, f / tg / 1e9
            );
        }
    }
}
