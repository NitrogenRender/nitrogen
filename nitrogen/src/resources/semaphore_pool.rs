/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx;

use std::sync::Arc;

use util::pool::{Pool, PoolImpl, PoolElem};
use device::DeviceContext;
use types;

use smallvec::SmallVec;


pub type Semaphore<'a> = PoolElem<'a, SemaphorePoolImpl, types::Semaphore>;
pub struct SemaphorePool(pub(crate) Pool<types::Semaphore, SemaphorePoolImpl>);

impl SemaphorePool {
    pub fn new(device: Arc<DeviceContext>) -> Self {
        Self::with_capacity(device, 0)
    }

    pub fn with_capacity(device: Arc<DeviceContext>, cap: usize) -> Self {
        SemaphorePool(Pool::with_intial_elems(SemaphorePoolImpl { device }, cap))
    }

    pub fn alloc(&self) -> Semaphore<'_> {
        self.0.alloc()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn cap(&self) -> usize {
        self.0.cap()
    }

    pub fn list_prev_sems<'a>(&'a self, list: &'a SemaphoreList) -> Box<dyn Iterator<Item = &'a types::Semaphore> + 'a> {
        Box::new(list.prev_semaphores.as_slice()
            .iter()
            .map(move |idx| {
                let this = unsafe { self.0.get() };
                &this.values[*idx]
            }))
    }

    pub fn list_next_sems<'a>(&'a self, list: &'a SemaphoreList) -> Box<dyn Iterator<Item = &'a types::Semaphore> + 'a> {
        Box::new(list.next_semaphores.as_slice()
            .iter()
            .map(move |idx| {
                let this = unsafe { self.0.get() };
                &this.values[*idx]
            }))
    }

    pub fn clear(&mut self) {
        self.0.clear()
    }

    pub fn reset(&mut self) {
        self.0.reset()
    }
}

pub struct SemaphorePoolImpl {
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

pub struct SemaphoreList {
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

    pub fn add_prev_semaphore(&mut self, sem: Semaphore<'_>) {
        self.prev_semaphores.push(unsafe { sem.into_idx() });
    }

    pub fn add_next_semaphore(&mut self, sem: Semaphore<'_>) {
        self.next_semaphores.push(unsafe { sem.into_idx() });
    }

    pub fn advance(&mut self) {
        self.prev_semaphores.clear();
        self.prev_semaphores.extend(self.next_semaphores.iter().cloned());
        self.next_semaphores.clear();
    }
}
