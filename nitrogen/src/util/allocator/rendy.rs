/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

pub(crate) mod export {
    use super::*;

    pub(crate) type DefaultAlloc = AllocatorRendy;
    pub(crate) type Buffer = BufferType<AllocatorRendy>;
    pub(crate) type Image = ImageType<AllocatorRendy>;
}

use super::*;
use rendy_memory::{DynamicConfig, Heaps, HeapsConfig, LinearConfig, MemoryUsage};

#[derive(Debug)]
pub(crate) struct AllocatorRendy {
    heaps: Heaps<back::Backend>,
    atom_size: usize,
}

impl AllocatorRendy {
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

        AllocatorRendy {
            heaps,
            atom_size: non_coherent_atom_size,
        }
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

impl Allocator for AllocatorRendy {
    type Block = Block;

    unsafe fn alloc(
        &mut self,
        device: &back::Device,
        req: Request,
    ) -> Result<Block, AllocationError> {
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

        let usage = RendyRequest(req);

        let mem = self.heaps.allocate(device, !0, usage, size, align);

        let block = match mem {
            Ok(block) => block,
            Err(err) => {
                let error = match err {
                    rendy_memory::HeapsError::AllocationError(err) => match err {
                        gfx::device::AllocationError::OutOfMemory(_) => {
                            AllocationError::OutOfMemory
                        }
                        gfx::device::AllocationError::TooManyObjects => {
                            AllocationError::TooManyObjects
                        }
                    },
                    rendy_memory::HeapsError::NoSuitableMemory(_, _) => {
                        AllocationError::NoSuitableMemoryType
                    }
                };

                return Err(error);
            }
        };

        Ok(Block { block })
    }

    unsafe fn free(&mut self, device: &back::Device, block: Block) {
        self.heaps.free(device, block.block)
    }

    unsafe fn dispose(self, device: &back::Device) -> Result<(), Self> {
        self.heaps.dispose(device);
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct Block {
    block: rendy_memory::MemoryBlock<back::Backend>,
}

impl super::Block for Block {
    fn range(&self) -> std::ops::Range<u64> {
        use rendy_memory::Block;

        self.block.range()
    }

    fn memory(&self) -> &crate::types::Memory {
        use rendy_memory::Block;

        self.block.memory()
    }
}
