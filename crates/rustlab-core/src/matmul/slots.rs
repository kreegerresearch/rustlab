//! How many threads may run the SME kernel at once.
//!
//! An Apple M4 has one matrix unit per CPU cluster, shared by every core in
//! it. When more threads multiply than there are units, they queue on the
//! units while their cores' own vector units sit idle. A product that cannot
//! get a slot runs on the NEON kernel instead (`neon.rs`), which reads the
//! same packed panels and rounds the same way, so the route never changes a
//! result bit.

use std::sync::atomic::{AtomicUsize, Ordering};

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
        todo!()
    }

    pub(crate) fn set_cap(&self, _cap: usize) {
        todo!()
    }

    /// A slot when fewer than `cap` are taken (always, when there is no cap).
    pub(crate) fn try_acquire(&self) -> Option<SlotGuard<'_>> {
        todo!()
    }
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

pub(crate) fn parse_count(_v: Option<&str>) -> Option<usize> {
    todo!()
}
