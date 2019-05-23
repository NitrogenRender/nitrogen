/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use rendy_memory::{DynamicConfig, Heaps, HeapsConfig, LinearConfig, MemoryUsage};

pub(crate) type Block = rendy_memory::MemoryBlock<back::Backend>;

pub type AllocationError = rendy_memory::HeapsError;

#[derive(Debug, Display, From)]
pub enum AllocatorError {
    #[display(fmt = "Could not allocate memory")]
    AllocationError(AllocationError),
    #[display(fmt = "Could not bind resource")]
    BindError(gfx::device::BindError),
    #[display(fmt = "Could not create buffer")]
    BufferCreationError(gfx::buffer::CreationError),
    #[display(fmt = "Could not create image")]
    ImageCreationError(gfx::image::CreationError),
}

/// An allocation request
#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) transient: bool,
    pub(crate) properties: gfx::memory::Properties,
    pub(crate) size: u64,
    pub(crate) alignment: u64,
    pub(crate) type_mask: u64,
}

#[derive(Debug)]
pub(crate) struct BufferRequest {
    pub(crate) transient: bool,
    pub(crate) properties: gfx::memory::Properties,
    pub(crate) usage: gfx::buffer::Usage,
    pub(crate) size: u64,
}

#[derive(Debug)]
pub(crate) struct ImageRequest {
    pub(crate) transient: bool,
    pub(crate) properties: gfx::memory::Properties,
    pub(crate) kind: gfx::image::Kind,
    pub(crate) level: gfx::image::Level,
    pub(crate) format: gfx::format::Format,
    pub(crate) tiling: gfx::image::Tiling,
    pub(crate) usage: gfx::image::Usage,
    pub(crate) view_caps: gfx::image::ViewCapabilities,
}

#[derive(Debug)]
pub(crate) struct Buffer {
    buffer: crate::types::Buffer,
    block: Block,
}

impl Buffer {
    pub(crate) fn raw(&self) -> &crate::types::Buffer {
        &self.buffer
    }

    pub(crate) fn block_mut(&mut self) -> &mut Block {
        &mut self.block
    }
}

pub(crate) struct Image {
    image: crate::types::Image,
    block: Block,
}

impl Image {
    pub(crate) fn raw(&self) -> &crate::types::Image {
        &self.image
    }

    #[allow(unused)]
    pub(crate) fn block(&self) -> &Block {
        &self.block
    }
}

pub struct Allocator {
    heaps: Heaps<back::Backend>,
    atom_size: usize,
}

impl Allocator {
    pub(crate) unsafe fn new(
        _device: &back::Device,
        props: gfx::adapter::MemoryProperties,
        non_coherent_atom_size: usize,
    ) -> Self {
        let types = {
            let _1mb = 1024 * 1024;
            let _32mb = 32 * _1mb;
            let _128mb = 128 * _1mb;

            props
                .memory_types
                .iter()
                .map(|mt| {
                    let config = HeapsConfig {
                        linear: if mt.properties.contains(gfx::memory::Properties::CPU_VISIBLE) {
                            Some(LinearConfig {
                                linear_size: _128mb.min(props.memory_heaps[mt.heap_index] / 16),
                            })
                        } else {
                            None
                        },
                        dynamic: Some(DynamicConfig {
                            block_size_granularity: 256.min(
                                (props.memory_heaps[mt.heap_index] / 4096).next_power_of_two(),
                            ),
                            min_device_allocation: _1mb
                                .min(props.memory_heaps[mt.heap_index] / 1048)
                                .next_power_of_two(),
                            max_chunk_size: _32mb
                                .min((props.memory_heaps[mt.heap_index] / 128).next_power_of_two()),
                        }),
                    };

                    (mt.properties, mt.heap_index as u32, config)
                })
                .collect::<Vec<_>>()
        };

        let heaps = Heaps::new(types, props.memory_heaps);

        Allocator {
            heaps,
            atom_size: non_coherent_atom_size,
        }
    }

    pub(crate) unsafe fn alloc(
        &mut self,
        device: &back::Device,
        req: Request,
    ) -> Result<Block, rendy_memory::HeapsError> {
        let padding = {
            let modulus = req.size % (self.atom_size as u64);
            if modulus != 0 {
                (self.atom_size as u64) - modulus
            } else {
                0
            }
        };

        let size = req.size + padding;
        let align = req.alignment.max(self.atom_size as u64);
        let mask = req.type_mask;

        let usage = RendyRequest(req);

        let mem = self
            .heaps
            .allocate(device, mask as u32, usage, size, align)?;

        Ok(mem)
    }

    pub(crate) unsafe fn free(&mut self, device: &back::Device, block: Block) {
        self.heaps.free(device, block)
    }

    pub(crate) unsafe fn create_buffer(
        &mut self,
        device: &back::Device,
        request: BufferRequest,
    ) -> Result<Buffer, AllocatorError> {
        use gfx::Device;
        use rendy_memory::Block as _;

        let mut buf = device.create_buffer(request.size, request.usage)?;
        let reqs = device.get_buffer_requirements(&buf);

        let request = Request {
            size: reqs.size,
            alignment: reqs.alignment,
            type_mask: reqs.type_mask,
            properties: request.properties,
            transient: request.transient,
        };

        let block = self.alloc(device, request)?;

        device.bind_buffer_memory(block.memory(), block.range().start, &mut buf)?;

        Ok(Buffer { buffer: buf, block })
    }

    pub(crate) unsafe fn create_image(
        &mut self,
        device: &back::Device,
        request: ImageRequest,
    ) -> Result<Image, AllocatorError> {
        use gfx::Device;
        use rendy_memory::Block as _;

        let mut img = device.create_image(
            request.kind,
            request.level,
            request.format,
            request.tiling,
            request.usage,
            request.view_caps,
        )?;

        let reqs = device.get_image_requirements(&img);

        let request = Request {
            size: reqs.size,
            alignment: reqs.alignment,
            type_mask: reqs.type_mask,
            properties: request.properties,
            transient: request.transient,
        };

        let block = self.alloc(device, request)?;

        device.bind_image_memory(block.memory(), block.range().start, &mut img)?;

        Ok(Image { image: img, block })
    }

    pub(crate) unsafe fn destroy_buffer(&mut self, device: &back::Device, buffer: Buffer) {
        use gfx::Device;
        device.destroy_buffer(buffer.buffer);
        self.free(device, buffer.block);
    }

    pub(crate) unsafe fn destroy_image(&mut self, device: &back::Device, image: Image) {
        use gfx::Device;
        device.destroy_image(image.image);
        self.free(device, image.block);
    }

    pub(crate) unsafe fn dispose(self, device: &back::Device) {
        self.heaps.dispose(device)
    }
}

#[derive(Debug)]
struct RendyRequest(Request);

impl MemoryUsage for RendyRequest {
    fn properties_required(&self) -> gfx::memory::Properties {
        self.0.properties
    }

    fn memory_fitness(&self, properties: gfx::memory::Properties) -> u32 {
        let req = &self.0;

        let is_big = self.0.size >= 2 * 1024 * 1024;

        if req.transient {
            assert!(properties.contains(gfx::memory::Properties::CPU_VISIBLE));
            assert!(!properties.contains(gfx::memory::Properties::LAZILY_ALLOCATED));

            0 | ((!properties.contains(gfx::memory::Properties::DEVICE_LOCAL)) as u32) << 2
                | (properties.contains(gfx::memory::Properties::COHERENT) as u32) << 1
                | ((!properties.contains(gfx::memory::Properties::CPU_CACHED)) as u32) << 0
        } else if is_big {
            // dedicated
            assert!(properties.contains(gfx::memory::Properties::DEVICE_LOCAL));
            0 | ((!properties.contains(gfx::memory::Properties::CPU_VISIBLE)) as u32) << 3
                | ((!properties.contains(gfx::memory::Properties::LAZILY_ALLOCATED)) as u32) << 2
                | ((!properties.contains(gfx::memory::Properties::CPU_CACHED)) as u32) << 1
                | ((!properties.contains(gfx::memory::Properties::COHERENT)) as u32) << 0
        } else {
            // dynamic
            assert!(!properties.contains(gfx::memory::Properties::LAZILY_ALLOCATED));

            0 | (properties.contains(gfx::memory::Properties::DEVICE_LOCAL) as u32) << 2
                | (properties.contains(gfx::memory::Properties::COHERENT) as u32) << 1
                | ((!properties.contains(gfx::memory::Properties::CPU_CACHED)) as u32) << 0
        }
    }

    fn allocator_fitness(&self, kind: rendy_memory::Kind) -> u32 {
        let is_big = self.0.size >= 2 * 1024 * 1024;

        if self.0.transient {
            match kind {
                rendy_memory::Kind::Dedicated => 0,
                rendy_memory::Kind::Dynamic => 1,
                rendy_memory::Kind::Linear => 2,
            }
        } else if is_big {
            // dedicated
            match kind {
                rendy_memory::Kind::Dedicated => 2,
                rendy_memory::Kind::Dynamic => 1,
                rendy_memory::Kind::Linear => 0,
            }
        } else {
            // dynamic
            match kind {
                rendy_memory::Kind::Dedicated => 1,
                rendy_memory::Kind::Dynamic => 2,
                rendy_memory::Kind::Linear => 0,
            }
        }
    }
}
