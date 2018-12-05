/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx;

use gfx::Device;
use gfx::Instance;

use gfxm::MemoryAllocator;
use gfxm::SmartAllocator;

use back;

use std::sync::{Arc, Mutex, MutexGuard};

#[repr(u8)]
pub enum QueueType {
    Rendering,
    ImageStorage,
}

pub struct DeviceContext {
    pub memory_allocator: Mutex<SmartAllocator<back::Backend>>,

    pub queue_group: Mutex<gfx::QueueGroup<back::Backend, gfx::General>>,

    pub device: Arc<back::Device>,
    pub adapter: Arc<gfx::Adapter<back::Backend>>,
}

impl DeviceContext {
    pub fn new(instance: &back::Instance) -> Self {
        use gfx::PhysicalDevice;

        let mut adapters = instance.enumerate_adapters();

        // TODO select best fitting adapter
        let adapter = adapters.remove(0);

        let memory_properties = adapter.physical_device.memory_properties();
        let memory_allocator =
            SmartAllocator::new(memory_properties, 256, 64, 1024, 256 * 1024 * 1024);

        //        TODO better queue handling than just requiring a `General` queue.
        //
        //        let (device, queues) = {
        //            let mut display_queues = vec![];
        //            let mut render_queues = vec![];
        //            let mut compute_queues = vec![];
        //
        //            adapter
        //                .queue_families
        //                .iter()
        //                .enumerate()
        //                .for_each(|(idx, family)| {
        //                    for surface in surfaces {
        //                        if surface.supports_queue_family(family) {
        //                            display_queues.push(family);
        //                        }
        //                    }
        //
        //                    if family.supports_graphics() {
        //                        render_queues.push(family);
        //                    }
        //                    if family.supports_compute() {
        //                        compute_queues.push(family);
        //                    }
        //                });
        //
        //            let queue_for_display = display_queues[0]; // TODO have proper selection?
        //            let queue_for_rendering = render_queues[0]; // TODO have a proper selection?
        //            let queue_for_compute = compute_queues[0]; // TODO have proper selection?
        //
        //            // deduplicate queues
        //            let mut families = [
        //                queue_for_rendering,
        //                queue_for_display,
        //                queue_for_compute,
        //            ];
        //
        //            let families = families
        //                .iter()
        //                .map(|fam| (*fam, &[1.0; 1][..]))
        //                .collect::<Vec<_>>();
        //
        //            let mut gpu: gfx::Gpu<back::Backend> = adapter
        //                .physical_device
        //                .open(&families[..])
        //                .expect("Can't open device");
        //
        //            let render_queue = gpu.queues.take::<gfx::Graphics>(queue_for_rendering.id());
        //            let display_queue = gpu.queues.take::<gfx::Graphics>(queue_for_display.id());
        //            let compute_queue = gpu.queues.take::<gfx::Compute>(queue_for_compute.id());
        //
        //            (gpu.device, gpu.queues)
        //        };

        let (device, queue_group) = adapter
            .open_with(1, |family| {
                // surfaces.iter().fold(true, |acc, surface| {
                //     surface.supports_queue_family(family) && acc
                // })
                use gfx::QueueFamily;
                family.supports_graphics() && family.supports_compute()
            })
            .unwrap();

        DeviceContext {
            memory_allocator: Mutex::new(memory_allocator),

            queue_group: Mutex::new(queue_group),

            device: Arc::new(device),
            adapter: Arc::new(adapter),
        }
    }

    pub fn allocator(&self) -> MutexGuard<SmartAllocator<back::Backend>> {
        // if we can't access the device-local memory allocator then ... well, RIP
        self.memory_allocator
            .lock()
            .expect("Memory allocator can't be accessed")
    }

    pub fn queue_group(&self) -> MutexGuard<gfx::QueueGroup<back::Backend, gfx::General>> {
        self.queue_group.lock().unwrap()
    }

    pub fn release(self) {
        self.memory_allocator
            .into_inner()
            .unwrap()
            .dispose(&self.device)
            .unwrap();
        self.device.wait_idle().unwrap();
    }
}
