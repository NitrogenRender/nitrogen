/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;

use gfx_memory::{Block as GfxmBlock, MemoryAllocator, SmartAllocator, SmartBlock};

pub(crate) mod export {
    use super::*;

    pub(crate) type DefaultAlloc = AllocatorGfxm;
    pub(crate) type Buffer = BufferType<AllocatorGfxm>;
    pub(crate) type Image = ImageType<AllocatorGfxm>;
}

#[derive(Debug)]
pub struct BlockGfxm {
    block: SmartBlock<crate::types::Memory>,
}

impl Block for BlockGfxm {
    fn range(&self) -> std::ops::Range<u64> {
        self.block.range()
    }

    fn memory(&self) -> &crate::types::Memory {
        self.block.memory()
    }
}

#[derive(Debug)]
pub(crate) struct AllocatorGfxm {
    alloc: SmartAllocator<back::Backend>,
    atom_size: usize,
}

fn gfxm_err_to_alloc_err(err: gfx_memory::MemoryError) -> AllocationError {
    eprintln!("Encountered an error! {:?}", err);

    use gfx_memory::MemoryError;
    match err {
        MemoryError::NoCompatibleMemoryType => AllocationError::NoSuitableMemoryType,
        MemoryError::OutOfMemory => AllocationError::OutOfMemory,
        MemoryError::TooManyObjects => AllocationError::TooManyObjects,
    }
}

impl AllocatorGfxm {
    pub(crate) unsafe fn new(
        _device: &back::Device,
        props: gfx::adapter::MemoryProperties,
        non_coherent_atom_size: usize,
    ) -> Self {
        let alloc = SmartAllocator::new(props, 256, 64, 1024, 256 * 1024 * 1024);
        Self {
            alloc,
            atom_size: non_coherent_atom_size,
        }
    }
}

impl Allocator for AllocatorGfxm {
    type Block = BlockGfxm;

    unsafe fn alloc(
        &mut self,
        device: &back::Device,
        request: Request,
    ) -> Result<BlockGfxm, AllocationError> {
        let ty = if request.transient {
            gfx_memory::Type::ShortLived
        } else {
            gfx_memory::Type::General
        };

        let prop = request.properties;

        let padding = {
            let modulus = request.size % (self.atom_size as u64);
            if modulus != 0 {
                (self.atom_size as u64) - modulus
            } else {
                0
            }
        };

        let size = request.size + padding;

        let reqs = gfx::memory::Requirements {
            size,
            alignment: request.alignment.max(self.atom_size as u64),
            type_mask: request.type_mask,
        };

        let block = self
            .alloc
            .alloc(device, (ty, prop), reqs)
            .map_err(gfxm_err_to_alloc_err)?;

        Ok(BlockGfxm { block })
    }

    unsafe fn free(&mut self, device: &back::Device, block: BlockGfxm) {
        self.alloc.free(device, block.block)
    }

    unsafe fn dispose(self, device: &back::Device) -> Result<(), Self> {
        let atom_size = self.atom_size;
        self.alloc
            .dispose(device)
            .map_err(|alloc| Self { alloc, atom_size })
    }
}
