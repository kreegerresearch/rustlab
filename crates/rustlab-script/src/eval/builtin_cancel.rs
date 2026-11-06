//! The evaluator's cancel flag, visible to a builtin while it runs.
//!
//! `Evaluator.cancel` is checked between statements. A builtin that runs for
//! seconds (`fdtd_run`) polls [`cancel_requested`] itself. The builtin
//! function type is unchanged; the evaluator installs the flag around the
//! call with [`enter`] when one is set.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

thread_local! {
    static FLAG: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

/// Restores the previous flag on drop (nested builtin calls through `feval`
/// or a lambda keep the outer one).
pub(crate) struct Scope {
    prev: Option<Arc<AtomicBool>>,
}

pub(crate) fn enter(flag: &Arc<AtomicBool>) -> Scope {
    let prev = FLAG.with(|f| f.replace(Some(Arc::clone(flag))));
    Scope { prev }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let prev = self.prev.take();
        FLAG.with(|f| *f.borrow_mut() = prev);
    }
}

/// The calling evaluator's cancel flag is installed and set.
pub(crate) fn cancel_requested() -> bool {
    FLAG.with(|f| {
        f.borrow()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    })
}
