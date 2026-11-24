//! Preconditioned conjugate gradients with MATLAB's `pcg` contract.
//!
//! The iteration, the stopping rules, and the returned values follow
//! MATLAB's `pcg`: the preconditioner is `M = M1·M2` applied as
//! `z = M2 \ (M1 \ r)`; convergence is `‖b − A·x‖ ≤ tol·‖b‖`, checked on
//! the true residual once the recursive one passes; a run that does not
//! converge returns the iterate with the smallest residual. Flags:
//!
//! | flag | meaning |
//! |---|---|
//! | 0 | converged |
//! | 1 | `maxit` iterations without converging |
//! | 2 | a preconditioner solve was not finite (ill-conditioned `M`) |
//! | 3 | stagnated (two consecutive iterates equal, or the true residual could not reach `tol`) |
//! | 4 | a scalar (`ρ`, `β`, `p'·A·p`, `α`) was zero, infinite, or `p'·A·p ≤ 0` |
//!
//! `A` must be Hermitian positive definite for the theory to hold; the
//! code does not check it (MATLAB does not either). An indefinite `A`
//! usually ends in flag 4.

use crate::sparse_solve::{SparseCsc, SparseScalar, TriangularSolve};

/// The operator and preconditioner of one `pcg` run. Implemented by
/// [`MatrixPcg`] for sparse matrices and by the script layer for
/// function handles.
pub trait PcgOperators<T: SparseScalar> {
    type Error;
    /// `A·x`.
    fn apply_a(&mut self, x: &[T]) -> Result<Vec<T>, Self::Error>;
    fn has_m1(&self) -> bool;
    fn has_m2(&self) -> bool;
    /// `M1 \ r`.
    fn solve_m1(&mut self, r: &[T]) -> Result<Vec<T>, Self::Error>;
    /// `M2 \ y`.
    fn solve_m2(&mut self, y: &[T]) -> Result<Vec<T>, Self::Error>;
}

/// Result of [`pcg`]: MATLAB's `[x, flag, relres, iter, resvec]` plus
/// what the caller needs for MATLAB's messages.
#[derive(Debug, Clone)]
pub struct PcgOutcome<T> {
    pub x: Vec<T>,
    pub flag: u8,
    /// `‖b − A·x‖ / ‖b‖` of the returned `x` (0 when `b = 0`).
    pub relres: f64,
    /// Iteration that produced the returned `x` (0 = the initial guess).
    pub iter: usize,
    /// Iteration at which the loop stopped (the "stopped at" of the
    /// warning); equals `iter` on success.
    pub stopped_at: usize,
    /// Residual norms, `resvec[0] = ‖b − A·x0‖`. Length `stopped_at + 1`
    /// for flags 0, 1, 3 and `stopped_at` for flags 2, 4.
    pub resvec: Vec<f64>,
    /// `b` was all zero, so `x = 0` without iterating.
    pub zero_rhs: bool,
    /// Flag 3 came from the true residual not reaching `tol` after the
    /// recursive one did (MATLAB's "tol may not be achievable" warning).
    pub tol_unreachable: bool,
}

/// A sparse `A` with optional triangular preconditioners.
#[derive(Debug, Clone)]
pub struct MatrixPcg<T: SparseScalar> {
    pub a: SparseCsc<T>,
    pub m1: Option<TriangularSolve<T>>,
    pub m2: Option<TriangularSolve<T>>,
}

impl<T: SparseScalar> MatrixPcg<T> {
    pub fn new(a: SparseCsc<T>) -> Self {
        MatrixPcg {
            a,
            m1: None,
            m2: None,
        }
    }
    pub fn with_m1(mut self, m: TriangularSolve<T>) -> Self {
        self.m1 = Some(m);
        self
    }
    pub fn with_m2(mut self, m: TriangularSolve<T>) -> Self {
        self.m2 = Some(m);
        self
    }
}

impl<T: SparseScalar> PcgOperators<T> for MatrixPcg<T> {
    type Error = std::convert::Infallible;
    fn apply_a(&mut self, x: &[T]) -> Result<Vec<T>, Self::Error> {
        Ok(self.a.spmv(x))
    }
    fn has_m1(&self) -> bool {
        self.m1.is_some()
    }
    fn has_m2(&self) -> bool {
        self.m2.is_some()
    }
    fn solve_m1(&mut self, r: &[T]) -> Result<Vec<T>, Self::Error> {
        Ok(self.m1.as_ref().map_or_else(|| r.to_vec(), |m| m.solve(r)))
    }
    fn solve_m2(&mut self, y: &[T]) -> Result<Vec<T>, Self::Error> {
        Ok(self.m2.as_ref().map_or_else(|| y.to_vec(), |m| m.solve(y)))
    }
}

fn norm<T: SparseScalar>(v: &[T]) -> f64 {
    v.iter().map(|x| x.abs2()).sum::<f64>().sqrt()
}

/// `u' * v` (conjugating `u`).
fn dot<T: SparseScalar>(u: &[T], v: &[T]) -> T {
    let mut s = T::zero();
    for (a, b) in u.iter().zip(v) {
        s += a.conj() * *b;
    }
    s
}

fn residual<T: SparseScalar>(b: &[T], ax: &[T]) -> Vec<T> {
    b.iter().zip(ax).map(|(&p, &q)| p - q).collect()
}

fn is_inf<T: SparseScalar>(t: T) -> bool {
    t.abs().is_infinite()
}

/// Solve `A·x = b` by preconditioned conjugate gradients.
///
/// `tol` is used as given (the caller applies MATLAB's clamping to
/// `[eps, 1 − eps]`). `x0 = None` starts from zero.
pub fn pcg<T: SparseScalar, O: PcgOperators<T>>(
    ops: &mut O,
    b: &[T],
    x0: Option<&[T]>,
    tol: f64,
    maxit: usize,
) -> Result<PcgOutcome<T>, O::Error> {
    let n = b.len();
    let n2b = norm(b);
    if n2b == 0.0 {
        return Ok(PcgOutcome {
            x: vec![T::zero(); n],
            flag: 0,
            relres: 0.0,
            iter: 0,
            stopped_at: 0,
            resvec: vec![0.0],
            zero_rhs: true,
            tol_unreachable: false,
        });
    }
    let mut x = x0.map_or_else(|| vec![T::zero(); n], <[T]>::to_vec);
    let tolb = tol * n2b;
    let mut r = residual(b, &ops.apply_a(&x)?);
    let normr = norm(&r);
    if normr <= tolb {
        return Ok(PcgOutcome {
            x,
            flag: 0,
            relres: normr / n2b,
            iter: 0,
            stopped_at: 0,
            resvec: vec![normr],
            zero_rhs: false,
            tol_unreachable: false,
        });
    }

    let mut flag = 1u8;
    let mut xmin = x.clone();
    let mut imin = 0usize;
    let mut iter = 0usize;
    let mut normr_act = normr;
    let mut resvec = Vec::with_capacity(maxit.min(4096) + 1);
    resvec.push(normr);
    let mut normrmin = normr;
    let mut rho = T::one();
    let mut stag = 0usize;
    let mut moresteps = 0usize;
    let maxmsteps = (n as i64 / 50).min(5).min(n as i64 - maxit as i64);
    const MAXSTAGSTEPS: usize = 3;
    let mut tol_unreachable = false;
    let mut p: Vec<T> = Vec::new();
    let mut ii = 0usize;

    for it in 1..=maxit {
        ii = it;
        let y = if ops.has_m1() {
            let y = ops.solve_m1(&r)?;
            if !y.iter().all(SparseScalar::is_finite) {
                flag = 2;
                break;
            }
            y
        } else {
            r.clone()
        };
        let z = if ops.has_m2() {
            let z = ops.solve_m2(&y)?;
            if !z.iter().all(SparseScalar::is_finite) {
                flag = 2;
                break;
            }
            z
        } else {
            y
        };

        let rho1 = rho;
        rho = dot(&r, &z);
        if rho.abs() == 0.0 || is_inf(rho) {
            flag = 4;
            break;
        }
        if it == 1 {
            p = z;
        } else {
            let beta = rho / rho1;
            if beta.abs() == 0.0 || is_inf(beta) {
                flag = 4;
                break;
            }
            for (pi, zi) in p.iter_mut().zip(&z) {
                *pi = *zi + beta * *pi;
            }
        }
        let q = ops.apply_a(&p)?;
        let pq = dot(&p, &q);
        if pq.re() <= 0.0 || is_inf(pq) {
            flag = 4;
            break;
        }
        let alpha = rho / pq;
        if is_inf(alpha) {
            flag = 4;
            break;
        }

        if norm(&p) * alpha.abs() < f64::EPSILON * norm(&x) {
            stag += 1;
        } else {
            stag = 0;
        }

        for ((xi, ri), (pi, qi)) in x.iter_mut().zip(r.iter_mut()).zip(p.iter().zip(&q)) {
            *xi += alpha * *pi;
            *ri -= alpha * *qi;
        }
        let normr = norm(&r);
        normr_act = normr;
        resvec.push(normr);

        if normr <= tolb || stag >= MAXSTAGSTEPS || moresteps > 0 {
            r = residual(b, &ops.apply_a(&x)?);
            normr_act = norm(&r);
            *resvec.last_mut().expect("pushed above") = normr_act;
            if normr_act <= tolb {
                flag = 0;
                iter = it;
                break;
            }
            if stag >= MAXSTAGSTEPS && moresteps == 0 {
                stag = 0;
            }
            moresteps += 1;
            if moresteps as i64 >= maxmsteps {
                tol_unreachable = true;
                flag = 3;
                iter = it;
                break;
            }
        }
        if normr_act < normrmin {
            normrmin = normr_act;
            xmin.clone_from(&x);
            imin = it;
        }
        if stag >= MAXSTAGSTEPS {
            flag = 3;
            break;
        }
    }

    let relres;
    if flag == 0 {
        relres = normr_act / n2b;
    } else {
        let r_comp = norm(&residual(b, &ops.apply_a(&xmin)?));
        if r_comp <= normr_act {
            x = xmin;
            iter = imin;
            relres = r_comp / n2b;
        } else {
            iter = ii;
            relres = normr_act / n2b;
        }
    }

    Ok(PcgOutcome {
        x,
        flag,
        relres,
        iter,
        stopped_at: ii,
        resvec,
        zero_rhs: false,
        tol_unreachable,
    })
}
