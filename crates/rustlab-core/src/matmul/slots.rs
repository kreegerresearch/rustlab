//! How many threads may run the SME kernel at once.
//!
//! An Apple M4 has one matrix unit per CPU cluster, shared by every core in
//! it. When more threads multiply than there are units, they queue on the
//! units while their cores' own vector units sit idle. A product that cannot
//! get a slot runs on the NEON kernel instead (`neon.rs`), which reads the
//! same packed panels and rounds the same way, so the route never changes a
//! result bit.
//!
//! Only SME-sized products on an SME machine consult the slots, so on x86
//! and on Arm without SME nothing here runs.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

/// A counter of SME kernels in flight with an optional cap.
pub(crate) struct Slots {
    /// 0 means no cap.
    cap: AtomicUsize,
    busy: AtomicUsize,
}

/// Held while one SME kernel runs; releases the slot on drop.
pub(crate) struct SlotGuard<'a>(&'a Slots);

impl Slots {
    pub(crate) const fn new(cap: usize) -> Slots {
        Slots {
            cap: AtomicUsize::new(cap),
            busy: AtomicUsize::new(0),
        }
    }

    pub(crate) fn cap(&self) -> usize {
        self.cap.load(Ordering::Relaxed)
    }

    pub(crate) fn set_cap(&self, cap: usize) {
        self.cap.store(cap, Ordering::Relaxed);
    }

    /// A slot past the cap, for products too large to overflow to NEON.
    pub(crate) fn acquire_anyway(&self) -> SlotGuard<'_> {
        self.busy.fetch_add(1, Ordering::AcqRel);
        SlotGuard(self)
    }

    /// A slot when fewer than `cap` are taken (always, when there is no cap).
    pub(crate) fn try_acquire(&self) -> Option<SlotGuard<'_>> {
        let cap = self.cap();
        if cap == 0 {
            self.busy.fetch_add(1, Ordering::AcqRel);
            return Some(SlotGuard(self));
        }
        let mut cur = self.busy.load(Ordering::Acquire);
        loop {
            if cur >= cap {
                return None;
            }
            match self
                .busy
                .compare_exchange_weak(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return Some(SlotGuard(self)),
                Err(now) => cur = now,
            }
        }
    }
}

/// The process-wide slots. The cap comes from `RUSTLAB_SME_SLOTS` (0 = no
/// cap) or, when that is unset or not a count, [`default_cap`] (the number
/// of performance clusters on macOS, else 2). Read on first use.
pub(crate) fn global() -> &'static Slots {
    static SLOTS: OnceLock<Slots> = OnceLock::new();
    SLOTS.get_or_init(|| {
        let env = std::env::var("RUSTLAB_SME_SLOTS").ok();
        Slots::new(parse_count(env.as_deref()).unwrap_or_else(default_cap))
    })
}

/// Products with more multiply-adds than this always take the SME kernel,
/// even past the cap: on NEON a large product takes long enough (≈ 5 ms at
/// 512³ on one M4 core) that the last one to finish sets the wall time.
/// `RUSTLAB_SME_OVERFLOW_MACS` overrides it (0 = never overflow).
pub(crate) fn overflow_max_macs() -> &'static AtomicUsize {
    static MAX: OnceLock<AtomicUsize> = OnceLock::new();
    MAX.get_or_init(|| {
        let env = std::env::var("RUSTLAB_SME_OVERFLOW_MACS").ok();
        AtomicUsize::new(parse_count(env.as_deref()).unwrap_or(DEFAULT_OVERFLOW_MACS))
    })
}

/// 2²⁴ ≈ 16.8 M multiply-adds: twice the 129 × 128 × 512 products of a
/// transformer layer, about 0.7 ms on one NEON core.
const DEFAULT_OVERFLOW_MACS: usize = 1 << 24;

/// Products that ran on each kernel since the process started.
pub(crate) static SME_RUNS: AtomicU64 = AtomicU64::new(0);
pub(crate) static NEON_RUNS: AtomicU64 = AtomicU64::new(0);

/// A non-negative integer, ignoring surrounding spaces.
pub(crate) fn parse_count(v: Option<&str>) -> Option<usize> {
    v?.trim().parse().ok()
}

/// One SME unit per performance cluster: `hw.perflevel0.physicalcpu` over
/// `hw.perflevel0.cpusperl2` (each cluster shares one L2). 2 when that
/// cannot be read.
fn default_cap() -> usize {
    super::sme::perf_clusters().unwrap_or(2).max(1)
}

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        self.0.busy.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_limits_concurrent_slots() {
        let s = Slots::new(2);
        let a = s.try_acquire().expect("first");
        let b = s.try_acquire().expect("second");
        assert!(s.try_acquire().is_none(), "third must overflow");
        drop(a);
        let c = s.try_acquire().expect("freed slot");
        assert!(s.try_acquire().is_none());
        drop((b, c));
        assert_eq!(s.busy.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn zero_cap_never_overflows() {
        let s = Slots::new(0);
        let held: Vec<_> = (0..64).map(|_| s.try_acquire().expect("no cap")).collect();
        assert_eq!(s.busy.load(Ordering::Relaxed), 64);
        drop(held);
        assert_eq!(s.busy.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn cap_can_change_while_slots_are_held() {
        let s = Slots::new(4);
        let held: Vec<_> = (0..3).map(|_| s.try_acquire().unwrap()).collect();
        s.set_cap(1);
        assert_eq!(s.cap(), 1);
        assert!(s.try_acquire().is_none());
        drop(held);
        assert!(s.try_acquire().is_some());
    }

    #[test]
    fn threads_never_exceed_the_cap() {
        let s = Slots::new(3);
        let peak = AtomicUsize::new(0);
        std::thread::scope(|sc| {
            for _ in 0..8 {
                sc.spawn(|| {
                    for _ in 0..2000 {
                        if let Some(_g) = s.try_acquire() {
                            let now = s.busy.load(Ordering::Acquire);
                            peak.fetch_max(now, Ordering::Relaxed);
                            std::hint::spin_loop();
                        }
                    }
                });
            }
        });
        assert!(peak.load(Ordering::Relaxed) <= 3);
        assert_eq!(s.busy.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn parses_the_environment_values() {
        assert_eq!(parse_count(Some("3")), Some(3));
        assert_eq!(parse_count(Some(" 0 ")), Some(0));
        assert_eq!(parse_count(Some("lots")), None);
        assert_eq!(parse_count(Some("-1")), None);
        assert_eq!(parse_count(None), None);
    }
}
