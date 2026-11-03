# Implementation brief: FDTD Phase 3 (CPML) — `fdtd_cpml`

**For:** the coding model that implements Phase 3 of `dev/plans/fdtd3d.md`.
**Written against:** `main` at `8d5638f` (workspace version 0.3.50).
**Ships as:** 0.3.51, on a feature branch, as a **draft** PR. Do not merge.
**Status of this brief:** planning only. No implementation code exists yet on any branch.

Read `dev/plans/fdtd3d.md` §§ "Absorber", "PML auxiliary in the curl", "CPML formulation", "Validation" (items 1, 6), the Phase 3 example, and Decision 7 before starting. Where this brief and the plan disagree, this brief records why (section 13) and Michael decides; do not silently pick one.

---

## 0. Ground rules

- Pure Rust, **no new dependencies** (not even dev-dependencies). `rayon` is already a dependency of `rustlab-dsp` but this phase does not use it (see section 13, gate (c)).
- Branch `cursor/<name>-5f25` (or `feature/fdtd-cpml` if Michael says so) from current `main`. Never push to `main`, never force push. If `main` moves, **merge** `main` into the branch, do not rebase.
- One logical change per commit. Suggested commit order is in section 14.
- Version bump to **0.3.51** in the same PR (`Cargo.toml` `[workspace.package] version`, then `cargo build --workspace` to refresh `Cargo.lock`).
- Toolchain: the VM image defaults to rustc 1.83, which cannot parse a locked dependency (`cpufeatures 0.3.0` needs `edition2024`). Run `rustup default stable` (1.99 at the time of writing) before building.
- Do not start Phase 4 (lumped port). Do not add a second Yee kernel (see section 5.5).
- Every builtin change needs `docs/functions.md`, a REPL `HelpEntry`, a `CATEGORIES` row entry, `docs/quickref.md`, `AGENTS.md`, and `CHANGELOG.md` in the same PR (AGENTS.md Workflow Rules 3, 5, 6, 7, 12).

---

## 1. What is being built

```text
fdtd_cpml(sim, n)
fdtd_cpml(sim, n, "sigma_factor", 1, "kappa_max", 1, "alpha_max", 0, "order", 3, "alpha_order", 1)
fdtd_cpml(sim, "zmin", 0)          % turn one face off (that side is a plain PEC wall)
fdtd_cpml(sim, "zmin", n)          % turn it back on (value must equal the current n)
fdtd_cpml(sim, 0)                  % remove the absorber
```

A Roden–Gedney convolutional PML: one auxiliary variable per Cartesian derivative in the curl, inside an `n`-cell slab on each enabled face. The outer boundary stays PEC. Geometry and coefficients are baked only while `steps == 0`.

---

## 2. Conventions you must keep straight

### 2.1 Storage and indices

- Fields are flat `Vec<f64>`, x fastest: `idx = (k * ny + i) * nx + j`. One `(i, k)` row is `nx` contiguous values.
- Script and Rust API indices are `(i, j, k)` = (y **row**, x **column**, z **page**). Rust is 0-based, script is 1-based.
- Grid has `nx × ny × nz` cells; nodes run `0..=N` on each axis; node `0` and node `N` are the PEC walls.

### 2.2 Yee stagger (0-based, positions in cell units)

| Field | x | y | z | Never updated |
|---|---|---|---|---|
| `Ex` | `j + ½` | `i` | `k` | `i = 0` or `k = 0` (min walls); `i = ny`, `k = nz` not stored (read as 0) |
| `Ey` | `j` | `i + ½` | `k` | `j = 0` or `k = 0` |
| `Ez` | `j` | `i` | `k + ½` | `j = 0` or `i = 0` |
| `Hx` | `j` | `i + ½` | `k + ½` | — |
| `Hy` | `j + ½` | `i` | `k + ½` | — |
| `Hz` | `j + ½` | `i + ½` | `k` | — |

So along each axis an E-field derivative is taken **at a node** (integer position) and an H-field derivative is taken **at a half node** (`m + ½`).

### 2.3 Face → axis → index mapping

| Face | Axis | Index it varies | Side | Slab cells, 1-based script indices | E samples in slab, 0-based `m` | H samples in slab, 0-based `m` |
|---|---|---|---|---|---|---|
| `xmin` | x | column `j` | lo | columns `1 .. n` | `0 ≤ m < n` (m = 0 is the wall) | `0 ≤ m < n` |
| `xmax` | x | column `j` | hi | columns `nx−n+1 .. nx` | `nx−n < m ≤ nx−1` | `nx−n ≤ m ≤ nx−1` |
| `ymin` | y | row `i` | lo | rows `1 .. n` | `0 ≤ m < n` | `0 ≤ m < n` |
| `ymax` | y | row `i` | hi | rows `ny−n+1 .. ny` | `ny−n < m ≤ ny−1` | `ny−n ≤ m ≤ ny−1` |
| `zmin` | z | page `k` | lo | pages `1 .. n` | `0 ≤ m < n` | `0 ≤ m < n` |
| `zmax` | z | page `k` | hi | pages `nz−n+1 .. nz` | `nz−n < m ≤ nz−1` | `nz−n ≤ m ≤ nz−1` |

`x` is the **column** axis (`j`), `y` the **row** axis (`i`), `z` the **page** axis (`k`). An `xmin`/`ymin` swap is the most likely silent bug; the face-axis test in section 8.1 (T7) exists to catch it.

Face order everywhere (arrays, `fdtd_info.cpml`, docs): `[xmin, xmax, ymin, ymax, zmin, zmax]`.

Corners and edges need no special code: each axis has its own profile, so a corner cell simply has two or three non-identity profiles.

---

## 3. CPML math (exact)

### 3.1 Profiles

For one axis with spacing `Δ` (that axis's `dx`, `dy` or `dz`), at a sample with local coordinate `ρ ∈ (0, 1]` (0 at the inner interface, 1 at the outer wall):

```text
m      = order            (default 3)
mα     = alpha_order      (default 1)
σ_max  = sigma_factor * (m + 1) * EPS0 * C0 / Δ        (default sigma_factor 1 → 4 ε0 c0 / Δ ≈ 0.01062 / Δ S/m)
σ(ρ)   = σ_max * ρ^m
κ(ρ)   = 1 + (kappa_max - 1) * ρ^m                     (default kappa_max 1)
α(ρ)   = alpha_max * (1 - ρ)^mα                        (default alpha_max 0, in S/m)

b      = exp(-(σ/κ + α) * dt / EPS0)
a      = 0.0                                  if σ == 0.0
a      = σ / (κ * (σ + κ * α)) * (b - 1)      otherwise
kinv   = 1 / κ
```

Use the module constants `EPS0` and `C0` from `fdtd/mod.rs` (`EPS0 = 1/(MU0·C0²)`), not the literal `8.854e-12`.

The `a = 0.0 if σ == 0.0` rule replaces the plan's "`σ = 0 and α = 0`". The two are the same number (`σ = 0, α > 0` makes the formula `0 · (b − 1) = −0.0`), but `σ = 0, α = 0` makes the formula `0/0 = NaN`, and writing a `+0.0` literal keeps the identity exact (section 5.3). Mention this in the plan update.

`a ≤ 0` always, because `b ≤ 1`. **Do not flip the sign of `a`.** The update in section 5.2 is written for this sign.

Samples **outside** the slab (and every sample on a disabled face's side) are the identity, written as literals, not computed: `kinv = 1.0`, `b = 1.0`, `a = 0.0`. In particular do **not** evaluate `α(ρ = 0) = alpha_max`, which is nonzero: membership is decided by index (table 2.3), never by `ρ > 0`.

`powf` notes: `0.0_f64.powf(0.0) == 1.0` in Rust, so `alpha_order = 0` gives `α = alpha_max` at the outer wall too; that is what the formula says. `order = 0` gives a constant (box) profile. Both are allowed.

### 3.2 Sampling `ρ` (the half-cell offsets)

The plan says compute `ρ` from positions in metres. With uniform spacing on an axis with `N` cells, `n_lo` cells on the min face and `n_hi` on the max face:

| Sample | Position | lo side (`m < n_lo`) | hi side |
|---|---|---|---|
| E (node, "primal") | `x = m·Δ` | `ρ = (n_lo·Δ − m·Δ) / (n_lo·Δ)` | `m > N − n_hi`: `ρ = (m·Δ − (N − n_hi)·Δ) / (n_hi·Δ)` |
| H (half node) | `x = (m + ½)·Δ` | `ρ = (n_lo·Δ − (m + ½)·Δ) / (n_lo·Δ)` | `m ≥ N − n_hi`: `ρ = ((m + ½)·Δ − (N − n_hi)·Δ) / (n_hi·Δ)` |

In cell units these are `(n_lo − m)/n_lo`, `(m − (N − n_hi))/n_hi`, `(n_lo − m − ½)/n_lo`, `(m + ½ − (N − n_hi))/n_hi`. These match `rustlab_em` `lessons/_shared/em.rlab` (`pml_sigma_profile` primal `x_l = (npml − ii + 1)/npml`, `x_r = (ii − (N − npml))/npml`; `pml_sigma_profile_h` with `+ 0.5`), translated from its 1-based `ii` to 0-based `m`.

Worked numbers (use these in test T1), `N = 40`, `n_lo = n_hi = 8`:

| Sample | m | ρ |
|---|---|---|
| E lo | 0 | 1 (the wall node; computed, never used) |
| E lo | 1 | 7/8 |
| E lo | 7 | 1/8 |
| E lo | 8 | identity |
| E hi | 32 | identity |
| E hi | 33 | 1/8 |
| E hi | 39 | 7/8 (node 40 is the wall, not stored) |
| H lo | 0 | 7.5/8 |
| H lo | 7 | 0.5/8 |
| H lo | 8 | identity |
| H hi | 31 | identity |
| H hi | 32 | 0.5/8 |
| H hi | 39 | 7.5/8 |

So each face has `n − 1` updated E samples (`ρ = 1/n … (n−1)/n`) plus the wall, and `n` H samples (`ρ = ½/n … (n−½)/n`). An E derivative never sees `ρ = 1`; that is correct (the plan's "outermost cell has `ρ = 1`" is the wall node).

Slabs cannot overlap when `n_lo + n_hi < N` (section 6.2 enforces it). With that rule, lo E samples are `m ≤ n_lo − 1` and hi E samples are `m ≥ N − n_hi + 1 ≥ n_lo + 2`; lo H `m ≤ n_lo − 1`, hi H `m ≥ N − n_hi ≥ n_lo + 1`.

### 3.3 The auxiliary, stored negated and scaled by Δ

Roden–Gedney, for a derivative along an axis of a field `u` with the two-point Yee difference `du`:

```text
ψ   := b·ψ + a·du/Δ
∂u  ≈  du/(κΔ) + ψ
```

**Store `q = −Δ·ψ` instead of `ψ`.** Then:

```text
q    := b·q − a·du          (a ≤ 0, so −a·du has the sign of du)
Δ·∂u ≈  kinv·du − q
```

and the existing curl coefficient (`c.x = dt/(μ0 dx)` for H, `dt/(ε0 dx)` for E, which already contains `1/Δ`) multiplies `(kinv·du − q)` exactly where it multiplies `du` today.

Why negated and Δ-scaled (both matter for the bit-exact gate, section 5.3):

- Δ-scaled: the term in the curl is `c.x * (kinv*du − q)` instead of `c.x * (du/Δ ...)`, so with `kinv = 1.0` and `q = +0.0` it reduces to `c.x * du`, today's expression.
- Negated: `x − (+0.0) == x` bit for bit for **every** `x` including `−0.0`, whereas `x + (+0.0)` turns `−0.0` into `+0.0`. Fields do become `−0.0`: a hard source writes `E = −g/(N·Δ)`, and `g` is exactly `0.0` once a Gaussian underflows. The golden test has a hard source. With `+ψ` the sigma-factor-0 identity can fail on a signed zero.
- `q` stays `+0.0` in the identity region: `1.0 * (+0.0) − (0.0 * du)` is `+0.0 − (±0.0) = +0.0`.

Write the code comment for this as a constraint ("`q` is `−Δψ` so that `kinv*du − q` is exactly `du` when `kinv = 1, q = +0`, signed zero included"), not as history.

Do **not** use `mul_add` anywhere in the kernel. The existing kernel uses separate multiply and add; a fused multiply-add changes bits.

---

## 4. The twelve auxiliaries

Index constants (put them in `cpml.rs`): H set `HX_Y = 0, HX_Z = 1, HY_Z = 2, HY_X = 3, HZ_X = 4, HZ_Y = 5`; E set `EX_Y = 0, EX_Z = 1, EY_Z = 2, EY_X = 3, EZ_X = 4, EZ_Y = 5`. Store as `q_h: [Vec<f64>; 6]` and `q_e: [Vec<f64>; 6]`, each x-fastest with the field's `idx`.

`du` is exactly the difference that appears in today's kernel (`yee.rs`); "row scalar" means read once per `(i, k)` row outside the `j` loop; "per j" means indexed inside it.

| # | Aux | Updates | Derivative | `du` (as in `yee.rs` today) | Profile used (sample, axis, index) | Term in curl |
|---|---|---|---|---|---|---|
| 1 | `q_h[HX_Y]` | Hx | ∂Ez/∂y | `ez_ip[j] − ez_r[j]` (`ez_ip` = zero row at `i = ny−1`) | H, y, `i` (row scalar) | `+ c.y * (ky*du − q)` |
| 2 | `q_h[HX_Z]` | Hx | ∂Ey/∂z | `ey_kp[j] − ey_r[j]` (zero row at `k = nz−1`) | H, z, `k` (row scalar) | `− c.z * (kz*du − q)` |
| 3 | `q_h[HY_Z]` | Hy | ∂Ex/∂z | `ex_kp[j] − ex_r[j]` | H, z, `k` (row scalar) | `+ c.z * (kz*du − q)` |
| 4 | `q_h[HY_X]` | Hy | ∂Ez/∂x | `ez_r[j+1] − ez_r[j]`; last column `0.0 − ez_r[j]` | H, x, `j` (per j) | `− c.x * (kx[j]*du − q)` |
| 5 | `q_h[HZ_X]` | Hz | ∂Ey/∂x | `ey_r[j+1] − ey_r[j]`; last column `0.0 − ey_r[j]` | H, x, `j` (per j) | `+ c.x * (kx[j]*du − q)` |
| 6 | `q_h[HZ_Y]` | Hz | ∂Ex/∂y | `ex_ip[j] − ex_r[j]` | H, y, `i` (row scalar) | `− c.y * (ky*du − q)` |
| 7 | `q_e[EX_Y]` | Ex | ∂Hz/∂y | `hz_r[j] − hz_im[j]` (rows `i ≥ 1, k ≥ 1`) | E, y, `i` (row scalar) | `+ c.y * (ky*du − q)` |
| 8 | `q_e[EX_Z]` | Ex | ∂Hy/∂z | `hy_r[j] − hy_km[j]` | E, z, `k` (row scalar) | `− c.z * (kz*du − q)` |
| 9 | `q_e[EY_Z]` | Ey | ∂Hx/∂z | `hx_r[j] − hx_km[j]` (rows `k ≥ 1`, `j ≥ 1`) | E, z, `k` (row scalar) | `+ c.z * (kz*du − q)` |
| 10 | `q_e[EY_X]` | Ey | ∂Hz/∂x | `hz_r[j] − hz_r[j−1]` | E, x, `j` (per j) | `− c.x * (kx[j]*du − q)` |
| 11 | `q_e[EZ_X]` | Ez | ∂Hy/∂x | `hy_r[j] − hy_r[j−1]` (rows `i ≥ 1`, `j ≥ 1`) | E, x, `j` (per j) | `+ c.x * (kx[j]*du − q)` |
| 12 | `q_e[EZ_Y]` | Ez | ∂Hx/∂y | `hx_r[j] − hx_im[j]` | E, y, `i` (row scalar) | `− c.y * (ky*du − q)` |

Each `q` is updated first with that same `du` and the same profile entry's `b`, `a`: `q_new = b*q_old − a*du`, stored back, and `q_new` (not `q_old`) goes into the term. Check the sampling against the stagger table: Hx sits at `(i + ½, k + ½)`, so its y and z derivatives use the **H (half-node)** profiles at `i` and `k`; Ex sits at node `i, k`, so its derivatives use the **E (node)** profiles at `i` and `k`. All six H aux use H profiles; all six E aux use E profiles. No other mixing.

H is updated before E within a step, so `du` for H aux uses `E^n` and `du` for E aux uses `H^{n+½}`. The existing loop order already guarantees this.

---

## 5. Kernel integration (`crates/rustlab-dsp/src/fdtd/yee.rs`)

### 5.1 Closures become `FnMut`

`step_e` and `step_h` take `curl: impl Fn(usize) -> f64` today. Change both to `mut curl: impl FnMut(usize) -> f64`. The loops call `curl(j)` exactly once per `j` in every arm; keep it that way (the closure now mutates `q`, so a second call would double-update it). No other change to `step_e` / `step_h`; the material variants (`ERow::{Vacuum, Lossless, Lossy}`, `HRow::{Vacuum, Scaled}`) stay as they are.

### 5.2 New arguments and the H update

Add to `yee.rs`:

```rust
/// One axis of CPML coefficients, one entry per sample along that axis.
/// Identity entries are exactly `kinv = 1.0`, `b = 1.0`, `a = 0.0`.
pub(super) struct Profile {
    pub kinv: Vec<f64>,
    pub b: Vec<f64>,
    pub a: Vec<f64>,
}

#[derive(Clone, Copy)]
pub(super) struct Axes<'a> {
    pub x: &'a Profile,
    pub y: &'a Profile,
    pub z: &'a Profile,
}

/// `q := b q − a du`, returning the new `q`. `q` is `−Δψ` (Roden–Gedney ψ).
#[inline(always)]
fn aux(q: &mut f64, b: f64, a: f64, du: f64) -> f64 {
    let s = b * *q - a * du;
    *q = s;
    s
}
```

(`Profile` may live in `cpml.rs` instead and be imported; either is fine. Keep `aux` in `yee.rs`.)

New signature:

```rust
pub(super) fn update_h(
    d: Dims, c: Coeffs, m: HCoefs, p: Axes, q: &mut [Vec<f64>; 6],
    zero: &[f64], ex: &[f64], ey: &[f64], ez: &[f64],
    hx: &mut [f64], hy: &mut [f64], hz: &mut [f64],
)
```

Body sketch (keep every existing line; add only what is marked):

```rust
let nx = d.nx;
let zero = &zero[..nx];
let full = q[0].len() == nx * d.ny * d.nz;          // NEW: false → one scratch row
let (kx, bx, ax) = (&p.x.kinv[..nx], &p.x.b[..nx], &p.x.a[..nx]);   // NEW
let [q_hx_y, q_hx_z, q_hy_z, q_hy_x, q_hz_x, q_hz_y] = q;           // NEW
for k in 0..d.nz {
    let (kz, bz, az) = (p.z.kinv[k], p.z.b[k], p.z.a[k]);          // NEW
    for i in 0..d.ny {
        let (ky, by, ay) = (p.y.kinv[i], p.y.b[i], p.y.a[i]);      // NEW
        let r = d.row(i, k);
        let pr = if full { r } else { 0 };                          // NEW
        // ... existing ex_r, ey_r, ez_r, ex_ip, ez_ip, ex_kp, ey_kp, hx_r, hy_r, hz_r, mx, my, mz ...

        let (q1, q2) = (&mut q_hx_y[pr..pr + nx], &mut q_hx_z[pr..pr + nx]);
        step_h(hx_r, 0, nx, mx, |j| {
            let du_y = ez_ip[j] - ez_r[j];
            let du_z = ey_kp[j] - ey_r[j];
            let sy = aux(&mut q1[j], by, ay, du_y);
            let sz = aux(&mut q2[j], bz, az, du_z);
            c.y * (ky * du_y - sy) - c.z * (kz * du_z - sz)
        });

        let (q3, q4) = (&mut q_hy_z[pr..pr + nx], &mut q_hy_x[pr..pr + nx]);
        step_h(hy_r, 0, nx - 1, my, |j| {
            let du_z = ex_kp[j] - ex_r[j];
            let du_x = ez_r[j + 1] - ez_r[j];
            let sz = aux(&mut q3[j], bz, az, du_z);
            let sx = aux(&mut q4[j], bx[j], ax[j], du_x);
            c.z * (kz * du_z - sz) - c.x * (kx[j] * du_x - sx)
        });
        let (q5, q6) = (&mut q_hz_x[pr..pr + nx], &mut q_hz_y[pr..pr + nx]);
        step_h(hz_r, 0, nx - 1, mz, |j| {
            let du_x = ey_r[j + 1] - ey_r[j];
            let du_y = ex_ip[j] - ex_r[j];
            let sx = aux(&mut q5[j], bx[j], ax[j], du_x);
            let sy = aux(&mut q6[j], by, ay, du_y);
            c.x * (kx[j] * du_x - sx) - c.y * (ky * du_y - sy)
        });
        let last = nx - 1;
        step_h(hy_r, last, nx, my, |j| {
            let du_z = ex_kp[j] - ex_r[j];
            let du_x = 0.0 - ez_r[j];
            let sz = aux(&mut q3[j], bz, az, du_z);
            let sx = aux(&mut q4[j], bx[j], ax[j], du_x);
            c.z * (kz * du_z - sz) - c.x * (kx[j] * du_x - sx)
        });
        step_h(hz_r, last, nx, mz, |j| {
            let du_x = 0.0 - ey_r[j];
            let du_y = ex_ip[j] - ex_r[j];
            let sx = aux(&mut q5[j], bx[j], ax[j], du_x);
            let sy = aux(&mut q6[j], by, ay, du_y);
            c.x * (kx[j] * du_x - sx) - c.y * (ky * du_y - sy)
        });
    }
}
```

The two last-column closures are **not optional**: without them the `j = nx − 1` H column (inside the `xmax` slab) never gets its x auxiliary. The `q3`..`q6` slices are borrowed again by the second pair of closures after the first pair has been consumed by `step_h`; that compiles with edition-2021 closure captures. If the borrow checker objects, reslice them.

The term expressions keep **the same operand order and the same subtraction order as today** (`c.y * (...) - c.z * (...)`). Only `du` is replaced by `(k* * du - s)`.

### 5.3 The E update

Same pattern, `update_e(d, c, m: ECoefs, p: Axes, q: &mut [Vec<f64>; 6], hx, hy, hz, ex, ey, ez)`, `let [q_ex_y, q_ex_z, q_ey_z, q_ey_x, q_ez_x, q_ez_y] = q;`, row scalars `(ky, by, ay)` from `p.y[i]`, `(kz, bz, az)` from `p.z[k]`, per-j `kx/bx/ax` from `p.x`:

```rust
// Ex, rows i >= 1 && k >= 1, from j = 0
|j| { let du_y = hz_r[j] - hz_im[j]; let du_z = hy_r[j] - hy_km[j];
      let sy = aux(&mut q7[j], by, ay, du_y); let sz = aux(&mut q8[j], bz, az, du_z);
      c.y * (ky * du_y - sy) - c.z * (kz * du_z - sz) }
// Ey, rows k >= 1, from j = 1
|j| { let du_z = hx_r[j] - hx_km[j]; let du_x = hz_r[j] - hz_r[j - 1];
      let sz = aux(&mut q9[j], bz, az, du_z); let sx = aux(&mut q10[j], bx[j], ax[j], du_x);
      c.z * (kz * du_z - sz) - c.x * (kx[j] * du_x - sx) }
// Ez, rows i >= 1, from j = 1
|j| { let du_x = hy_r[j] - hy_r[j - 1]; let du_y = hx_r[j] - hx_im[j];
      let sx = aux(&mut q11[j], bx[j], ax[j], du_x); let sy = aux(&mut q12[j], by, ay, du_y);
      c.x * (kx[j] * du_x - sx) - c.y * (ky * du_y - sy) }
```

`q` for E uses the **E** profiles (`Absorber.e`), for H the **H** profiles (`Absorber.h`). Passing the wrong set compiles and runs; T1 and T5 catch it.

### 5.4 Why this is bit-identical when CPML does nothing

In the identity region: `kinv = 1.0` so `kinv*du == du` bit for bit (including `−0.0`); `q` is `+0.0` and stays `+0.0` (section 3.3); `du − (+0.0) == du` bit for bit. So `c.y * (ky*du_y − sy) − c.z * (kz*du_z − sz)` has the same bits as `c.y * du_y − c.z * du_z`. This covers both no-CPML grids (scratch rows) and `sigma_factor = 0, kappa_max = 1` grids (full arrays, every profile entry identity because `σ = 0 → a = 0.0`, `κ = 1 + 0·ρ^m = 1.0`, `b = exp(−(0 + α)·dt/ε0)` is irrelevant while `a = 0` and `q = 0`).

### 5.5 One kernel, scratch rows when CPML is off

**Superseded (2026-11-03).** Michael approved the Taflove split after the always-ψ kernel left CPML-off vacuum at 184 Mcell/s (52% of 0.3.50). The shipped stepper is the Phase 2 curl on every cell, then a slab-only correction. See `dev/plans/fdtd3d.md` (Phase 3 status). The paragraphs below are the original brief and are not what landed.

The plan requires **one** kernel with `κ` and `ψ` always present. So:

- No CPML (never called, `n = 0`, or every face disabled): `Absorber` holds identity profiles (lengths `nx`, `ny`, `nz`) and twelve **scratch rows of length `nx`** (`q[0].len() == nx`, `full == false`, every row uses offset 0). They stay `+0.0` forever.
- CPML with any face `> 0` (including `sigma_factor = 0`): twelve **full** arrays of `nx·ny·nz` (plan § CPML formulation: full arrays in v1; slab-packing is a later optimisation, do not do it now).

Do **not** add a non-ψ fast path for interior rows or for CPML-off grids, even though section 5.4 shows it would be bit-identical. If the CPML-off slowdown (section 9) is large, report it with numbers and list the fast path as an option for Michael; he decides.

Phase 6 note for the code comment on `Absorber`: the scratch rows are shared by every row, which is fine serially; a row-parallel loop needs per-thread scratch or full arrays.

---

## 6. File-by-file changes

### 6.1 New: `crates/rustlab-dsp/src/fdtd/cpml.rs`

```rust
use super::{Axis, DspError, C0, EPS0};      // whatever the module already exposes
use super::yee::Profile;                     // or define Profile here and import it in yee.rs

/// The six faces, in the order used by `faces` arrays and `fdtd_info.cpml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face { XMin, XMax, YMin, YMax, ZMin, ZMax }

impl Face {
    pub const ALL: [Face; 6] = [...];
    pub fn parse(s: &str) -> Option<Face>;   // case-insensitive "xmin" .. "zmax"
    pub fn name(self) -> &'static str;
    pub fn index(self) -> usize;             // 0..6 in ALL order
    pub fn axis(self) -> Axis;               // XMin/XMax → Axis::X, ...
    pub fn is_max(self) -> bool;
}

/// User knobs. `Default` is the plan's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CpmlParams {
    pub sigma_factor: f64,   // 1.0, finite, >= 0
    pub kappa_max: f64,      // 1.0, finite, >= 1
    pub alpha_max: f64,      // 0.0, finite, >= 0, S/m
    pub order: f64,          // 3.0, finite, >= 0
    pub alpha_order: f64,    // 1.0, finite, >= 0
}
impl Default for CpmlParams { ... }
impl CpmlParams { pub fn validate(&self) -> Result<(), DspError>; }

/// What the user asked for. `faces[f]` is 0 (off) or `n`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CpmlSettings { pub n: usize, pub faces: [usize; 6], pub params: CpmlParams }

/// Baked coefficients and auxiliaries. Always present on a grid.
pub(super) struct Absorber {
    pub e: [Profile; 3],          // x, y, z; node samples; lengths nx, ny, nz
    pub h: [Profile; 3],          // x, y, z; half-node samples
    pub q_e: [Vec<f64>; 6],       // EX_Y .. EZ_Y, full grid or one row of nx
    pub q_h: [Vec<f64>; 6],       // HX_Y .. HZ_Y
}

impl Profile {
    pub(super) fn identity(len: usize) -> Profile;
    /// `len` samples along an axis of `len` cells. `half` selects H sampling.
    pub(super) fn bake(len: usize, n_lo: usize, n_hi: usize, delta: f64, dt: f64,
                       p: &CpmlParams, half: bool) -> Profile;
}

impl Absorber {
    pub(super) fn off(nx: usize, ny: usize, nz: usize) -> Absorber;     // identity + scratch rows
    pub(super) fn bake(nx, ny, nz, (dx, dy, dz), dt, s: &CpmlSettings) -> Result<Absorber, DspError>;
    /// Bytes of the full auxiliaries and the profiles. 0 when off (scratch rows are
    /// a fixed overhead like `zero_row`, which `bytes()` does not count either).
    pub(super) fn bytes(&self, active: bool) -> usize;
    pub(super) fn is_full(&self) -> bool;
}
```

`Profile::bake` loops `m in 0..len`: decide lo membership `m < n_lo`, hi membership by section 3.2 (E: `m > len − n_hi` when `n_hi > 0`; H: `m >= len − n_hi` when `n_hi > 0`), compute `ρ` in metres, then section 3.1; else identity literals. Guard the hi test so `n_hi = 0` never matches (`len − 0 = len`, and `m >= len` is false for H; `m > len` is false for E; it already works, but write it explicitly).

`Absorber::bake` computes `n_lo/n_hi` per axis from `faces`, bakes six profiles, and allocates `q` full when any face is `> 0`, else scratch rows. Allocate full arrays through `checked_elems(&[nx, ny, nz, 12], ELEM_F64)` first (same pattern as `FdtdSim::new`), so an absurd grid errors instead of aborting. After baking, assert every `b` is in `(0, 1]`, every `a` is finite and `<= 0`, every `kinv` in `(0, 1]` (return `DspError::InvalidParameter` otherwise; with validated params this cannot fire, but it keeps a bad formula edit from producing NaNs silently).

### 6.2 `crates/rustlab-dsp/src/fdtd/mod.rs`

- `mod cpml;` and `pub use cpml::{CpmlParams, Face};`.
- `FdtdSim` new fields: `cpml: Option<cpml::CpmlSettings>`, `absorber: cpml::Absorber`. In `new`, `absorber: Absorber::off(nx, ny, nz)`.
- New public methods (all call `self.require_setup("CPML")?` first):

```rust
/// Absorber on every face with thickness `n` cells. `n = 0` removes it.
/// Replaces any earlier call (faces turned off before are on again).
pub fn set_cpml(&mut self, n: usize, params: CpmlParams) -> Result<(), DspError>;
/// Turn one face off (`n = 0`, that side is a plain PEC wall) or back on
/// (`n` must equal the current thickness). Needs an earlier `set_cpml`.
pub fn set_cpml_face(&mut self, face: Face, n: usize) -> Result<(), DspError>;
/// Thickness per face, `[xmin, xmax, ymin, ymax, zmin, zmax]`; zeros when off.
pub fn cpml_faces(&self) -> [usize; 6];
```

- Validation in `set_cpml` / `set_cpml_face` (errors are `DspError::InvalidParameter` with these messages, more or less verbatim):
  - `params.validate()`: `"sigma_factor must be finite and >= 0, got {v}"`, `"kappa_max must be finite and >= 1, got {v}"`, `"alpha_max must be finite and >= 0, got {v}"`, `"order must be finite and >= 0, got {v}"`, `"alpha_order must be finite and >= 0, got {v}"`.
  - Per axis, `n_lo + n_hi < N`: `"CPML of {n_lo} + {n_hi} cells leaves no interior on {axis} ({axis}: {N} cells); need fewer than {N} in total"`.
  - `set_cpml_face` with no earlier `set_cpml` (or after `set_cpml(0)`): `"call fdtd_cpml(sim, n) before turning a face on or off"`.
  - `set_cpml_face(face, v)` with `v != 0 && v != n`: `"a face takes 0 (off) or the absorber thickness {n}, got {v}"`.
  - Validate everything **before** mutating, so a failed call leaves the grid as it was (test T9 checks this).
- `set_cpml(0, _)` sets `cpml = None` and `absorber = Absorber::off(...)`.
- Private `fn rebake_cpml(&mut self) -> Result<(), DspError>` rebuilds `absorber` from `cpml` with the **current** `dt` (zeroing every `q`). Call it from `set_cpml`, `set_cpml_face`, **and at the end of `set_media`** (which changes `dt`; `b` depends on `dt`). `set_media` already returns `Result`; propagate. Forgetting the `set_media` call is pitfall P6; test T8 catches it.
- `bytes()`: add `+ self.absorber.bytes(self.cpml_active())` where active means some face `> 0`. Full arrays: `12 · ncell · 8`; profiles: `2 · 3 · (nx + ny + nz) · 8`. A grid without CPML reports exactly what it reports today (no behaviour change for existing scripts).
- `step()`: build `yee::Axes` for E and H from `self.absorber.e` / `.h` and pass `&mut self.absorber.q_h` to `update_h`, `&mut self.absorber.q_e` to `update_e`. Borrow-wise, `absorber` is a separate field from the six field vectors, so `&self.absorber.h[..]` together with `&mut self.absorber.q_h` needs a split borrow; destructure `let cpml::Absorber { e, h, q_e, q_h } = &mut self.absorber;` before the calls.
- Doc comment on `FdtdSim` (top of file): one line saying the outer walls are PEC and `set_cpml` adds an absorbing layer inside them.

### 6.3 `crates/rustlab-dsp/src/fdtd/yee.rs`

Section 5. Also update the module doc comment: one paragraph that every derivative goes through `kinv*du − q` (CPML), that a grid without CPML runs that same arithmetic with identity coefficients and one scratch row, and the signed-zero reason for `q = −Δψ` (the constraint, not the history).

### 6.4 `crates/rustlab-dsp/src/fdtd/tests.rs`

Tests T1–T9 (section 8.1). `bytes_count_the_coefficients` gets two more assertions (section 8.1, T9), its existing lines stay.

### 6.5 `crates/rustlab-dsp/examples/bench_fdtd.rs`

Section 9.

### 6.6 `crates/rustlab-script/src/eval/fdtd.rs`

```rust
/// `fdtd_cpml(sim, n [, key, value]...)` / `fdtd_cpml(sim, face, n)`
pub(crate) fn builtin_fdtd_cpml(args: Vec<Value>) -> Result<Value, ScriptError>
```

- `args.len() < 2` → `ScriptError::arg_count_range(NAME, 2, 12, len)`.
- `sim_arg`, `lock`, `setup_check(NAME, "CPML", &g)` (message: "set CPML before the first step (N steps done); build a new fdtd3d").
- If `args[1]` is a `Value::Str`: face form. Require exactly 3 args (`"expected fdtd_cpml(sim, face, n)"`), `Face::parse` (error `unknown face "{s}" (expected xmin, xmax, ymin, ymax, zmin or zmax)`), `n` via a new `nonneg_int(NAME, "n", v)` helper (same as `positive_int` but allows 0; reject non-integers, negatives, NaN), then `g.set_cpml_face(face, n)`.
- Otherwise: `n = nonneg_int(...)`; remaining args must come in pairs (`"options come in name, value pairs"`); keys via `str_arg`, matched case-insensitively against the five names; unknown key → `unknown option "{key}" (expected sigma_factor, kappa_max, alpha_max, order or alpha_order)`; a repeated key → `option "{key}" given twice`; values via `real_scalar` (the DSP layer checks ranges). Start from `CpmlParams::default()`. Then `g.set_cpml(n, params)`.
- Map `DspError` with `dsp_err(NAME, e)`. Return `Value::None` (like `fdtd_media`).
- `builtin_fdtd_info`: add field `"cpml"` = a 1×6 real row of `g.cpml_faces()` as `f64` (build it the same way `record_fields` builds the `1 × steps` `t` row). Keep `"bytes"`.

### 6.7 `crates/rustlab-script/src/eval/builtins.rs`

`r.register("fdtd_cpml", super::fdtd::builtin_fdtd_cpml);` next to the other `fdtd_*` registrations (around line 175–184). If there is a purity / `IMPURE_BUILTINS` / cache denylist that lists `fdtd_media` or `fdtd_pec`, add `fdtd_cpml` to the same list (grep for `"fdtd_media"` across `crates/rustlab-script/src`).

### 6.8 `crates/rustlab-cli/src/commands/repl.rs`

- `HelpEntry` after `fdtd_pec` (~line 621–642):

```text
name:  "fdtd_cpml"
brief: "Convolutional PML absorber on the faces of an fdtd3d grid"
detail: usage lines from section 1; n cells on every enabled face; options and defaults
        (sigma_factor 1 → σ_max = (order+1)·ε0·c0/Δ, kappa_max 1, alpha_max 0 S/m,
        order 3, alpha_order 1); face names; a disabled face is a PEC wall; slabs must
        leave interior cells (n_lo + n_hi < n on each axis); only before the first step;
        fdtd_info(sim).cpml lists the faces and .bytes includes the auxiliaries;
        example: fdtd_cpml(sim, 8); fdtd_cpml(sim, "zmin", 0)
```

- Update the `fdtd_info` `HelpEntry` detail: mention `cpml` (1×6, `[xmin xmax ymin ymax zmin zmax]`) and that `bytes` includes CPML arrays.
- Add `"fdtd_cpml"` to the existing FDTD `CategoryRow` (~line 1464–1466), after `"fdtd_pec"`. The `help_coverage_tests` fail otherwise.

### 6.9 Examples

New `examples/pde/fdtd_pml.rlab` and `examples/notebooks/fdtd_pml.md` (section 10), plus `gallery/fdtd_pml.md` and `gallery/plots/fdtd_pml/*.svg` regenerated by `bash dev/build-notebooks.sh` (commit only the `.md` and `.svg`, as for `fdtd_cavity`).

### 6.10 Docs, plan, version

Section 11.

---

## 7. Behaviour rules the implementation must have

1. Only at `steps == 0` (`require_setup("CPML")`). Order relative to `fdtd_media`, `fdtd_pec`, `fdtd_source`, `fdtd_probe` does not matter (T8 proves it for media).
2. `fdtd_cpml(sim, n)` again replaces settings and turns every face back on.
3. Sources and probes inside the slab are allowed. PEC (`fdtd_pec`) inside the slab is allowed and still wins (its edges have `Cb = 0`).
4. Materials: the CPML coefficients use vacuum `ε0` and the axis `Δ` regardless of `eps_r` in the slab (Roden–Gedney "arbitrary media" form; the material `Cb` / `Db` multiply the whole curl including the `q` term, which is what folding `q` into the curl does). σ_max is not scaled by `sqrt(eps_r)`; note that as a v1 limitation in the docs.
5. `dt` is never changed by `fdtd_cpml`.
6. A disabled face's side uses identity coefficients right up to the PEC wall.

---

## 8. Tests

Timing budget: the workspace CI runs `cargo test --workspace` in **debug** only (`.github/workflows/cargo-test.yml`). The current kernel runs about **4 Mcell/s in debug** on this VM (`cavity_tm110_matches_yee_dispersion`: 192 cells × 20,000 steps in 1.07 s). Keep each default test under about 15 s debug; put full-size measurements behind `#[ignore]` and run them with `--release`.

Shared helper for the tests (put in `tests.rs`): the zero-mean derivative-of-Gaussian current that `examples/pde/fdtd_hello.rlab` uses, so no static charge is left behind (a plain Gaussian soft source leaves charge and a static field, which would show up as a fake late-time "residual"):

```rust
/// Sample s (1-based) drives step s: w(s·dt) = −((t − t0)/τ)·exp(−((t − t0)/τ)²).
fn dgauss(nsteps: usize, dt: f64, tau_steps: f64) -> Vec<f64>   // t0 = 4τ
```

Steps per cell for cubic cells at Courant 0.99: `1 / (0.99/√3) = 1.7495`. Use this in the sizing formulas below.

### 8.1 `crates/rustlab-dsp/src/fdtd/tests.rs`

**T1 `cpml_profile_samples_match_the_formulas`** (pure, no stepping). `Profile::bake(40, 8, 8, 1e-3, dt, &params, half)` for E and H with `params = { sigma_factor: 1, kappa_max: 2, alpha_max: 0.05, order: 3, alpha_order: 1 }` (non-default κ and α so all three are exercised). For each `m`, recompute `ρ` from table 3.2 independently in the test (cell units) and the expected `kinv, b, a` from section 3.1; compare with relative tolerance `4 * f64::EPSILON` (ρ in metres vs cells can differ by an ulp). Spot-check the table rows of section 3.2. Identity entries: assert `to_bits()` equals `1.0f64.to_bits()` / `0.0f64.to_bits()` exactly. Assert `a <= 0`, `0 < b <= 1` everywhere. A second case `Profile::bake(12, 4, 0, ...)` checks `n_hi = 0` gives identity on the hi side and `n_lo = 4` the lo side.

**T2 `cpml_sigma_zero_matches_phase1_bit_for_bit`** (gate a). `let mut s = golden_sim(); s.set_cpml(3, CpmlParams { sigma_factor: 0.0, ..Default::default() }).unwrap(); assert_eq!(golden_hash(s), PHASE1_GOLDEN);` (golden grid 10×9×8; `n = 3` is the largest that fits `2n < 8`). Same again with `alpha_max: 0.05` added (α without σ is still identity, because `a = 0`). Same again with `golden_sim()` + `set_media` lossy + `mu_r` + an interior `add_pec_volume`, compared against the same grid without CPML (hash equality between the two, not against `PHASE1_GOLDEN`). The existing `vacuum_matches_phase1_bit_for_bit` and `unit_media_and_wall_pec_match_vacuum_bit_for_bit` must pass **unchanged** (they exercise the scratch-row path).

**T3 `cpml_defaults_change_the_fields`** (wiring sanity). `golden_sim()` with `set_cpml(3, default)` after 300 steps: hash differs from `PHASE1_GOLDEN`, and `energy()` is lower than the no-CPML run's.

**T4 `cpml_box_drains_while_pec_box_keeps_energy`** (sign check, cheap). 20×20×20, 1 mm, soft `dgauss` Ez at `(10, 10, 10)`, `τ = 6` steps, 300 steps. PEC run: after the source ends (`> 8τ`), energy stays within 1% of its value at step `8τ + 10` (lossless). CPML run, `set_cpml(6, default)`: `energy(300) / max_t energy(t) < 1e-4`. A flipped `a` sign or a missing aux makes this grow or stall. (Record energy every step with a loop of `run(1, ...)`.)

**T5 `cpml_reflection_below_40_db`** (gate b, default size, debug-friendly). Reference-subtraction method, documented in the test's doc comment:

> The reflection is `test − reference`, where `reference` is the same source and probe in a PEC box large enough that no wall echo reaches the probe inside the window. The interior update is the same arithmetic in both grids (same `dx`, so the same `dt`), so the difference is exactly what the absorber sends back, plus whatever comes back from the PEC wall behind it.

Setup:

| | Test grid | Reference grid |
|---|---|---|
| Size | `N_t = 28` cubic, 1 mm | `L = 76` cubic, 1 mm, no CPML |
| CPML | `set_cpml(8, default)` | none |
| Source | soft Ez at `(14, 14, 14)` | soft Ez at `(38, 38, 38)` |
| Waveform | `dgauss(W, dt, 8.0)` (τ = 8 steps, t0 = 32 steps) | same |
| Probe | Ez at `(14, 17, 14)`: on the +x axis, 3 cells inside the `xmax` interface (E node 20) | Ez at `(38, 41, 38)` |
| Window | `W = 128` steps | same |

Sizing (put this in the doc comment so the next person can resize): with `t0 = 4τ`, the window must hold the source pulse plus the echo from the far outer wall of the **test** grid, `W ≥ 8τ + 1.75·(N_t + p) + 10` with `p = 3` the probe offset (= 64 + 54 + 10 = 128). The **reference** wall echo first reaches the probe along the +x wall, path `2·(L/2) − p` cells; its leading edge (3τ before the pulse centre) must arrive after `W`: `t0 − 3τ + 1.75·(L − p) > W` → `L ≥ 76` for these numbers (echo leading edge at step ≈ 135.7). Cost ≈ 76³ × 128 ≈ 5.6e7 cell updates, about 10–15 s in debug. If it is slower than 20 s, drop `τ` to 6 steps and recompute `W` and `L` from the two inequalities; do not shrink `L` below the inequality.

Metric: `R = 20·log10( max_n |test_n − ref_n| / max_n |ref_n| )` over `n = 1..=W`. **Assert `R < −40.0`.** Print `R` and also the late-time value over `n ≥ 8τ + 1.75·p + 5` (after the direct pulse has passed). Sanity in the same test: the test grid **without** CPML (PEC walls at 28³) gives `R > −20.0` (shows the method sees a reflection). Expect roughly −60 to −80 dB for CPML; whatever it is, report it.

**T5b `cpml_reflection_hello_geometry`**, `#[ignore = "release-size reference run: cargo test --release -p rustlab-dsp -- --ignored cpml_reflection"]`. The plan's geometry: `N_t = 40`, `n = 8`, source `(20, 20, 20)`, probe 6 cells toward +x `(20, 26, 20)` (6 cells inside the interface at node 32), `τ = 20` steps (the hello waveform), `W = 250`, reference `L = 144` (center 72; the same two inequalities: `W ≥ 160 + 1.75·46 + 10`, `20 + 1.75·(144 − 6) = 261 > 250`). ≈ 7.5e8 updates, a few seconds in release. Same metric and `< −40` assert; this is the number to quote in the PR as the gate result. Also run it with `alpha_max = 0.033` (section 10) and quote both.

**T6 `cpml_split_run_is_bit_identical`** (gate c substitute). Grid 24×22×20 with `set_cpml(6, default)` and `small_sim`-like soft + hard sources: `run(150)` vs `run(70); run(80)` → equal `field_bits()` and records; also a second fresh run of 150 equals the first. (See section 13 for why this replaces "parallel = serial".)

**T7 `cpml_each_face_absorbs_on_its_own_side`** (Michael's face-axis check). Grid `nx = 22, ny = 20, nz = 18` (all different, so an axis swap cannot hide), 1 mm, `n = 4` all faces. Sources: soft `dgauss` (τ = 6 steps) on Ez **and** Ex at `(10, 11, 9)` (Ez radiates along ±x and ±y, Ex along ±z). Six probes, each one cell inside its face's interface on the axis through the source:

| Face | Probe component and `(i, j, k)` |
|---|---|
| xmin | Ez `(10, 5, 9)` |
| xmax | Ez `(10, 17, 9)` (interface E node `22 − 4 = 18`) |
| ymin | Ez `(5, 11, 9)` |
| ymax | Ez `(15, 11, 9)` (interface node 16) |
| zmin | Ex `(10, 11, 5)` |
| zmax | Ex `(10, 11, 13)` (interface node 14) |

Runs: one reference with all six faces on, then six runs each with exactly one face turned off via `set_cpml_face(f, 0)`; `W = 100` steps each (7 runs × 7,920 cells × 100 ≈ 5.5e6 updates). For off-face `f`, let `diff_g = run_f − ref` at probe `g`, `peak_g = max |ref_g|`, and `first_g` = first step where `|diff_g| > 1e-3 · peak_g`. Assert: `first_f < first_g` for every `g ≠ f` (the face named is the one that reflects first, at its own probe), and `max |diff_f| > 0.03 · peak_f` (the PEC echo is large). Print the six `first_*` per case. If the strict ordering is flaky only at the numerical-precursor level, raise the threshold to `1e-2`; do not loosen the ordering.

**T8 `cpml_then_media_equals_media_then_cpml`.** 12×10×10 grid, `fdtd_media` with `eps_r = 2` (changes `dt`) and `set_cpml(3, default)` in both orders → identical `field_bits()` after 100 steps. Catches the missing rebake in `set_media` (P6).

**T9 `cpml_setup_rules`** (and extend `bytes_count_the_coefficients`). Errors: `set_cpml` after a step; `n` with `n_lo + n_hi >= N` on the smallest axis; `set_cpml_face` before `set_cpml`; face value not 0 or `n`; each bad param (`sigma_factor −1`, `kappa_max 0.5`, `alpha_max NaN`, `order −1`, `alpha_order inf`). After each error, `cpml_faces()` and `bytes()` are unchanged. `cpml_faces()` reflects `set_cpml(2)`, then `set_cpml_face(ZMin, 0)` → `[2, 2, 2, 2, 0, 2]`, then back on. `set_cpml(0)` → zeros and `bytes()` back to the no-CPML value. Bytes: on the 5×4×3 grid, `set_cpml(1, default)` adds exactly `12·n·8 + 2·3·(5 + 4 + 3)·8`.

### 8.2 `crates/rustlab-script/tests/fdtd.rs`

- `cpml_errors`: one `run_err` per message in section 6.6 / 6.2 (unknown option, odd option list, repeated option, bad face name, face value `3` when `n = 8`, face form first, `n = 1.5`, `n = -1`, `n` too large, after `fdtd_step`). Match on a distinctive substring.
- `cpml_sigma_zero_matches_no_cpml_bit_for_bit`: two 12×12×12 scripts, same soft source and probe, one with `fdtd_cpml(sim, 3, "sigma_factor", 0)`; compare `bits(...)` of `out.probe` after 120 steps.
- `cpml_info_reports_faces_and_bytes`: `info.cpml` is `[4 4 4 4 4 4]`, then `[4 4 4 4 0 4]` after `fdtd_cpml(sim, "zmin", 0)`; `bytes` grows by `12·ncell·8 + 2·3·(nx+ny+nz)·8` against a grid without CPML.
- `cpml_absorbs_at_script_level`: 24³, `fdtd_cpml(sim, 6)`, hello-style `dgauss` source, late-time probe max over peak is at least 30 dB lower than the same PEC run.

### 8.3 Integration gates

- `cargo test --workspace` green (debug).
- `cargo test --release -p rustlab-dsp -- --ignored cpml_reflection` passes; quote its numbers.
- `cargo build --bin rustlab && bash dev/run-examples.sh` exits 0 (includes `fdtd_pml.rlab`, which itself errors if the gate fails; section 10).
- `cargo build --release -p rustlab-notebook --bin rustlab-notebook && ./target/release/rustlab-notebook render examples/notebooks/fdtd_pml.md -o /tmp/fdtd_pml.html --strict` exits 0; also `rustlab-notebook check examples/notebooks/fdtd_pml.md`.
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace` introduces no new warnings in the touched files.

---

## 9. Performance procedure

### 9.1 Baseline (main, this VM, before any change)

`cargo run --release --example bench_fdtd -p rustlab-dsp` on `8d5638f`, measured while writing this brief:

| Case | Mcell/s | % of vacuum |
|---|---|---|
| vacuum | 364.0 | 100% |
| PEC | 304.1 | 84% |
| dielectric | 301.2 | 83% |
| dielectric + mu | 251.9 | 69% |
| lossy | 253.9 | 70% |

(The 0.3.48 CHANGELOG numbers on Michael's machine were 374 / 305 / 290 / 242 / 246, i.e. 82 / 78 / 65 / 66%.) Re-measure main yourself in the same session as the branch numbers (`git worktree add /tmp/rustlab-main 8d5638f`), twice, best of each; VMs drift.

### 9.2 Bench changes (`crates/rustlab-dsp/examples/bench_fdtd.rs`)

- `build(case, cpml: bool)`: after the existing case setup, `if cpml { sim.set_cpml(8, CpmlParams::default()).unwrap(); }` (all faces; `NZ = 20` leaves 4 interior pages, fine for timing).
- Print one line per case with both columns: `CPML off: X Mcell/s (P% of vacuum off)  CPML on: Y Mcell/s (Q% of vacuum off)`. Mcell/s counts all `nx·ny·nz` cells, slab included.
- `gate` argument (`cargo run --release --example bench_fdtd -p rustlab-dsp -- gate`): 60×60×20, `dx = dy = 1e-3`, `dz = 0.8e-3` (the existing bench grid), vacuum, `set_cpml(8, default)`, the existing soft source and probe, **8000** steps, one timed run, print wall seconds and Mcell/s, `std::process::exit(1)` if `>= 60 s`. Update the file's doc comment to describe both modes.

### 9.3 What to report in the PR

| Case | main | branch, CPML off | off vs main | branch, CPML on | on vs branch vacuum-off |
|---|---|---|---|---|---|

plus the gate line (seconds for 8000 steps). Expected bands, not gates: CPML off at 75–95% of main (twelve extra loads, stores, and multiply-adds per cell against one-row scratch that stays in L1); CPML on at 40–70% of vacuum-off (the full `q` arrays double the memory traffic); gate a few seconds, far under 60 s.

### 9.4 CPML-off regression rule

If branch vacuum CPML-off is below 85% of main (same session, best of 5, measured twice):
1. Check bounds-check elision: every per-j slice (`kx`, `bx`, `ax`, `q*`) must be resliced to the loop length before the loop (`&x[..nx]`), as the existing code does for `cb`/`ca`. Look at `cargo asm` or `perf annotate` for `panic_bounds_check` in the inner loops.
2. Confirm row scalars are hoisted (not reloaded per `j`).
3. Confirm no `mul_add` and no accidental `Vec` allocation per step.
Then report the number. Do **not** add a second (non-ψ) kernel path; list it as an option for Michael (section 5.5).

**Superseded (2026-11-03).** The CPML-off number was 52% of main. Michael approved the second path. Shipped behavior and the new numbers are in `dev/plans/fdtd3d.md`.

---

## 10. Example, notebook, and `alpha_max`

### 10.1 `alpha_max` value and why

Use **`alpha_max = 0.033` S/m** (write it in the script as `2*pi*eps0*f_alpha` with `f_alpha = 0.6e9`, so the derivation is visible).

Justification to put in the notebook prose:
- The CFS stretch is `s = κ + σ/(α + jωε0)`. For `ω ≫ α/ε0` it is the ordinary PML stretch; for `ω ≪ α/ε0` it becomes real, so the layer stops absorbing there. `α` sets a corner frequency `f_α = α/(2π ε0)`.
- The example's source current is `−((t−t0)/τ)·exp(−((t−t0)/τ)²)` with `τ = 20·dt ≈ 38.1 ps` (1 mm cubic cells, `dt ≈ 1.906 ps`). Its spectrum `∝ f·exp(−(π f τ)²)` peaks at `f_pk = 1/(√2 π τ) ≈ 5.9 GHz`. Taking `f_α = f_pk/10 ≈ 0.6 GHz` keeps the corner a decade below the band that carries the pulse (in-band absorption essentially unchanged; at 5.9 GHz `ωε0 ≈ 0.33 S/m`, ten times `α`) while damping the quasi-static and very-low-frequency part, which is where an α = 0 CPML is known to be weak (evanescent fields, slow late-time drift). Below 0.6 GHz the wavelength is over 500 mm against a 40 mm box, so "reflection" there is near-field coupling, not a travelling wave.
- `α(ρ) = α_max (1 − ρ)` is largest at the inner interface and 0 at the wall; with `σ_max ≈ 10.6 S/m`, `σ` exceeds `α_max` from about the second cell in.
- Expectation, to be replaced by the measured numbers: with a zero-mean source and a probe 6 cells from the layer, the change is small (a few dB either way). Report what you measure; if α makes it worse, say so — it does not change the default (`alpha_max = 0` stays the default per Decision 7).

### 10.2 `examples/pde/fdtd_pml.rlab`

Header in the house style (`# CPML: a pulse leaves the box instead of bouncing` / `# Demonstrates: fdtd_cpml, ...`). Content:

- 40×40×40, 1 mm, `nsteps = 400`, the hello source (soft derivative-of-Gaussian Ez at `ic = n/2 + 1`, `τ = 20·dt`, `t0 = 4τ`) and probe (`ic, ic + 6, ic`).
- A function `ez = pml_run(n, dx, nsteps, npml, alpha)` that builds the grid, calls `fdtd_cpml(sim, npml, "alpha_max", alpha)` when `npml > 0`, adds source and probe, runs, returns the probe column. (Functions with `if` inside are fine; see `fdtd_cavity.rlab`.)
- Three runs: PEC (`npml = 0`), CPML defaults (`npml = 8, alpha = 0`), CPML with `alpha = 2*pi*eps0*0.6e9`.
- `n_left = ceil(8*20 + 6*dx/(c0*dt))` (the step after which the direct pulse has passed the probe; about 171). `late_dB = 20*log10(max(abs(ez(n_left:end))) / max(abs(ez)))` for each run. `fprintf` the three numbers and `fdtd_info(sim).bytes` with and without CPML.
- Gate inside the script: `if late_cpml > -40, error(sprintf("CPML late residual %.1f dB is above -40 dB", late_cpml)); end` so `run-examples` fails if the absorber regresses.
- Plot: `subplot(2, 1, 1)` the PEC and CPML traces vs `t` in ns; `subplot(2, 1, 2)` `mag2db(ez / max(abs(ez)))` for all three runs, `ylim([-120, 0])`, legend/labels.

### 10.3 `examples/notebooks/fdtd_pml.md`

Same runs, split into cells with prose: what CPML is (one paragraph, the stretch `s = κ + σ/(α + jωε0)`, the `ψ` recursion with `b` and `a` in display math), the face names and `fdtd_cpml(sim, "zmin", 0)`, the three-run comparison, a small table of the late residuals (template interpolation of the measured values), the α paragraph from 10.1, and a "Try" line (thinner layer `n = 4`, `sigma_factor` 0.5 / 2). Use `> [!NOTE]` callouts, not HTML. Math in `$...$` / `$$...$$`. Keep it under the runtime of `fdtd_cavity.md`. Regenerate `gallery/` (section 6.9).

---

## 11. Docs, CHANGELOG, version, plan

- `Cargo.toml` `version = "0.3.51"`; `cargo build --workspace`; stage `Cargo.lock` with it.
- `CHANGELOG.md`, new top section `## 0.3.51`:
  - *Added*: `fdtd_cpml` (forms, defaults, face names, disabled face = PEC wall, only before the first step); `fdtd_info(sim).cpml`; `bytes` includes CPML; example + notebook; the measured gate numbers (T5b residual with and without α), and the perf table summary (CPML off vs 0.3.50, CPML on, 8000-step gate time).
  - *Changed* (or *Breaking / behavior changes* if Michael prefers): none for scripts without `fdtd_cpml` — fields and records are bit-identical to 0.3.50 (say so explicitly; T2 proves it). `fdtd_info` gains a field.
- `docs/functions.md` FDTD section (~line 301): change the intro sentence (line 303) — CPML is no longer "a later phase"; add `### fdtd_cpml` after `fdtd_pec` with signature, description, defaults table, the face/axis table from 2.3 (user-facing 1-based columns), the v1 limitation (σ not scaled for dielectric-filled slabs, uniform spacing), and an example; update `fdtd_info` for `cpml` and `bytes`.
- `docs/quickref.md` FDTD block (~line 161–170): add the `fdtd_cpml` row; adjust the intro sentence ("PEC walls; vacuum unless ... ; `fdtd_cpml` adds an absorber").
- `AGENTS.md`: builtin table row for `fdtd_cpml` (after `fdtd_pec`), update the `fdtd_info` row; `rustlab-dsp` `src/fdtd/` bullet (add `cpml.rs`: profiles, `Face`, `CpmlParams`, `Absorber`; `yee.rs`: every derivative is `kinv*du − q`, `q = −Δψ`, scratch row when off); `rustlab-script` `eval/fdtd.rs` bullet (option parsing); Active Plans `fdtd3d` row ("**Phase 3 shipped in 0.3.51** (...numbers...). Phase 4 (lumped port) waits for an explicit yes."); Repository Layout `pde/` count 7 → 8 and add `fdtd_pml`.
- `dev/plans/fdtd3d.md`: top **Status** and **Current phase** lines; Phase 3 row "shipped in 0.3.51" with the measured residual and perf; under the CPML formulation, add the `q = −Δψ` storage note and the `a = 0 when σ = 0` rule; record the gate (c) deviation (section 13) under Decisions.
- `docs/agent-guide.md`: only if it describes FDTD boundaries (grep for `fdtd`); otherwise leave it.
- `llms.txt`: no change (no new docs file).

Dead-code / stale-docs check before opening the PR (note the result in the PR body): `cargo clippy --workspace` for dead code in `fdtd/`; grep docs for "CPML" / "absorber" / "later phase" statements that are now false (`docs/functions.md`, `docs/quickref.md`, `AGENTS.md`, `examples/notebooks/fdtd_hello.md` "PEC walls" prose, `README.md`); confirm nothing references a removed name.

---

## 12. Pitfalls checklist

| # | Pitfall | What goes wrong | Guard |
|---|---|---|---|
| P1 | Sign of `a` | `a ≤ 0`. Flipping it (or using `q := b q + a du` with `term = kinv du − q`) amplifies instead of absorbing. | Formula in 3.1/3.3 exactly; T1 asserts `a ≤ 0`; T4 energy drain. |
| P2 | Using the old `q` | The term must use the **updated** `q` (`aux` returns it). | Code shape in 5.2; T5 residual. |
| P3 | Half-cell H sampling | H derivatives use `(m + ½)` positions; using the node formula for H shifts the profile by half a cell (absorbs, but worse, and breaks T1). | Table 3.2; T1 checks both. |
| P4 | E vs H profile set | E aux must use `Absorber.e`, H aux `Absorber.h`. | T1 + T5. |
| P5 | Axis/face swap | `x` ↔ column `j`, `y` ↔ row `i`, `z` ↔ page `k`; `min` = small index. | Table 2.3; T7 with distinct `nx, ny, nz`. |
| P6 | Stale `dt` | `b` depends on `dt`; `set_media` changes `dt`. | Rebake in `set_media`; T8. |
| P7 | Bit-exact identity | Needs `kinv = 1.0` exactly, `q` stored as `−Δψ` and starting `+0.0`, subtraction `kinv*du − q`, identity entries as literals, no `mul_add`, same operand order as today. | 3.3, 5.4; T2 and the two untouched golden tests. |
| P8 | `α` at the interface | `α(ρ = 0) = alpha_max ≠ 0`. Evaluating the profile outside the slab puts a nonzero `b` / `a` in the interior. | Membership by index, identity literals outside (3.1). |
| P9 | `0/0` | `σ = 0, α = 0` makes the `a` formula `NaN`. | `if σ == 0.0 { a = 0.0 }`. |
| P10 | Last H column | `Hy`/`Hz` at `j = nx − 1` have their own closures; they need the x aux too. | 5.2 code; T7 (`xmax` probe). |
| P11 | Calling `curl(j)` twice | Double-updates `q`. | Keep `step_e`/`step_h` loops as they are. |
| P12 | Overlapping slabs | `n_lo + n_hi ≥ N` leaves no interior and overlaps profiles. | Validation in 6.2; T9. |
| P13 | Plain Gaussian soft source in tests | Leaves static charge; the static field differs between grids and shows up as a fake late residual. | Use the zero-mean `dgauss` everywhere in CPML tests and the example. |
| P14 | Reference grid too small | A wall echo inside the window makes `R` meaningless. | The two sizing inequalities in T5; keep them in the doc comment. |
| P15 | Partial mutation on error | A failed `fdtd_cpml` must not leave half-applied settings. | Validate before assigning; T9. |

---

## 13. Deviations from the plan that need Michael's sign-off

1. **Gate (c), "parallel = serial with CPML on just above the parallel threshold":** there is no parallel FDTD path yet (Phase 6 adds rayon y-slabs). Default: record it as **N/A in Phase 3**, keep the kernel row-local so Phase 6 can split rows, and substitute T6 (split-run and repeat-run bit identity with CPML on). Alternative if Michael wants it now: pull a minimal row-parallel loop forward (rayon is already a dependency; threshold `PAR_MIN_CELLS = 32_768` with a test-only override like `vector_calc`'s `__test_set_par_threshold`; per-row `q` offsets into the full arrays; scratch rows per thread). That is Phase 6 scope creep; do not do it without a yes.
2. **`q = −Δψ` storage** instead of the plan's literal `ψ`: same mathematics, needed for exact signed-zero identity (3.3). Document in the plan.
3. **`a = 0 when σ = 0`** instead of "when `σ = 0` and `α = 0`": same values, avoids `0/0` (3.1).
4. **`fdtd_info(sim).cpml`** (new field) and **face re-enable** (`fdtd_cpml(sim, face, n)` with the current `n`): small additions not in the plan text.
5. **Reflection test size:** the default CI test uses a 28³ grid and `τ = 8·dt` so it fits a debug run; the plan's 40³ hello geometry is the `#[ignore]` release test T5b, whose number is the one quoted for the gate.

---

## 14. Suggested commit sequence

1. `fdtd: CPML profiles, settings and absorber (cpml.rs) + profile tests` (T1, no kernel change yet).
2. `fdtd: route every curl derivative through the CPML auxiliary` (yee.rs + mod.rs wiring; T2, T3, T4, T6, T8, T9; existing golden tests unchanged and passing).
3. `fdtd: reflection and face-axis tests` (T5, T5b, T7).
4. `script: fdtd_cpml builtin, fdtd_info.cpml, REPL help` (+ script tests).
5. `bench: CPML columns and the 8000-step gate`.
6. `examples: fdtd_pml script and notebook` (+ gallery).
7. `docs: fdtd_cpml reference, quickref, AGENTS, plan status; bump to 0.3.51` (CHANGELOG with measured numbers).

Then push the branch and open a **draft** PR whose body has: gate results with numbers (a: bit-exact yes/no; b: T5 and T5b residuals, late-time values, PEC sanity; c: N/A + T6; d: `render --strict`, `run-examples`, `cargo test --workspace`), the perf table from 9.3, the α comparison, every deviation from section 13 with its reason, small bugs fixed along the way (each with its regression test), and the dead-code / stale-docs check result.
