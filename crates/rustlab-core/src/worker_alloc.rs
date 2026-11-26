//! A global allocator that keeps a small per-thread cache of large blocks
//! while a `parmap` task runs.
//!
//! Every block still comes from [`System`]. While a thread holds a
//! [`TaskCache`] guard, a freed block of at least [`MIN_BLOCK`] bytes is
//! kept on that thread instead of returned, and a later request with the
//! same size and alignment gets it back. Outside a guard the allocator is
//! `System` unchanged, so one-thread scripts allocate exactly as before.
//!
//! Why: 8 `parmap` workers that each allocate a window's arrays (about
//! 18 MB of `zeros` in 20 blocks) contend in the system allocator and the
//! kernel. On Linux glibc moves those sizes onto the `brk` heap after the
//! first free, so every window grows and trims the heap again (924 `brk`
//! calls and 134,029 page faults per 100 windows), and `calloc` zeroes the
//! recycled memory as well. Reusing a block costs one `memset` for a zeroed
//! request and nothing for an uninitialised one, with no lock and no fault.
//!
//! Matching is exact (`size` and `align`), so a block is always returned to
//! `System` with the layout it was allocated with, on whichever thread
//! frees it. The cache holds at most [`MAX_BLOCKS`] blocks and
//! [`MAX_BYTES`] bytes per thread; anything past that goes back to
//! `System`. Cached blocks stay with their thread (rayon's pool threads
//! live for the whole process), so a process that ran `parmap` keeps up to
//! `MAX_BYTES` per pool thread.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::marker::PhantomData;
use std::ptr::null_mut;

/// Smallest block the cache keeps. Smaller blocks stay in the system
/// allocator's own per-thread caches.
pub const MIN_BLOCK: usize = 64 * 1024;
/// Most blocks one thread keeps.
pub const MAX_BLOCKS: usize = 32;
/// Most bytes one thread keeps.
pub const MAX_BYTES: usize = 64 << 20;

#[derive(Clone, Copy)]
struct Block {
    ptr: *mut u8,
    size: usize,
    align: usize,
}

const EMPTY: Block = Block {
    ptr: null_mut(),
    size: 0,
    align: 0,
};

/// One thread's cache. No field needs `Drop`, so the thread-local needs no
/// destructor registration (which could allocate from inside the
/// allocator).
struct Cache {
    depth: Cell<u32>,
    len: Cell<usize>,
    bytes: Cell<usize>,
    blocks: UnsafeCell<[Block; MAX_BLOCKS]>,
}

thread_local! {
    static CACHE: Cache = const {
        Cache {
            depth: Cell::new(0),
            len: Cell::new(0),
            bytes: Cell::new(0),
            blocks: UnsafeCell::new([EMPTY; MAX_BLOCKS]),
        }
    };
}

/// While alive, large blocks freed on this thread are cached and reused.
/// Nests. Not `Send`: it belongs to the thread that entered it.
pub struct TaskCache {
    _thread: PhantomData<*const ()>,
}

impl TaskCache {
    pub fn enter() -> Self {
        let _ = CACHE.try_with(|c| c.depth.set(c.depth.get() + 1));
        TaskCache {
            _thread: PhantomData,
        }
    }
}

impl Drop for TaskCache {
    fn drop(&mut self) {
        let _ = CACHE.try_with(|c| c.depth.set(c.depth.get().saturating_sub(1)));
    }
}

/// Blocks and bytes this thread's cache holds now.
pub fn cached_on_this_thread() -> (usize, usize) {
    CACHE
        .try_with(|c| (c.len.get(), c.bytes.get()))
        .unwrap_or((0, 0))
}

/// A cached block with exactly this layout, removed from the cache.
fn take(layout: Layout) -> Option<*mut u8> {
    if layout.size() < MIN_BLOCK {
        return None;
    }
    CACHE
        .try_with(|c| {
            if c.depth.get() == 0 {
                return None;
            }
            // SAFETY: only this thread touches its cache, and nothing here
            // re-enters the allocator while the borrow is live.
            let blocks = unsafe { &mut *c.blocks.get() };
            let n = c.len.get();
            let i = (0..n)
                .rev()
                .find(|&i| blocks[i].size == layout.size() && blocks[i].align == layout.align())?;
            let b = blocks[i];
            blocks[i] = blocks[n - 1];
            blocks[n - 1] = EMPTY;
            c.len.set(n - 1);
            c.bytes.set(c.bytes.get() - b.size);
            Some(b.ptr)
        })
        .ok()
        .flatten()
}

/// Keep `ptr` (allocated by `System` with `layout`) if there is room.
fn put(ptr: *mut u8, layout: Layout) -> bool {
    if layout.size() < MIN_BLOCK {
        return false;
    }
    CACHE
        .try_with(|c| {
            let n = c.len.get();
            if c.depth.get() == 0 || n == MAX_BLOCKS || c.bytes.get() + layout.size() > MAX_BYTES {
                return false;
            }
            // SAFETY: as in `take`.
            let blocks = unsafe { &mut *c.blocks.get() };
            blocks[n] = Block {
                ptr,
                size: layout.size(),
                align: layout.align(),
            };
            c.len.set(n + 1);
            c.bytes.set(c.bytes.get() + layout.size());
            true
        })
        .unwrap_or(false)
}

/// [`System`] plus the per-thread block cache. Install it in a binary:
///
/// ```ignore
/// #[global_allocator]
/// static ALLOC: rustlab_core::worker_alloc::WorkerCacheAlloc =
///     rustlab_core::worker_alloc::WorkerCacheAlloc;
/// ```
pub struct WorkerCacheAlloc;

// SAFETY: every pointer handed out is a `System` allocation of exactly the
// requested layout (fresh, or cached after being freed with that same
// layout), and every pointer given back is either cached or returned to
// `System` with the layout it was allocated with.
unsafe impl GlobalAlloc for WorkerCacheAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match take(layout) {
            Some(p) => p,
            None => unsafe { System.alloc(layout) },
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        match take(layout) {
            Some(p) => {
                unsafe { p.write_bytes(0, layout.size()) };
                p
            }
            None => unsafe { System.alloc_zeroed(layout) },
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !put(ptr, layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: WorkerCacheAlloc = WorkerCacheAlloc;

    fn layout(size: usize) -> Layout {
        Layout::from_size_align(size, 16).unwrap()
    }

    #[test]
    fn outside_a_task_nothing_is_cached() {
        let l = layout(MIN_BLOCK * 4);
        unsafe {
            let p = A.alloc(l);
            A.dealloc(p, l);
        }
        assert_eq!(cached_on_this_thread(), (0, 0));
    }

    #[test]
    fn a_freed_block_is_reused_for_the_same_layout() {
        let l = layout(MIN_BLOCK * 8);
        let _task = TaskCache::enter();
        unsafe {
            let p = A.alloc(l);
            A.dealloc(p, l);
            assert_eq!(cached_on_this_thread(), (1, l.size()));
            let q = A.alloc(l);
            assert_eq!(q, p);
            assert_eq!(cached_on_this_thread(), (0, 0));
            // Another size or alignment is not served from the cache.
            A.dealloc(q, l);
            let other = layout(MIN_BLOCK * 8 + 8);
            let r = A.alloc(other);
            assert_eq!(cached_on_this_thread(), (1, l.size()));
            A.dealloc(r, other);
            let wide = Layout::from_size_align(l.size(), 64).unwrap();
            let s = A.alloc(wide);
            assert_eq!(cached_on_this_thread().0, 2);
            System.dealloc(s, wide);
        }
        drop(_task);
        drain();
    }

    #[test]
    fn a_reused_zeroed_block_is_zero() {
        let l = layout(MIN_BLOCK * 2);
        let _task = TaskCache::enter();
        unsafe {
            let p = A.alloc(l);
            p.write_bytes(0xAB, l.size());
            A.dealloc(p, l);
            let q = A.alloc_zeroed(l);
            assert_eq!(q, p);
            let s = std::slice::from_raw_parts(q, l.size());
            assert!(s.iter().all(|&b| b == 0));
            A.dealloc(q, l);
        }
        drop(_task);
        drain();
    }

    #[test]
    fn small_blocks_and_full_caches_go_back_to_the_system() {
        let _task = TaskCache::enter();
        unsafe {
            let small = layout(MIN_BLOCK - 16);
            A.dealloc(A.alloc(small), small);
            assert_eq!(cached_on_this_thread(), (0, 0));
            let l = layout(MIN_BLOCK);
            let ps: Vec<_> = (0..MAX_BLOCKS + 3).map(|_| A.alloc(l)).collect();
            for p in ps {
                A.dealloc(p, l);
            }
            assert_eq!(cached_on_this_thread().0, MAX_BLOCKS);
            drain_with(l);
            let big = layout(MAX_BYTES / 2 + 64);
            let (p, q) = (A.alloc(big), A.alloc(big));
            A.dealloc(p, big);
            A.dealloc(q, big);
            assert_eq!(cached_on_this_thread(), (1, big.size()));
            drain_with(big);
        }
    }

    #[test]
    fn a_block_may_be_freed_on_another_thread() {
        let l = layout(MIN_BLOCK * 3);
        let _task = TaskCache::enter();
        let p = unsafe {
            let p = A.alloc(l);
            A.dealloc(p, l);
            A.alloc(l)
        } as usize;
        std::thread::spawn(move || unsafe { A.dealloc(p as *mut u8, l) })
            .join()
            .unwrap();
        assert_eq!(cached_on_this_thread(), (0, 0));
    }

    /// Return every cached block of `l` to the system.
    fn drain_with(l: Layout) {
        let _task = TaskCache::enter();
        while cached_on_this_thread().0 > 0 {
            match take(l) {
                Some(p) => unsafe { System.dealloc(p, l) },
                None => break,
            }
        }
    }

    fn drain() {
        for size in [MIN_BLOCK * 2, MIN_BLOCK * 8, MIN_BLOCK * 8 + 8] {
            drain_with(layout(size));
        }
        drain_with(Layout::from_size_align(MIN_BLOCK * 8, 64).unwrap());
    }
}
