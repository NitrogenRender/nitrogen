/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use failure::Fail;

/// The returned object representing a memory allocation
pub(crate) trait Block: Sized {
    fn range(&self) -> std::ops::Range<u64>;
    fn memory(&self) -> &crate::types::Memory;
}

#[derive(Debug)]
pub(crate) struct BufferType<A: Allocator> {
    buffer: crate::types::Buffer,
    block: A::Block,
}

impl<A: Allocator> BufferType<A> {
    pub(crate) fn raw(&self) -> &crate::types::Buffer {
        &self.buffer
    }

    pub(crate) fn block(&self) -> &A::Block {
        &self.block
    }
}

pub(crate) struct ImageType<A: Allocator> {
    image: crate::types::Image,
    block: A::Block,
}

impl<A: Allocator> ImageType<A> {
    pub(crate) fn raw(&self) -> &crate::types::Image {
        &self.image
    }

    #[allow(unused)]
    pub(crate) fn block(&self) -> &A::Block {
        &self.block
    }
}

/// Errors that can occur when allocating memory from a device
#[derive(Debug, Fail, Clone)]
pub enum AllocationError {
    /// No suitable memory type could be found
    #[fail(display = "No suitable memory type could be found")]
    NoSuitableMemoryType,

    /// Not enough free memory available to perform the allocation
    #[fail(display = "Out of memory")]
    OutOfMemory,

    /// An implementation might impose a limited number of objects
    #[fail(display = "Too many objects")]
    TooManyObjects,
}

#[derive(Debug, Fail, Clone)]
pub enum AllocatorError {
    #[fail(display = "Could not allocate memory")]
    AllocationError(#[cause] AllocationError),
    #[fail(display = "Could not bind resource")]
    BindError(#[cause] gfx::device::BindError),
    #[fail(display = "Could not create buffer")]
    BufferCreationError(#[cause] gfx::buffer::CreationError),
    #[fail(display = "Could not create image")]
    ImageCreationError(#[cause] gfx::image::CreationError),
}

/// An allocation request
#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) transient: bool,
    pub(crate) _persistently_mappable: bool,
    pub(crate) properties: gfx::memory::Properties,
    pub(crate) size: u64,
    pub(crate) alignment: u64,
    pub(crate) type_mask: u64,
}

#[derive(Debug)]
pub(crate) struct BufferRequest {
    pub(crate) transient: bool,
    pub(crate) persistently_mappable: bool,
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

/// The interface any memory allocator has to implement
pub(crate) trait Allocator: std::fmt::Debug + Sized {
    type Block: Block;

    /// Allocate a block of memory from `device` that satisfies `reqs` and `request`
    unsafe fn alloc(
        &mut self,
        device: &back::Device,
        request: Request,
    ) -> Result<Self::Block, AllocationError>;

    /// Free a block of memory so it can be used in a later allocation
    unsafe fn free(&mut self, device: &back::Device, block: Self::Block);

    unsafe fn create_buffer(
        &mut self,
        device: &back::Device,
        request: BufferRequest,
    ) -> Result<BufferType<Self>, AllocatorError> {
        use gfx::Device;

        let mut buf = device
            .create_buffer(request.size, request.usage)
            .map_err(AllocatorError::BufferCreationError)?;
        let reqs = device.get_buffer_requirements(&buf);

        let request = Request {
            size: reqs.size,
            alignment: reqs.alignment,
            type_mask: reqs.type_mask,
            properties: request.properties,
            transient: request.transient,
            _persistently_mappable: request.persistently_mappable,
        };

        let block = self
            .alloc(device, request)
            .map_err(AllocatorError::AllocationError)?;

        device
            .bind_buffer_memory(block.memory(), block.range().start, &mut buf)
            .map_err(AllocatorError::BindError)?;

        Ok(BufferType { buffer: buf, block })
    }

    unsafe fn create_image(
        &mut self,
        device: &back::Device,
        request: ImageRequest,
    ) -> Result<ImageType<Self>, AllocatorError> {
        use gfx::Device;
        let mut img = device
            .create_image(
                request.kind,
                request.level,
                request.format,
                request.tiling,
                request.usage,
                request.view_caps,
            )
            .map_err(AllocatorError::ImageCreationError)?;

        let reqs = device.get_image_requirements(&img);

        let request = Request {
            size: reqs.size,
            alignment: reqs.alignment,
            type_mask: reqs.type_mask,
            properties: request.properties,
            transient: request.transient,
            _persistently_mappable: false,
        };

        let block = self
            .alloc(device, request)
            .map_err(AllocatorError::AllocationError)?;

        device
            .bind_image_memory(block.memory(), block.range().start, &mut img)
            .map_err(AllocatorError::BindError)?;

        Ok(ImageType { image: img, block })
    }

    unsafe fn destroy_buffer(&mut self, device: &back::Device, buffer: BufferType<Self>) {
        use gfx::Device;
        device.destroy_buffer(buffer.buffer);
        self.free(device, buffer.block);
    }

    unsafe fn destroy_image(&mut self, device: &back::Device, image: ImageType<Self>) {
        use gfx::Device;
        device.destroy_image(image.image);
        self.free(device, image.block);
    }

    unsafe fn dispose(self, device: &back::Device) -> Result<(), Self>
    where
        Self: Sized;
}

#[cfg(feature = "alloc_gfxm")]
pub(crate) use self::alloc_gfxm::reexport::*;

#[cfg(not(feature = "alloc_gfxm"))]
pub(crate) use self::alloc_empty::reexport::*;

#[cfg(feature = "alloc_gfxm")]
mod alloc_gfxm {

    use super::*;

    use gfx_memory::{Block as GfxmBlock, MemoryAllocator, SmartAllocator, SmartBlock};

    pub(crate) mod reexport {
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
    }

    fn gfxm_err_to_alloc_err(err: gfx_memory::MemoryError) -> AllocationError {
        println!("Encountered an error! {:?}", err);

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
        ) -> Self {
            let alloc = SmartAllocator::new(props, 256, 64, 1024, 256 * 1024 * 1024);
            Self { alloc }
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

            let reqs = gfx::memory::Requirements {
                size: request.size,
                alignment: request.alignment,
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
            self.alloc.dispose(device).map_err(|alloc| Self { alloc })
        }
    }

}

#[cfg(not(feature = "alloc_gfxm"))]
mod alloc_empty {
    use super::*;

    pub(crate) mod reexport {
        use super::*;

        pub(crate) type DefaultAlloc = AllocatorEmpty;
        pub(crate) type Buffer = BufferType<AllocatorEmpty>;
        pub(crate) type Image = ImageType<AllocatorEmpty>;
    }

    struct BlockEmpty;

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
}
