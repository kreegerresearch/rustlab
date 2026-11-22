//! Copy-on-write storage for array values, real or complex.
//!
//! `Value::Vector`, `Value::Matrix` and `Value::Tensor3` hold their array in
//! a [`Shared`]: a reference-counted pointer, so cloning a `Value` (reading a
//! variable, passing an argument, capturing it in a lambda, copying a struct,
//! handing it to a `parmap` task) costs a counter increment instead of a copy
//! of every element.
//!
//! **Real or complex storage.** Every rustlab array is complex to the
//! script, but most hold only real numbers. A `Shared` holds either a
//! complex array or a real `f64` one, half the bytes. Which one is never
//! observable: `Deref` always gives the complex view, and for real storage
//! it widens the buffer once (`x → x + 0i`) into a cache kept next to it, so
//! code that only knows complex arrays stays correct. Code that knows about
//! real storage reads [`Shared::as_real`] first and skips the widening.
//!
//! Writes go through [`Shared::make_mut`], which copies the array first when
//! another value still refers to it, so `b = a; b(1) = 5` leaves `a`
//! unchanged. `make_mut` hands out the complex array, so it converts real
//! storage to complex ("promotes" it); a write that keeps the data real uses
//! [`Shared::make_mut_real`] instead. There is deliberately no `DerefMut`: it
//! would hide one atomic check per element inside write loops. A caller that
//! would overwrite every element anyway should check [`Shared::get_mut`] and
//! build a new array when the buffer is shared, instead of paying for the
//! copy.
//!
//! The `--profile` report counts widenings and promotions (`[widen]`,
//! `[promote]`), which show the code paths still to convert.
//!
//! Helpers are associated functions (`Shared::get_mut(&mut s)`), as on
//! `Arc`, so they never shadow a method of the array inside.

use ndarray::{Array, Dimension};
use rustlab_core::C64;
use std::cell::Cell;
use std::fmt;
use std::ops::Deref;
use std::sync::{Arc, OnceLock};

/// An array type that [`Shared`] can hold: `Array<C64, D>`, with
/// `Array<f64, D>` as its real form.
pub trait Storage: Clone {
    type Real: Clone + PartialEq;
    /// `x → x + 0i`, keeping the shape and memory order.
    fn widen(real: &Self::Real) -> Self;
    /// Number of elements of a real array.
    fn real_len(real: &Self::Real) -> usize;
}

impl<D: Dimension> Storage for Array<C64, D> {
    type Real = Array<f64, D>;
    fn widen(real: &Array<f64, D>) -> Self {
        real.mapv(|x| C64::new(x, 0.0))
    }
    fn real_len(real: &Array<f64, D>) -> usize {
        real.len()
    }
}

pub struct Shared<T: Storage>(Arc<Inner<T>>);

struct Inner<T: Storage> {
    /// Authoritative when `Some`: the value is real-stored.
    real: Option<T::Real>,
    /// Authoritative when `real` is `None`; otherwise the widened copy,
    /// filled on the first complex read.
    complex: OnceLock<T>,
}

impl<T: Storage> Inner<T> {
    fn complex(c: T) -> Self {
        Inner { real: None, complex: OnceLock::from(c) }
    }

    /// Turn real storage into complex storage in place, reusing the
    /// widened cache when it is filled.
    fn promote(&mut self) {
        if let Some(r) = self.real.take() {
            let t = timing();
            let c = self.complex.take().unwrap_or_else(|| T::widen(&r));
            count(&PROMOTE, T::real_len(&r), t);
            self.complex = OnceLock::from(c);
        }
    }
}

impl<T: Storage> Shared<T> {
    pub fn new(value: T) -> Self {
        Shared(Arc::new(Inner::complex(value)))
    }

    /// Real storage.
    pub fn from_real(value: T::Real) -> Self {
        Shared(Arc::new(Inner { real: Some(value), complex: OnceLock::new() }))
    }

    /// The real array, when the value is real-stored.
    pub fn as_real(this: &Self) -> Option<&T::Real> {
        this.0.real.as_ref()
    }

    /// True when the value is real-stored.
    pub fn is_real(this: &Self) -> bool {
        this.0.real.is_some()
    }

    /// Mutable access to the complex array without copying, or `None` when
    /// the buffer is shared. Converts real storage to complex.
    pub fn get_mut(this: &mut Self) -> Option<&mut T> {
        let inner = Arc::get_mut(&mut this.0)?;
        inner.promote();
        inner.complex.get_mut()
    }

    /// Mutable access to the real array without copying, or `None` when the
    /// buffer is shared or complex. Drops the widened cache.
    pub fn get_mut_real(this: &mut Self) -> Option<&mut T::Real> {
        let inner = Arc::get_mut(&mut this.0)?;
        inner.real.as_ref()?;
        inner.complex.take();
        inner.real.as_mut()
    }

    /// True when no other value refers to this buffer.
    pub fn is_unique(this: &Self) -> bool {
        Arc::strong_count(&this.0) == 1
    }

    /// True when both refer to the same buffer.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// Mutable access to the complex array, copying it first when it is
    /// shared. Converts real storage to complex. Call it once per write site
    /// and keep the reference: each call is an atomic check, too slow to
    /// repeat per element.
    pub fn make_mut(this: &mut Self) -> &mut T {
        if !Shared::is_unique(this) {
            let copy = match &this.0.real {
                Some(r) => {
                    let t = timing();
                    let c = T::widen(r);
                    count(&PROMOTE, T::real_len(r), t);
                    c
                }
                None => (**this).clone(),
            };
            *this = Shared::new(copy);
        }
        Shared::get_mut(this).expect("unique after the copy")
    }

    /// Mutable access to the real array, copying it first when it is shared,
    /// or `None` when the value is complex-stored. Drops the widened cache.
    pub fn make_mut_real(this: &mut Self) -> Option<&mut T::Real> {
        let r = this.0.real.as_ref()?;
        if !Shared::is_unique(this) {
            *this = Shared::from_real(r.clone());
        }
        Shared::get_mut_real(this)
    }

    /// The complex array itself: moved out when unique, copied when shared.
    pub fn into_inner(this: Self) -> T {
        match Arc::try_unwrap(this.0) {
            Ok(mut inner) => {
                inner.promote();
                inner.complex.into_inner().expect("complex after promote")
            }
            Err(shared) => Shared(shared).deref().clone(),
        }
    }

    /// The real array, moved out when unique and copied when shared, or
    /// `Err(self)` when the value is complex-stored.
    pub fn into_real(this: Self) -> Result<T::Real, Self> {
        if this.0.real.is_none() {
            return Err(this);
        }
        match Arc::try_unwrap(this.0) {
            Ok(inner) => Ok(inner.real.expect("checked above")),
            Err(shared) => Ok(shared.real.clone().expect("checked above")),
        }
    }
}

/// Shape queries that never widen real storage. `v.len()` and friends go
/// through `Deref`, which builds the complex view; converted code uses
/// these instead.
impl<D: Dimension> Shared<Array<C64, D>> {
    pub fn shape(this: &Self) -> &[usize] {
        match &this.0.real {
            Some(r) => r.shape(),
            None => this.0.complex.get().expect("complex storage is always filled").shape(),
        }
    }

    /// Number of elements.
    pub fn len(this: &Self) -> usize {
        Shared::shape(this).iter().product()
    }

    /// Bytes of the authoritative storage: 8 per element when real, 16
    /// when complex.
    pub fn stored_bytes(this: &Self) -> usize {
        Shared::len(this) * if Shared::is_real(this) { 8 } else { 16 }
    }

    /// The real elements as one row-major slice, when real-stored in
    /// standard layout.
    pub fn real_slice(this: &Self) -> Option<&[f64]> {
        this.0.real.as_ref()?.as_slice()
    }
}

impl<T: Storage> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared(Arc::clone(&self.0))
    }
}

impl<T: Storage> Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        let inner = &*self.0;
        match &inner.real {
            None => inner.complex.get().expect("complex storage is always filled"),
            Some(r) => inner.complex.get_or_init(|| {
                let t = timing();
                let c = T::widen(r);
                count(&WIDEN, T::real_len(r), t);
                c
            }),
        }
    }
}

impl<T: Storage> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Shared::new(value)
    }
}

impl<T: Storage + fmt::Debug> fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: Storage + PartialEq> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        if Shared::ptr_eq(self, other) {
            return true;
        }
        match (&self.0.real, &other.0.real) {
            (Some(a), Some(b)) => a == b,
            _ => **self == **other,
        }
    }
}

// ── Storage switch ──────────────────────────────────────────────────────────

thread_local! {
    static REAL_STORAGE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Whether producers build real storage on this thread. On by default;
/// `RUSTLAB_COMPLEX_STORAGE=1` turns it off process-wide (every array is
/// complex-stored, as before 0.3.17), and tests can switch it per thread
/// with [`set_real_storage`] to compare both modes.
pub fn real_storage() -> bool {
    REAL_STORAGE.with(|c| match c.get() {
        Some(on) => on,
        None => {
            static ENV_OFF: OnceLock<bool> = OnceLock::new();
            !*ENV_OFF.get_or_init(|| std::env::var_os("RUSTLAB_COMPLEX_STORAGE").is_some_and(|v| v != "0"))
        }
    })
}

/// Override [`real_storage`] for this thread (`None` restores the default).
pub fn set_real_storage(on: Option<bool>) {
    REAL_STORAGE.with(|c| c.set(on));
}

impl<D: Dimension> Shared<Array<C64, D>> {
    /// Real storage for `a`, or its complex widening when [`real_storage`]
    /// is off.
    pub fn real_or_widened(a: Array<f64, D>) -> Self {
        if real_storage() {
            Shared::from_real(a)
        } else {
            Shared::new(<Array<C64, D> as Storage>::widen(&a))
        }
    }
}

// ── Counters for the profiler ───────────────────────────────────────────────

/// Calls, elements and nanoseconds of one kind of storage conversion on
/// one thread. Time is measured only once [`set_timing`] has turned it on
/// for the thread (the profiler does that when it starts there).
pub struct Counter {
    calls: Cell<u64>,
    elements: Cell<u64>,
    ns: Cell<u64>,
}

impl Counter {
    const fn new() -> Self {
        Counter { calls: Cell::new(0), elements: Cell::new(0), ns: Cell::new(0) }
    }
}

thread_local! {
    static WIDEN: Counter = const { Counter::new() };
    static PROMOTE: Counter = const { Counter::new() };
    static TIMING: Cell<bool> = const { Cell::new(false) };
}

/// Time conversions on this thread from now on.
pub fn set_timing(on: bool) {
    TIMING.with(|t| t.set(on));
}

fn timing() -> Option<std::time::Instant> {
    TIMING.with(|t| t.get()).then(std::time::Instant::now)
}

fn count(which: &'static std::thread::LocalKey<Counter>, elements: usize, start: Option<std::time::Instant>) {
    let ns = start.map_or(0, |t| t.elapsed().as_nanos() as u64);
    which.with(|c| {
        c.calls.set(c.calls.get() + 1);
        c.elements.set(c.elements.get() + elements as u64);
        c.ns.set(c.ns.get() + ns);
    });
}

/// Totals of one conversion kind: calls, elements, nanoseconds.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Conversions {
    pub calls: u64,
    pub elements: u64,
    pub ns: u64,
}

impl std::ops::Sub for Conversions {
    type Output = Conversions;
    fn sub(self, b: Conversions) -> Conversions {
        Conversions {
            calls: self.calls - b.calls,
            elements: self.elements - b.elements,
            ns: self.ns - b.ns,
        }
    }
}

/// `(widen, promote)` totals on this thread since it started: complex views
/// built from real storage, and real buffers converted to complex by a
/// write.
pub fn conversion_counts() -> (Conversions, Conversions) {
    let read = |c: &Counter| Conversions { calls: c.calls.get(), elements: c.elements.get(), ns: c.ns.get() };
    (WIDEN.with(read), PROMOTE.with(read))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::{array, Array1};

    type V = Shared<Array1<C64>>;

    fn cv(xs: &[f64]) -> Array1<C64> {
        xs.iter().map(|&x| C64::new(x, 0.0)).collect()
    }

    #[test]
    fn write_after_clone_copies_and_leaves_the_original() {
        let a = V::new(cv(&[1.0, 2.0, 3.0]));
        let mut b = a.clone();
        assert!(Shared::ptr_eq(&a, &b));
        Shared::make_mut(&mut b)[0] = C64::new(5.0, 0.0);
        assert_eq!(*a, cv(&[1.0, 2.0, 3.0]));
        assert_eq!(*b, cv(&[5.0, 2.0, 3.0]));
        assert!(!Shared::ptr_eq(&a, &b));
    }

    #[test]
    fn unique_buffer_is_written_in_place() {
        let mut a = V::new(cv(&[1.0, 2.0, 3.0]));
        let p = a.as_ptr();
        Shared::make_mut(&mut a)[1] = C64::new(7.0, 0.0);
        assert_eq!(a.as_ptr(), p);
        assert!(Shared::get_mut(&mut a).is_some());
        let b = a.clone();
        assert!(Shared::get_mut(&mut a).is_none());
        drop(b);
        assert!(Shared::is_unique(&a));
    }

    #[test]
    fn into_inner_moves_when_unique_and_copies_when_shared() {
        let a = V::new(cv(&[1.0, 2.0, 3.0]));
        let p = a.as_ptr();
        assert_eq!(Shared::into_inner(a).as_ptr(), p);
        let a = V::new(cv(&[4.0]));
        let b = a.clone();
        assert_eq!(Shared::into_inner(b), cv(&[4.0]));
        assert_eq!(*a, cv(&[4.0]));
    }

    #[test]
    fn real_storage_reads_as_complex_and_widens_once() {
        let a = V::from_real(array![1.0, -2.0, 0.5]);
        assert!(Shared::is_real(&a));
        let before = conversion_counts().0;
        assert_eq!(*a, cv(&[1.0, -2.0, 0.5]));
        let p = a.as_ptr();
        assert_eq!(a.as_ptr(), p, "the widened view is cached");
        assert_eq!(conversion_counts().0.calls, before.calls + 1);
        assert_eq!(Shared::as_real(&a).unwrap(), &array![1.0, -2.0, 0.5]);
        // Equal to the complex-stored value with the same numbers.
        assert_eq!(a, V::new(cv(&[1.0, -2.0, 0.5])));
        assert_ne!(a, V::from_real(array![1.0, -2.0, 0.25]));
    }

    #[test]
    fn complex_write_promotes_and_leaves_a_real_clone_real() {
        let a = V::from_real(array![1.0, 2.0]);
        let mut b = a.clone();
        let promotions = conversion_counts().1.calls;
        Shared::make_mut(&mut b)[0] = C64::new(0.0, 1.0);
        assert!(!Shared::is_real(&b));
        assert!(Shared::is_real(&a));
        assert_eq!(*a, cv(&[1.0, 2.0]));
        assert_eq!(b[0], C64::new(0.0, 1.0));
        assert_eq!(conversion_counts().1.calls, promotions + 1);

        let mut c = V::from_real(array![3.0]);
        let _ = &*c; // fill the cache
        Shared::get_mut(&mut c).unwrap()[0].im = 2.0;
        assert_eq!(c[0], C64::new(3.0, 2.0));
        assert!(Shared::as_real(&c).is_none());
    }

    #[test]
    fn real_write_keeps_real_storage_and_drops_the_cache() {
        let a = V::from_real(array![1.0, 2.0]);
        let _ = &*a;
        let mut b = a.clone();
        Shared::make_mut_real(&mut b).unwrap()[1] = 9.0;
        assert!(Shared::is_real(&b));
        assert_eq!(*b, cv(&[1.0, 9.0]), "a stale widened cache would show 2");
        assert_eq!(*a, cv(&[1.0, 2.0]));
        let mut c = V::new(cv(&[1.0]));
        assert!(Shared::make_mut_real(&mut c).is_none());
        assert!(Shared::get_mut_real(&mut c).is_none());
    }

    #[test]
    fn into_real_moves_or_reports_complex() {
        let a = V::from_real(array![1.0, 2.0]);
        assert_eq!(Shared::into_real(a).unwrap(), array![1.0, 2.0]);
        let c = V::new(cv(&[1.0]));
        assert!(Shared::into_real(c).is_err());
        let a = V::from_real(array![5.0]);
        assert_eq!(Shared::into_inner(a), cv(&[5.0]));
    }
}
