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
use std::cell::RefCell;

thread_local! {
    static RNG: RefCell<StdRng> = RefCell::new(StdRng::from_entropy());
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
}

/// Re-seed from OS entropy — restores non-deterministic behaviour after a
/// previous `seed(N)` call.
pub fn seed_rng_from_entropy() {
    RNG.with(|cell| *cell.borrow_mut() = StdRng::from_entropy());
}
