//! Shared seedable RNG used by every random builtin (`rand`, `randn`,
//! `randi`, `rand3`, `randn3`, `sprand`). Calling `seed(N)` re-seeds the
//! thread-local generator so notebooks become deterministic across renders;
//! without a `seed()` call, the generator is initialised from OS entropy and
//! behaves like the previous `rand::thread_rng()` baseline.
//!
//! All builtins that draw random values must go through `with_rng` rather
//! than calling `rand::thread_rng()` directly, otherwise `seed()` won't
//! affect them.

use rand::rngs::StdRng;
use rand::SeedableRng;
use std::cell::{Cell, RefCell};

thread_local! {
    static RNG: RefCell<StdRng> = RefCell::new(StdRng::from_entropy());
    /// Tracks the most recent explicit `seed(N)` value on this thread, so
    /// `parmap` can derive deterministic per-task seeds without disturbing
    /// the calling thread's RNG. `None` means "never explicitly seeded";
    /// parmap falls back to OS entropy for the base in that case.
    static MASTER_SEED: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Run `f` with mutable access to the thread-local RNG. Random builtins use
/// this so a single `seed(N)` call covers every subsequent draw on the same
/// thread.
pub fn with_rng<R>(f: impl FnOnce(&mut StdRng) -> R) -> R {
    RNG.with(|cell| f(&mut cell.borrow_mut()))
}

/// Re-seed the thread-local RNG with a deterministic 64-bit seed.
pub fn seed_rng(seed: u64) {
    RNG.with(|cell| *cell.borrow_mut() = StdRng::seed_from_u64(seed));
    MASTER_SEED.with(|c| c.set(Some(seed)));
}

/// Re-seed from OS entropy — restores non-deterministic behaviour after a
/// previous `seed(N)` call.
pub fn seed_rng_from_entropy() {
    RNG.with(|cell| *cell.borrow_mut() = StdRng::from_entropy());
    MASTER_SEED.with(|c| c.set(None));
}

/// Return the master seed last set by `seed(N)` on this thread, or `None`
/// if the RNG is currently in entropy mode. Used by `parmap` to derive
/// per-task seeds deterministically.
pub fn current_master_seed() -> Option<u64> {
    MASTER_SEED.with(|c| c.get())
}

/// Mix `master_seed` and `task_index` into a deterministic per-task seed
/// via SplitMix64. Two different `task_index` values always produce
/// different output seeds; the same `(master, idx)` pair always produces
/// the same output seed. That's what `parmap`'s determinism contract
/// promises.
pub fn derive_task_seed(master_seed: u64, task_index: usize) -> u64 {
    // SplitMix64 finalizer — fast, good avalanche, deterministic.
    let mut z = master_seed.wrapping_add((task_index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
