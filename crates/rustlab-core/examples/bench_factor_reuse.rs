//! Phase 1 demo: factor once, solve N times — the canonical pattern
//! for parameter sweeps and animations.
//!
//! Run via:
//!
//!   cargo run --release --example bench_factor_reuse -p rustlab-core
//!
//! Compares two strategies on a 100×100 SPD grid Laplacian:
//!   (a) `spsolve(A, b)` repeated N times — refactor every call.
//!   (b) `chol(A); solve(F, b)` — factor once, back-solve N times.
//!
//! Strategy (b) is the Phase 1 contribution: it amortizes the factor
//! cost across all N right-hand sides instead of paying it N times.

use rustlab_core::sparse_solve::{
    AmdOrdering, IdentityOrdering, OrderingMethod, SparseChol, SparseCsc,
};
use std::time::Instant;

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
    SparseCsc::<f64>::from_coo_sorted(nx * ny, nx * ny, &coo)
}

fn refactor_per_solve<O: OrderingMethod>(
    a: &SparseCsc<f64>,
    rhs_list: &[Vec<f64>],
    ord: &O,
) -> f64 {
    let t = Instant::now();
    for b in rhs_list {
        let chol = SparseChol::factor(a, ord).expect("SPD");
        let _ = chol.solve(b).expect("dim ok");
    }
    t.elapsed().as_secs_f64()
}

fn factor_once_solve_many<O: OrderingMethod>(
    a: &SparseCsc<f64>,
    rhs_list: &[Vec<f64>],
    ord: &O,
) -> f64 {
    let t = Instant::now();
    let chol = SparseChol::factor(a, ord).expect("SPD");
    for b in rhs_list {
        let _ = chol.solve(b).expect("dim ok");
    }
    t.elapsed().as_secs_f64()
}

fn main() {
    let nx = 100;
    let ny = 100;
    let n = nx * ny;
    let a = build_laplacian_real(nx, ny);

    println!("Phase 1 factor-reuse demo (100×100 grid Laplacian, n={n})");
    println!();

    for &n_rhs in &[1_usize, 5, 10, 25, 50, 100] {
        // Build N distinct RHS so the back-solve does real work each time.
        let rhs_list: Vec<Vec<f64>> = (0..n_rhs)
            .map(|seed| {
                let mut state = (seed as u64).wrapping_add(1);
                (0..n)
                    .map(|_| {
                        state = state
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        ((state >> 33) as f64) / (u32::MAX as f64) - 0.5
                    })
                    .collect()
            })
            .collect();

        // Identity ordering — the Phase 2 default for grid Laplacians.
        let t_refactor = refactor_per_solve(&a, &rhs_list, &IdentityOrdering);
        let t_reuse = factor_once_solve_many(&a, &rhs_list, &IdentityOrdering);
        let speedup = t_refactor / t_reuse;
        println!(
            "  N_rhs = {n_rhs:>3} (id):  refactor={t_refactor:>7.4}s   factor-once={t_reuse:>7.4}s   speedup={speedup:>5.2}×"
        );

        // AMD ordering — the default for unhinted matrices, included for
        // contrast (factor cost is much higher under AMD on grids).
        let t_refactor_amd = refactor_per_solve(&a, &rhs_list, &AmdOrdering);
        let t_reuse_amd = factor_once_solve_many(&a, &rhs_list, &AmdOrdering);
        let speedup_amd = t_refactor_amd / t_reuse_amd;
        println!(
            "  N_rhs = {n_rhs:>3} (amd): refactor={t_refactor_amd:>7.4}s   factor-once={t_reuse_amd:>7.4}s   speedup={speedup_amd:>5.2}×"
        );
        println!();
    }
}
