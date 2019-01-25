/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx::Device;
use gfx::Instance;

use crate::types;
use crate::util::allocator::{Allocator, DefaultAlloc};

use smallvec::SmallVec;

use std::sync::{Arc, Mutex, MutexGuard};

pub(crate) struct DeviceContext {
    pub(crate) memory_allocator: Mutex<DefaultAlloc>,

    pub(crate) graphics_queue_idx: usize,
    pub(crate) compute_queue_idx: usize,
    pub(crate) queue_groups: SmallVec<[types::QueueGroup<gfx::Transfer>; 2]>,
    pub(crate) queues: SmallVec<[Vec<Mutex<types::CommandQueue<gfx::Transfer>>>; 2]>,

    pub(crate) device: Arc<back::Device>,
    pub(crate) adapter: Arc<gfx::Adapter<back::Backend>>,
}

impl DeviceContext {
    pub(crate) unsafe fn new(instance: &back::Instance) -> Self {
        use gfx::PhysicalDevice;
        use std::mem::replace;

        let mut adapters = instance.enumerate_adapters();

        // TODO select best fitting adapter
        let adapter = adapters.remove(0);

        let (device, mut queue_groups, graphics_idx, compute_idx) = {
            use gfx::QueueFamily;

            let graphics_queue = adapter
                .queue_families
                .iter()
                .filter(|fam| fam.supports_graphics())
                .max_by_key(|fam| fam.max_queues())
                .expect("No suitable graphics queue available");

            let compute_queue = adapter
                .queue_families
                .iter()
                .filter(|fam| fam.supports_compute())
                .max_by_key(|fam| fam.max_queues())
                .expect("No suitable compute queue available");

            // create device, for that we need a list of all the queues we want to be created.
            let queues: &[(_, &[_])] = &[(graphics_queue, &[1.0]), (compute_queue, &[1.0])];

            // Here we only have two queues we found out are compatible.
            // But those things might point to the same queue! Unfortunately Vulkan doesn't like
            // when you ask it to create two queues which are actually the same queue.
            // Because of that we have to de-duplicate the queue list. Fortunately here we only have
            // two things that could alias each other, so when they are the same we just ignore one
            // of them.
            let end = if graphics_queue.id() == compute_queue.id() {
                1
            } else {
                2
            };

            let mut gpu = adapter
                .physical_device
                .open(&queues[0..end])
                .expect("Can't create logical device");

            let mut queues = SmallVec::new();
            queues.push(gpu.queues.take(graphics_queue.id()).unwrap());

            let graphics_idx = 0;

            let compute_idx = if compute_queue.id() != graphics_queue.id() {
                queues.push(gpu.queues.take(compute_queue.id()).unwrap());

                1
            } else {
                0
            };

            (gpu.device, queues, graphics_idx, compute_idx)
        };

        let queues = queue_groups
            .as_mut_slice()
            .iter_mut()
            .map(|group: &mut gfx::QueueGroup<_, _>| {
                let queues = replace(&mut group.queues, vec![]);

                queues.into_iter().map(|queue| Mutex::new(queue)).collect()
            })
            .collect();

        let memory_properties = adapter.physical_device.memory_properties();

        let coherent_atom_size = adapter.physical_device.limits().non_coherent_atom_size;

        let memory_allocator = DefaultAlloc::new(&device, memory_properties, coherent_atom_size);

        DeviceContext {
            memory_allocator: Mutex::new(memory_allocator),

            graphics_queue_idx: graphics_idx,
            compute_queue_idx: compute_idx,
            queue_groups,
            queues,

            device: Arc::new(device),
            adapter: Arc::new(adapter),
        }
    }

    pub(crate) fn allocator(&self) -> MutexGuard<DefaultAlloc> {
        // if we can't access the device-local memory allocator then ... well, RIP
        self.memory_allocator
            .lock()
            .expect("Memory allocator can't be accessed")
    }

    pub(crate) fn graphics_queue_group(&self) -> &types::QueueGroup<gfx::Graphics> {
        use std::mem::transmute;

        let queue = &self.queue_groups[self.graphics_queue_idx];

        unsafe { transmute(queue) }
    }

    pub(crate) fn graphics_queue(&self) -> MutexGuard<types::CommandQueue<gfx::Graphics>> {
        use std::mem::transmute;
        unsafe { transmute(self.queues[self.graphics_queue_idx][0].lock().unwrap()) }
    }

    pub(crate) fn compute_queue_group(&self) -> &types::QueueGroup<gfx::Compute> {
        use std::mem::transmute;

        let queue = &self.queue_groups[self.compute_queue_idx];

        unsafe { transmute(queue) }
    }

    pub(crate) fn compute_queue(&self) -> MutexGuard<types::CommandQueue<gfx::Compute>> {
        use std::mem::transmute;
        unsafe { transmute(self.queues[self.compute_queue_idx][0].lock().unwrap()) }
    }

    pub(crate) fn transfer_queue_group(&self) -> &types::QueueGroup<gfx::Transfer> {
        use std::mem::transmute;

        // TODO find the "best" queue to use.
        let queue = &self.queue_groups[self.compute_queue_idx];

        unsafe { transmute(queue) }
    }

    pub(crate) fn transfer_queue(&self) -> MutexGuard<types::CommandQueue<gfx::Transfer>> {
        use std::mem::transmute;
        unsafe { transmute(self.queues[self.compute_queue_idx][0].lock().unwrap()) }
    }

    pub(crate) unsafe fn release(self) {
        self.memory_allocator
            .into_inner()
            .unwrap()
            .dispose(&self.device)
            .unwrap();
        self.device.wait_idle().unwrap();
    }
}
