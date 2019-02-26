/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::borrow::Borrow;
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

/// Any type that implements this trait can be used to create and free elements
/// of a [`Pool<T>`]
pub(crate) trait PoolImpl<T> {
    fn new_elem(&mut self) -> T;
    fn reset_elem(&mut self, _elem: &mut T) {}
    fn free_elem(&mut self, elem: T);
    fn free_on_drop() -> bool {
        true
    }
}

pub(crate) struct PoolInner<T, Impl: PoolImpl<T>> {
    pool_impl: Impl,
    pub(crate) values: Vec<T>,
    // elements which are currently in use will be None
    // elements which are not used will point to the next free element
    //
    // that means to query for alive status the next field can be used.
    nexts: Vec<Option<usize>>,
    next_free: usize,
    size: usize,
}

/// A pool to insert (or allocate) items which can be reused after freeing.
///
/// This pool allocates a number of resources which are kept alive rather than destroying
/// and recreating resources. (like a slab would do)
pub(crate) struct Pool<T, Impl: PoolImpl<T>> {
    inner: UnsafeCell<PoolInner<T, Impl>>,
}

impl<T, Impl: PoolImpl<T>> Pool<T, Impl> {
    #[allow(unused)]
    pub(crate) fn new(back: Impl) -> Self {
        Pool::with_intial_elems(back, 0)
    }

    pub(crate) fn with_intial_elems(mut back: Impl, cap: usize) -> Self {
        let mut values = Vec::with_capacity(cap);
        for _ in 0..cap {
            values.push(back.new_elem());
        }

        let mut nexts = Vec::with_capacity(cap);

        for i in 0..cap {
            nexts.push(Some((i + 1) % cap));
        }

        let next_free = 0;

        Pool {
            inner: UnsafeCell::new(PoolInner {
                pool_impl: back,
                values,
                nexts,
                next_free,
                size: 0,
            }),
        }
    }

    pub(crate) unsafe fn get(&self) -> &mut PoolInner<T, Impl> {
        &mut *self.inner.get()
    }

    #[allow(unused)]
    pub(crate) fn len(&self) -> usize {
        unsafe { self.get().size }
    }

    pub(crate) fn alloc(&self) -> PoolElem<'_, Impl, T> {
        let this = unsafe { self.get() };

        let next = if this.size == this.values.len() {
            let next = this.values.len();
            this.values.push(this.pool_impl.new_elem());
            this.nexts.push(Some(this.next_free));
            next
        } else {
            this.next_free
        };

        this.next_free = this.nexts[next].unwrap();
        this.nexts[next] = None;

        this.size += 1;

        unsafe { PoolElem::from_idx(next, self) }
    }

    unsafe fn free_on_drop(&self, idx: usize) {
        if Impl::free_on_drop() {
            self.free(idx);
        }
    }

    unsafe fn free(&self, idx: usize) {
        let this = self.get();

        this.pool_impl.reset_elem(&mut this.values[idx]);

        this.nexts[idx] = Some(this.next_free);
        this.next_free = idx;

        this.size -= 1;
    }

    pub(crate) fn clear(&mut self) {
        let this = unsafe { self.get() };

        this.size = 0;

        for (i, val) in this.values.iter_mut().enumerate() {
            if this.nexts[i].is_none() {
                this.pool_impl.reset_elem(val);
            }
        }

        for i in 0..this.nexts.len() {
            this.nexts[i] = Some((i + 1) % this.nexts.len());
        }

        this.next_free = 0;
    }

    pub(crate) fn reset(&mut self) {
        let this = unsafe { self.get() };

        for val in this.values.drain(0..) {
            this.pool_impl.free_elem(val);
        }

        this.nexts.clear();
    }

    pub(crate) fn impl_ref_mut(&mut self) -> &mut Impl {
        let this = unsafe { self.get() };

        &mut this.pool_impl
    }

    pub(crate) unsafe fn into_impl(self) -> Impl {
        let this = self.inner.into_inner();
        this.pool_impl
    }
}

pub(crate) struct PoolElem<'a, Impl, T>
where
    Impl: PoolImpl<T>,
{
    idx: usize,
    pool: *mut (),
    _marker: PhantomData<(&'a T, Impl)>,
}

impl<'a, Impl, T> PoolElem<'a, Impl, T>
where
    Impl: PoolImpl<T>,
{
    #[allow(unused_unsafe)]
    pub(crate) unsafe fn into_idx(self) -> usize {
        use std::mem::forget;

        let idx = self.idx;

        forget(self);

        idx
    }

    pub(crate) unsafe fn from_idx(idx: usize, pool: &Pool<T, Impl>) -> Self {
        PoolElem {
            idx,
            pool: pool as *const _ as *mut _,
            _marker: PhantomData,
        }
    }
}

impl<'a, Impl, T> Drop for PoolElem<'a, Impl, T>
where
    Impl: PoolImpl<T>,
{
    fn drop(&mut self) {
        unsafe {
            let pool: &mut Pool<T, Impl> = &mut *(self.pool as *mut _);
            pool.free_on_drop(self.idx);
        }
    }
}

impl<'a, Impl, T> Deref for PoolElem<'a, Impl, T>
where
    Impl: PoolImpl<T>,
{
    type Target = T;

    fn deref(&self) -> &<Self as Deref>::Target {
        unsafe {
            let pool: &Pool<T, Impl> = &*(self.pool as *const _);
            &pool.get().values[self.idx]
        }
    }
}

impl<'a, Impl, T> DerefMut for PoolElem<'a, Impl, T>
where
    Impl: PoolImpl<T>,
{
    fn deref_mut(&mut self) -> &mut <Self as Deref>::Target {
        unsafe {
            let pool: &mut Pool<T, Impl> = &mut *(self.pool as *mut _);
            &mut pool.get().values[self.idx]
        }
    }
}

impl<'a, Impl, T> Borrow<T> for PoolElem<'a, Impl, T>
where
    Impl: PoolImpl<T>,
{
    fn borrow(&self) -> &T {
        &*self
    }
}

#[cfg(test)]
mod test {
    use super::*;

    struct NumImpl;

    impl PoolImpl<usize> for NumImpl {
        fn new_elem(&mut self) -> usize {
            0
        }

        fn reset_elem(&mut self, elem: &mut usize) {
            *elem = 0;
        }

        fn free_elem(&mut self, _elem: usize) {}
    }

    #[test]
    fn alloc() {
        let pool = Pool::with_intial_elems(NumImpl, 1);
        assert_eq!(pool.len(), 0);

        let mut entry = pool.alloc();
        assert_eq!(pool.len(), 1);

        assert_eq!(*entry, 0);

        *entry = 1;

        assert_eq!(*entry, 1);
    }

    #[test]
    fn reuse() {
        let pool = Pool::with_intial_elems(NumImpl, 1);
        assert_eq!(pool.len(), 0);

        {
            let mut entry = pool.alloc();
            assert_eq!(pool.len(), 1);

            assert_eq!(*entry, 0);
            *entry += 1;
            assert_eq!(*entry, 1);
        }

        {
            let mut entry = pool.alloc();
            assert_eq!(pool.len(), 1);

            assert_eq!(*entry, 0);
            *entry += 1;
            assert_eq!(*entry, 1);
        }
    }

    #[test]
    fn new_alloc() {
        let pool = Pool::new(NumImpl);

        let mut entry = pool.alloc();

        assert_eq!(*entry, 0);
        *entry += 1;
        assert_eq!(*entry, 1);
    }

    #[test]
    fn grow_a_lot_new() {
        use std::mem::forget;

        let pool = Pool::new(NumImpl);

        for _ in 0..1000 {
            let entry = pool.alloc();
            forget(entry);
        }

        assert_eq!(pool.len(), 1000);
    }

    #[test]
    fn grow_a_lot_cap() {
        use std::mem::forget;

        let pool = Pool::with_intial_elems(NumImpl, 1000);

        for _ in 0..1000 {
            let entry = pool.alloc();
            forget(entry);
        }

        assert_eq!(pool.len(), 1000);
    }

    #[test]
    fn clear() {
        use std::mem::forget;

        let mut pool = Pool::with_intial_elems(NumImpl, 1000);

        for _ in 0..1000 {
            let entry = pool.alloc();
            forget(entry);
        }

        assert_eq!(pool.len(), 1000);

        pool.clear();

        assert_eq!(pool.len(), 0);
    }

}
