//! Requested Rust heap bytes, including allocations made before helper activation.
//! Native library allocations are not covered; macOS whole-process qualification remains open.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};
pub struct Bounded {
    used: AtomicUsize,
    limit: AtomicUsize,
}
impl Bounded {
    pub const fn new() -> Self {
        Self {
            used: AtomicUsize::new(0),
            limit: AtomicUsize::new(usize::MAX),
        }
    }
    pub fn limit(&self, bytes: usize) -> Result<(), String> {
        self.limit.store(bytes, Ordering::SeqCst);
        if self.used.load(Ordering::SeqCst) > bytes {
            Err("Script heap already exceeds its allocation budget.".into())
        } else {
            Ok(())
        }
    }
    fn reserve(&self, bytes: usize) -> bool {
        self.used
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |used| {
                used.checked_add(bytes)
                    .filter(|next| *next <= self.limit.load(Ordering::SeqCst))
            })
            .is_ok()
    }
    fn release(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::SeqCst);
    }
}
// Only allocator bookkeeping runs here: no allocations, locks or unwinding.
unsafe impl GlobalAlloc for Bounded {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !self.reserve(layout.size()) {
            return std::ptr::null_mut();
        }
        let pointer = System.alloc(layout);
        if pointer.is_null() {
            self.release(layout.size());
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !self.reserve(layout.size()) {
            return std::ptr::null_mut();
        }
        let pointer = System.alloc_zeroed(layout);
        if pointer.is_null() {
            self.release(layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
        self.release(layout.size());
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let growth = size.saturating_sub(layout.size());
        if !self.reserve(growth) {
            return std::ptr::null_mut();
        }
        let next = System.realloc(pointer, layout, size);
        if next.is_null() {
            self.release(growth);
        } else {
            self.release(layout.size().saturating_sub(size));
        }
        next
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocations_growth_shrink_and_release_obey_budget() {
        let heap = Bounded::new();
        heap.limit(128).unwrap();
        let small = Layout::from_size_align(32, 8).unwrap();
        let large = Layout::from_size_align(256, 8).unwrap();
        unsafe {
            assert!(heap.alloc(large).is_null());
            assert!(heap.alloc_zeroed(large).is_null());
            let pointer = heap.alloc_zeroed(small);
            assert!(!pointer.is_null());
            assert_eq!(*pointer, 0);
            assert!(heap.realloc(pointer, small, 129).is_null());
            let pointer = heap.realloc(pointer, small, 64);
            assert!(!pointer.is_null());
            let pointer = heap.realloc(pointer, Layout::from_size_align(64, 8).unwrap(), 16);
            assert!(!pointer.is_null());
            heap.dealloc(pointer, Layout::from_size_align(16, 8).unwrap());
            let pointer = heap.alloc(small);
            assert!(!pointer.is_null());
            heap.dealloc(pointer, small);
        }
        assert_eq!(heap.used.load(Ordering::SeqCst), 0);
        assert!(heap.reserve(128));
        assert!(!heap.reserve(usize::MAX));
        assert!(heap.limit(64).is_err());
        heap.release(128);
    }
}
