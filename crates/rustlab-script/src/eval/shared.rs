//! Copy-on-write storage for array values.
//!
//! `Value::Vector`, `Value::Matrix` and `Value::Tensor3` hold their array in
//! a [`Shared`]: a reference-counted pointer, so cloning a `Value` (reading a
//! variable, passing an argument, capturing it in a lambda, copying a struct,
//! handing it to a `parmap` task) costs a counter increment instead of a copy
//! of every element.
//!
//! Reads go through `Deref`. Writes go through [`Shared::make_mut`], which
//! copies the array first when another value still refers to it, so
//! `b = a; b(1) = 5` leaves `a` unchanged. There is deliberately no
//! `DerefMut`: it would hide one atomic check per element inside write
//! loops. A caller that would overwrite every element anyway should check
//! [`Shared::get_mut`] and build a new array when the buffer is shared,
//! instead of paying for the copy.
//!
//! Helpers are associated functions (`Shared::get_mut(&mut s)`), as on
//! `Arc`, so they never shadow a method of the array inside.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

pub struct Shared<T>(Arc<T>);

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        Shared(Arc::new(value))
    }

    /// Mutable access without copying, or `None` when the buffer is shared.
    pub fn get_mut(this: &mut Self) -> Option<&mut T> {
        Arc::get_mut(&mut this.0)
    }

    /// True when no other value refers to this buffer.
    pub fn is_unique(this: &Self) -> bool {
        Arc::strong_count(&this.0) == 1
    }

    /// True when both refer to the same buffer.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }
}

impl<T: Clone> Shared<T> {
    /// Mutable access, copying the array first when it is shared. Call it
    /// once per write site and keep the reference: each call is an atomic
    /// check, too slow to repeat per element.
    pub fn make_mut(this: &mut Self) -> &mut T {
        Arc::make_mut(&mut this.0)
    }

    /// The array itself: moved out when unique, copied when shared.
    pub fn into_inner(this: Self) -> T {
        Arc::try_unwrap(this.0).unwrap_or_else(|a| (*a).clone())
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared(Arc::clone(&self.0))
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Shared::new(value)
    }
}

impl<T: fmt::Debug> fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: PartialEq> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        Shared::ptr_eq(self, other) || *self.0 == *other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_after_clone_copies_and_leaves_the_original() {
        let a = Shared::new(vec![1, 2, 3]);
        let mut b = a.clone();
        assert!(Shared::ptr_eq(&a, &b));
        Shared::make_mut(&mut b)[0] = 5;
        assert_eq!(*a, vec![1, 2, 3]);
        assert_eq!(*b, vec![5, 2, 3]);
        assert!(!Shared::ptr_eq(&a, &b));
    }

    #[test]
    fn unique_buffer_is_written_in_place() {
        let mut a = Shared::new(vec![1, 2, 3]);
        let p = a.as_ptr();
        Shared::make_mut(&mut a)[1] = 7;
        assert_eq!(a.as_ptr(), p);
        assert!(Shared::get_mut(&mut a).is_some());
        let b = a.clone();
        assert!(Shared::get_mut(&mut a).is_none());
        drop(b);
        assert!(Shared::is_unique(&a));
    }

    #[test]
    fn into_inner_moves_when_unique_and_copies_when_shared() {
        let a = Shared::new(vec![1, 2, 3]);
        let p = a.as_ptr();
        assert_eq!(Shared::into_inner(a).as_ptr(), p);
        let a = Shared::new(vec![4]);
        let b = a.clone();
        assert_eq!(Shared::into_inner(b), vec![4]);
        assert_eq!(*a, vec![4]);
    }
}
