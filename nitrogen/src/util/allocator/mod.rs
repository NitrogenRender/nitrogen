/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#[cfg(not(any(feature = "alloc_rendy", feature = "alloc_gfxm")))]
mod empty;
#[cfg(not(any(feature = "alloc_rendy", feature = "alloc_gfxm")))]
pub(crate) use self::empty::export::*;

#[cfg(feature = "alloc_rendy")]
mod rendy;
#[cfg(feature = "alloc_rendy")]
pub(crate) use self::rendy::export::*;

#[cfg(feature = "alloc_gfxm")]
mod gfxm;
#[cfg(feature = "alloc_gfxm")]
pub(crate) use self::gfxm::export::*;

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
#[derive(Debug, Display, Clone)]
pub enum AllocationError {
    /// No suitable memory type could be found
    #[display(fmt = "No suitable memory type could be found")]
    NoSuitableMemoryType,

    /// Not enough free memory available to perform the allocation
    #[display(fmt = "Out of memory")]
    OutOfMemory,

    /// An implementation might impose a limited number of objects
    #[display(fmt = "Too many objects")]
    TooManyObjects,
}

impl std::error::Error for AllocationError {}

#[derive(Debug, Display, From, Clone)]
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

impl std::error::Error for AllocatorError {}

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

        let mut buf = device.create_buffer(request.size, request.usage)?;
        let reqs = device.get_buffer_requirements(&buf);

        let request = Request {
            size: reqs.size,
            alignment: reqs.alignment,
            type_mask: reqs.type_mask,
            properties: request.properties,
            transient: request.transient,
            _persistently_mappable: request.persistently_mappable,
        };

        let block = self.alloc(device, request)?;

        device.bind_buffer_memory(block.memory(), block.range().start, &mut buf)?;

        Ok(BufferType { buffer: buf, block })
    }

    unsafe fn create_image(
        &mut self,
        device: &back::Device,
        request: ImageRequest,
    ) -> Result<ImageType<Self>, AllocatorError> {
        use gfx::Device;
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
            _persistently_mappable: false,
        };

        let block = self.alloc(device, request)?;

        device.bind_image_memory(block.memory(), block.range().start, &mut img)?;

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
