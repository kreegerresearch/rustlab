"""Phase 6 planning: closed-form estimates the FDTD patch is checked against.

Not part of the build. numpy only:
    python3 dev/plans/fdtd3d_phase6_impl/estimates.py

Transmission-line model (Balanis 14.2): eps_eff, the open-end extension dL,
and the resonance of a metal span L_m. On the Yee grid the metal is a set of
nodes; the edge E field sits half a cell outside the last node on each side,
so the "staircase-aware" length is L_m + d (one cell), plus 2 dL.

Radiation Q and surface-wave efficiency: Jackson & Alexopoulos (1991),
Q_d = 1/tan(delta). Directivity: two radiating slots over an infinite ground
(Balanis 14.2.4), for an order-of-magnitude check only; the finite ground
of the capstone is not in this model.
"""

import numpy as np

C0 = 299792458.0
ER = 4.4
H = 1.6e-3
F0 = 2.4e9
TAND = 0.02


def tl(w):
    ee = (ER + 1) / 2 + (ER - 1) / 2 / np.sqrt(1 + 12 * H / w)
    dl = 0.412 * H * (ee + 0.3) * (w / H + 0.264) / ((ee - 0.258) * (w / H + 0.8))
    return ee, dl


def f_res(lm, wm, d):
    """Resonance of an lm x wm metal span on cell size d (0 = continuum)."""
    ee, dl = tl(wm + d)
    return C0 / (2 * (lm + d + 2 * dl) * np.sqrt(ee))


def q_rad(lp, wp, f):
    n = np.sqrt(ER)
    c1 = 1 - 1 / n**2 + 2 / (5 * n**4)
    k = 2 * np.pi * f / C0
    p = 1 - 0.16605 / 20 * (k * wp) ** 2 + 0.00761 * 3 / 560 * (k * wp) ** 4 - 0.0914153 / 5 * (k * lp) ** 2
    q_space = 3 / 16 * ER / (p * c1) * (lp / wp) * (C0 / f / H)
    e_sw = 1 / (1 + 0.75 * np.pi * k * H / c1 * (1 - 1 / n**2) ** 3)
    return q_space, e_sw, q_space * e_sw


def two_slot_directivity(we, le, f):
    k = 2 * np.pi * f / C0
    th = np.linspace(1e-6, np.pi - 1e-6, 2001)
    ph = np.linspace(0, np.pi, 801)
    t, p = np.meshgrid(th, ph, indexing="ij")
    x = k * we / 2 * np.cos(t)
    slot = np.sin(x) / np.cos(t) * np.sin(t)
    af = np.cos(k * le / 2 * np.sin(t) * np.sin(p))
    i2 = np.trapz(np.trapz((slot * af) ** 2 * np.sin(t), ph, axis=1), th)
    return (2 * np.pi * we / (C0 / f)) ** 2 * np.pi / i2


def main():
    w = C0 / (2 * F0) * np.sqrt(2 / (ER + 1))
    ee, dl = tl(w)
    l = C0 / (2 * F0 * np.sqrt(ee)) - 2 * dl
    print(f"design: W = {w*1e3:.4f} mm, L = {l*1e3:.4f} mm, eps_eff = {ee:.4f}, dL = {dl*1e3:.4f} mm")
    print(f"cavity TM010, no fringing: L = {l*1e3:.2f} mm -> {C0/(2*l*np.sqrt(ER))/1e9:.4f} GHz")
    print("staircase-aware TL resonance (metal span + one cell + 2 dL):")
    for tag, lm, wm, d in [
        ("2 mm, 28 x 36", 28, 36, 2),
        ("2 mm, 28 x 40", 28, 40, 2),
        ("1.5 mm, 27 x 36", 27, 36, 1.5),
        ("1 mm, 28 x 38", 28, 38, 1),
        ("0.5 mm, 28 x 38", 28, 38, 0.5),
        ("continuum, 28 x 38", 28, 38, 0),
    ]:
        print(f"  {tag:20s} {f_res(lm*1e-3, wm*1e-3, d*1e-3)/1e9:.4f} GHz")

    fr = 2.44e9
    qs, esw, qr = q_rad(29e-3, 39e-3, fr)
    qd = 1 / TAND * fr / F0
    print(f"Q_space = {qs:.1f}, e_sw = {esw:.3f}, Q_rad = {qr:.1f}, Q_d (sigma fixed at f0) = {qd:.1f}")
    for fill in (1.0, 0.85):
        eta = (1 / qr) / (1 / qr + fill / qd)
        print(f"  radiation efficiency, filling factor {fill}: {eta:.3f}")
    _, dle = tl(39e-3)
    d = two_slot_directivity(39e-3, 29e-3 + 2 * dle, fr)
    print(f"two-slot directivity, infinite ground: {d:.2f} ({10*np.log10(d):.2f} dBi)")

    print("width sweep, 2 mm lesson mesh, L_m = 28 mm:")
    for wm in (28, 32, 36, 40, 44):
        print(f"  W = {wm} mm: {f_res(28e-3, wm*1e-3, 2e-3)/1e9:.4f} GHz")


if __name__ == "__main__":
    main()
