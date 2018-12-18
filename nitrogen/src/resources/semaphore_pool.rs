/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::sync::Arc;

use crate::device::DeviceContext;
use crate::types;
use crate::util::pool::{Pool, PoolElem, PoolImpl};

use smallvec::SmallVec;

pub(crate) type Semaphore<'a> = PoolElem<'a, SemaphorePoolImpl, types::Semaphore>;
pub(crate) struct SemaphorePool(pub(crate) Pool<types::Semaphore, SemaphorePoolImpl>);

impl SemaphorePool {
    pub(crate) fn new(device: Arc<DeviceContext>) -> Self {
        Self::with_capacity(device, 0)
    }

    pub(crate) fn with_capacity(device: Arc<DeviceContext>, cap: usize) -> Self {
        SemaphorePool(Pool::with_intial_elems(SemaphorePoolImpl { device }, cap))
    }

    pub(crate) fn alloc(&self) -> Semaphore<'_> {
        self.0.alloc()
    }

    pub(crate) fn list_prev_sems<'a>(
        &'a self,
        list: &'a SemaphoreList,
    ) -> Box<dyn Iterator<Item = &'a types::Semaphore> + 'a> {
        Box::new(list.prev_semaphores.as_slice().iter().map(move |idx| {
            let this = unsafe { self.0.get() };
            &this.values[*idx]
        }))
    }

    pub(crate) fn list_next_sems<'a>(
        &'a self,
        list: &'a SemaphoreList,
    ) -> Box<dyn Iterator<Item = &'a types::Semaphore> + 'a> {
        Box::new(list.next_semaphores.as_slice().iter().map(move |idx| {
            let this = unsafe { self.0.get() };
            &this.values[*idx]
        }))
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear()
    }

    pub(crate) fn reset(&mut self) {
        self.0.reset()
    }
}

pub(crate) struct SemaphorePoolImpl {
    device: Arc<DeviceContext>,
}

impl PoolImpl<types::Semaphore> for SemaphorePoolImpl {
    fn new_elem(&mut self) -> types::Semaphore {
        use gfx::Device;
        self.device.device.create_semaphore().unwrap()
    }

    fn free_elem(&mut self, elem: types::Semaphore) {
        use gfx::Device;
        self.device.device.destroy_semaphore(elem);
    }
}

pub(crate) struct SemaphoreList {
    prev_semaphores: SmallVec<[usize; 6]>,
    next_semaphores: SmallVec<[usize; 6]>,
}

impl SemaphoreList {
    pub(crate) fn new() -> Self {
        Self {
            prev_semaphores: SmallVec::new(),
            next_semaphores: SmallVec::new(),
        }
    }

    pub(crate) fn add_prev_semaphore(&mut self, sem: Semaphore<'_>) {
        self.prev_semaphores.push(unsafe { sem.into_idx() });
    }

    pub(crate) fn add_next_semaphore(&mut self, sem: Semaphore<'_>) {
        self.next_semaphores.push(unsafe { sem.into_idx() });
    }

    pub(crate) fn advance(&mut self) {
        self.prev_semaphores.clear();
        self.prev_semaphores
            .extend(self.next_semaphores.iter().cloned());
        self.next_semaphores.clear();
    }
}
