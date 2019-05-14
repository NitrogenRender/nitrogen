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

#[derive(Debug)]
pub(crate) struct AllocatorRendy;

impl AllocatorRendy {
    pub(crate) unsafe fn new(
        _device: &back::Device,
        _props: gfx::adapter::MemoryProperties,
        _non_coherent_atom_size: usize,
    ) -> Self {
        AllocatorRendy
    }
}

impl Allocator for AllocatorRendy {
    type Block = Block;

    unsafe fn alloc(&mut self, _: &back::Device, _: Request) -> Result<Block, AllocationError> {
        unimplemented!("empty memory allocator used")
    }

    unsafe fn free(&mut self, _: &back::Device, _: Block) {
        unimplemented!("empty memory allocator used")
    }

    unsafe fn dispose(self, _: &back::Device) -> Result<(), Self> {
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct Block;

impl super::Block for Block {
    fn range(&self) -> std::ops::Range<u64> {
        unimplemented!()
    }

    fn memory(&self) -> &crate::types::Memory {
        unimplemented!()
    }
}
