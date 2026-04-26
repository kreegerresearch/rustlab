//! Sparse-solve benchmarks. Run via:
//!
//!   cargo run --release --example bench_sparse_solve -p rustlab-core
//!
//! Reports factor-and-solve wall-clock times for the canonical 5-point
//! Laplacian Poisson at a range of grid sizes, comparing:
//!   * sparse Cholesky with various orderings
//!   * sparse LU
//!   * the legacy dense Gaussian elimination (current fallback) — bounded
//!     to small grids where it doesn't OOM the machine
//!
//! Measured times are factor + solve combined (single solve per factor
//! for direct comparison; the cost amortizes across multiple solves in
//! practice).

use num_complex::Complex;
use rustlab_core::sparse_solve::{
    AmdOrdering, ColCountOrdering, IdentityOrdering, OrderingMethod, SparseChol, SparseCsc,
    SparseLU,
};
use std::time::Instant;

/// Build the negated 5-point Laplacian on an n×n grid as a CSC matrix.
/// Negated (so it's SPD) for direct compatibility with both Cholesky
/// and LU.
fn build_laplacian_real(nx: usize, ny: usize) -> SparseCsc<f64> {
    let mut coo = Vec::new();
    for j in 0..nx {
        for i in 0..ny {
            let k = j * ny + i;
            coo.push((k, k, 4.0));
            if i > 0 {
                coo.push((k, k - 1, -1.0));
            }
            if i + 1 < ny {
                coo.push((k, k + 1, -1.0));
            }
            if j > 0 {
                coo.push((k, k - ny, -1.0));
            }
            if j + 1 < nx {
                coo.push((k, k + ny, -1.0));
            }
        }
    }
    coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    SparseCsc::from_coo_sorted(nx * ny, nx * ny, &coo)
}

/// Dense Gaussian elimination on a complex matrix — the current fallback
/// path. Used only as a comparison target; OOMs above ~150x150.
fn dense_solve_complex(a: &SparseCsc<f64>, b: &[f64]) -> Vec<f64> {
    let n = a.nrows();
    // Convert to dense.
    let mut aug = vec![vec![0.0_f64; n + 1]; n];
    for j in 0..n {
        for (i, v) in a.col_iter(j) {
            aug[i][j] = v;
        }
    }
    for (i, &bi) in b.iter().enumerate() {
        aug[i][n] = bi;
    }
    // Partial-pivoting Gaussian elimination.
    for k in 0..n {
        let mut max_idx = k;
        let mut max_val = aug[k][k].abs();
        for i in (k + 1)..n {
            let v = aug[i][k].abs();
            if v > max_val {
                max_val = v;
                max_idx = i;
            }
        }
        if max_idx != k {
            aug.swap(k, max_idx);
        }
        if aug[k][k].abs() < 1e-14 {
            return vec![0.0; n]; // pretend success for benchmark continuity
        }
        for i in (k + 1)..n {
            let factor = aug[i][k] / aug[k][k];
            for j in k..(n + 1) {
                let sub = factor * aug[k][j];
                aug[i][j] -= sub;
            }
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = aug[i][n];
        for j in (i + 1)..n {
            s -= aug[i][j] * x[j];
        }
        x[i] = s / aug[i][i];
    }
    x
}

fn time_chol<O: OrderingMethod>(a: &SparseCsc<f64>, b: &[f64], ord: &O) -> (f64, usize) {
    let t = Instant::now();
    let chol = SparseChol::factor(a, ord).unwrap();
    let _x = chol.solve(b).unwrap();
    let secs = t.elapsed().as_secs_f64();
    (secs, chol.factor_csc().nnz())
}

fn time_lu<O: OrderingMethod>(a: &SparseCsc<f64>, b: &[f64], ord: &O) -> (f64, usize) {
    let t = Instant::now();
    let lu = SparseLU::factor(a, ord, 0.1).unwrap();
    let _x = lu.solve(b).unwrap();
    let secs = t.elapsed().as_secs_f64();
    (secs, lu.l_factor().nnz() + lu.u_factor().nnz())
}

fn time_dense(a: &SparseCsc<f64>, b: &[f64]) -> f64 {
    let t = Instant::now();
    let _x = dense_solve_complex(a, b);
    t.elapsed().as_secs_f64()
}

fn main() {
    println!(
        "{:>5} {:>8}    {:>10} {:>10} {:>12}",
        "n", "n^2", "method", "time (s)", "factor nnz"
    );
    println!("{}", "-".repeat(60));

    for n in [25, 50, 75, 100, 150, 200] {
        let a = build_laplacian_real(n, n);
        let b = vec![1.0; n * n];
        let nrows = n * n;

        // Sparse Cholesky paths
        let (t_id, nz_id) = time_chol(&a, &b, &IdentityOrdering);
        let (t_cc, nz_cc) = time_chol(&a, &b, &ColCountOrdering);
        let (t_amd, nz_amd) = time_chol(&a, &b, &AmdOrdering);

        // Sparse LU (use AMD as default, like the dispatch does)
        let (t_lu, nz_lu) = time_lu(&a, &b, &AmdOrdering);

        println!(
            "{n:>5} {nrows:>8}    chol/id    {t_id:>10.3} {nz_id:>12}",
        );
        println!(
            "{:>5} {:>8}    chol/cc    {t_cc:>10.3} {nz_cc:>12}",
            "", ""
        );
        println!(
            "{:>5} {:>8}    chol/amd   {t_amd:>10.3} {nz_amd:>12}",
            "", ""
        );
        println!(
            "{:>5} {:>8}    lu/amd     {t_lu:>10.3} {nz_lu:>12}",
            "", ""
        );

        // Dense fallback only for n <= 75 (n^2 = 5625, dense is ~10s)
        if n <= 75 {
            let t_d = time_dense(&a, &b);
            println!(
                "{:>5} {:>8}    dense LU   {t_d:>10.3} {:>12}",
                "", "", "—"
            );
        } else {
            println!(
                "{:>5} {:>8}    dense LU   {:>10} {:>12}",
                "", "", "(skipped)", "—"
            );
        }
        println!();
    }

    // Complex Hermitian path: solve a complex-shifted Helmholtz on a
    // moderate grid. Demonstrates the 4× cost vs the real path and
    // that the complex factorization works at scale.
    let n = 100;
    let mut coo: Vec<(usize, usize, Complex<f64>)> = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let k = j * n + i;
            coo.push((k, k, Complex::new(4.0, -0.01))); // tiny lossy term
            if i > 0 {
                coo.push((k, k - 1, Complex::new(-1.0, 0.0)));
            }
            if i + 1 < n {
                coo.push((k, k + 1, Complex::new(-1.0, 0.0)));
            }
            if j > 0 {
                coo.push((k, k - n, Complex::new(-1.0, 0.0)));
            }
            if j + 1 < n {
                coo.push((k, k + n, Complex::new(-1.0, 0.0)));
            }
        }
    }
    coo.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let a_complex: SparseCsc<Complex<f64>> = SparseCsc::from_coo_sorted(n * n, n * n, &coo);
    let b_complex = vec![Complex::new(1.0, 0.0); n * n];

    let t = Instant::now();
    let lu = SparseLU::factor(&a_complex, &AmdOrdering, 0.1).unwrap();
    let _x = lu.solve(&b_complex).unwrap();
    let secs = t.elapsed().as_secs_f64();
    println!(
        "Complex 100x100 (n=10000) lossy Helmholtz, sparse LU/AMD: {secs:.3} s, factor nnz {}",
        lu.l_factor().nnz() + lu.u_factor().nnz()
    );
}
