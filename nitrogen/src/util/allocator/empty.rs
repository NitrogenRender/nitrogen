/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;

pub(crate) mod export {
    use super::*;

    pub(crate) type DefaultAlloc = AllocatorEmpty;
    pub(crate) type Buffer = BufferType<AllocatorEmpty>;
    pub(crate) type Image = ImageType<AllocatorEmpty>;
}

#[derive(Debug)]
pub(crate) struct BlockEmpty;

impl Block for BlockEmpty {
    fn range(&self) -> std::ops::Range<u64> {
        unimplemented!()
    }

    fn memory(&self) -> &crate::types::Memory {
        unimplemented!()
    }
}

#[derive(Debug)]
pub(crate) struct AllocatorEmpty;

impl AllocatorEmpty {
    pub(crate) unsafe fn new(
        _device: &back::Device,
        _props: gfx::adapter::MemoryProperties,
        _non_coherent_atom_size: usize,
    ) -> Self {
        AllocatorEmpty
    }
}

impl Allocator for AllocatorEmpty {
    type Block = BlockEmpty;

    unsafe fn alloc(
        &mut self,
        _: &back::Device,
        _: Request,
    ) -> Result<BlockEmpty, AllocationError> {
        unimplemented!("empty memory allocator used")
    }

    unsafe fn free(&mut self, _: &back::Device, _: BlockEmpty) {
        unimplemented!("empty memory allocator used")
    }

    unsafe fn dispose(self, _: &back::Device) -> Result<(), Self> {
        Ok(())
    }
}
