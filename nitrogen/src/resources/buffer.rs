/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Description of buffer objects.

use bitflags::bitflags;

use std;
use std::borrow::Borrow;
use std::collections::BTreeSet;

use crate::device::DeviceContext;

use crate::util::allocator::{AllocatorError, Buffer as AllocBuffer, BufferRequest};
use crate::util::storage::{Handle, Storage};

use crate::resources::command_pool::CommandPoolTransfer;
use crate::submit_group::{QueueSyncRefs, ResourceList};

pub(crate) type BufferTypeInternal = AllocBuffer;

/// A buffer object represents a chunk of (conceptually) linear memory.
///
/// Buffer objects can be used to store arbitrary data.
///
/// The most common uses are for storing vertex data for 2D or 3D objects, as well as storing
/// properties and values used for rendering (uniform inputs).
#[derive(Debug)]
pub struct Buffer {
    pub(crate) buffer: BufferTypeInternal,
    size: u64,
    _usage: gfx::buffer::Usage,
    _properties: gfx::memory::Properties,
}

/// Opaque handle to a buffer object.
pub type BufferHandle = Handle<Buffer>;

/// Errors that can occur when operating on buffer objects.
#[derive(Debug, Display, From)]
#[allow(missing_docs)]
pub enum BufferError {
    #[display(fmt = "The specified buffer handle was invalid")]
    HandleInvalid,

    #[display(fmt = "Failed to allocate buffer")]
    CantCreate(AllocatorError),

    #[display(fmt = "Failed to map the memory of the buffer")]
    MappingError(gfx::mapping::Error),

    #[display(fmt = "The provided data and offset would cause a buffer overflow")]
    UploadOutOfBounds,

    #[display(fmt = "The buffer could not be written to (not CPU visible and not TRANSFER_DST)")]
    CantWriteToBuffer,
}

impl std::error::Error for BufferError {}

bitflags!(

    /// Buffer usage flags.
    pub struct BufferUsage: u32 {
        /// Buffer can be used as a source in transfer operations.
        const TRANSFER_SRC  = 0x1;
        /// Buffer can be used as a destination in a transfer operation.
        const TRANSFER_DST = 0x2;
        /// Buffer can be used as a uniform-texel input to a shader.
        const UNIFORM_TEXEL = 0x4;
        /// Buffer can be used as a storage-texel input to a shader.
        const STORAGE_TEXEL = 0x8;
        /// Buffer can be used as a uniform input to a shader.
        const UNIFORM = 0x10;
        /// Buffer can be used as a storage input to a shader.
        const STORAGE = 0x20;
        /// Buffer can be used as an index buffer for draw operations.
        const INDEX = 0x40;
        /// Buffer can be used as a source for vertex data.
        const VERTEX = 0x80;
        /// Buffer can be used as an indirect buffer for draw operations.
        const INDIRECT = 0x100;
    }
);

impl From<BufferUsage> for gfx::buffer::Usage {
    fn from(usage: BufferUsage) -> Self {
        use gfx::buffer::Usage;
        let mut u = Usage::empty();

        if usage.contains(BufferUsage::TRANSFER_SRC) {
            u |= Usage::TRANSFER_SRC;
        }
        if usage.contains(BufferUsage::TRANSFER_DST) {
            u |= Usage::TRANSFER_DST;
        }
        if usage.contains(BufferUsage::UNIFORM_TEXEL) {
            u |= Usage::UNIFORM_TEXEL;
        }
        if usage.contains(BufferUsage::STORAGE_TEXEL) {
            u |= Usage::STORAGE_TEXEL;
        }
        if usage.contains(BufferUsage::UNIFORM) {
            u |= Usage::UNIFORM;
        }
        if usage.contains(BufferUsage::STORAGE) {
            u |= Usage::STORAGE;
        }
        if usage.contains(BufferUsage::INDEX) {
            u |= Usage::INDEX;
        }
        if usage.contains(BufferUsage::VERTEX) {
            u |= Usage::VERTEX;
        }
        if usage.contains(BufferUsage::INDIRECT) {
            u |= Usage::INDIRECT;
        }

        u
    }
}

/// Description of a cpu-visible buffer's properties.
///
/// A cpu-visible buffer is backed by memory visible both from the host and the device.
/// This memory is typically faster to update from the host but slower to access from the device.
pub struct CpuVisibleCreateInfo<U: Into<gfx::buffer::Usage> + Clone> {
    /// Size of the buffer (in bytes).
    pub size: u64,

    // TODO persistent mapping?
    /// Flag indicating whether the buffer object is short-lived or not.
    pub is_transient: bool,
    /// Usage flags indicating how the buffer object can be used.
    pub usage: U,
}

/// Description of a device-local buffer's properties.
///
/// A device-local buffer is backed by device-local memory, which is faster to access from the
/// device but cannot be accessed directly from the host.
pub struct DeviceLocalCreateInfo<U: Into<gfx::buffer::Usage> + Clone> {
    /// Size of the buffer (in bytes).
    pub size: u64,

    /// Flag indicating whether the buffer object is short-lived or not.
    pub is_transient: bool,
    /// Usage flags indicating how the buffer object can be used.
    pub usage: U,
}

/// Data provided for uploading data to a buffer.
#[derive(Copy, Clone)]
pub struct BufferUploadInfo<'a, T: 'a> {
    /// Target offset (in bytes) of the upload.
    pub offset: u64,
    /// Data to be uploaded to the buffer.
    pub data: &'a [T],
}

pub(crate) struct BufferStorage {
    cpu_visible: BTreeSet<usize>,
    device_local: BTreeSet<usize>,

    buffers: Storage<Buffer>,

    atom_size: usize,
}

impl BufferStorage {
    pub(crate) fn new(atom_size: usize) -> Self {
        BufferStorage {
            cpu_visible: BTreeSet::new(),
            device_local: BTreeSet::new(),
            buffers: Storage::new(),

            atom_size,
        }
    }

    pub(crate) unsafe fn release(self, device: &DeviceContext) {
        let mut alloc = device.allocator();

        for (_, buffer) in self.buffers {
            alloc.destroy_buffer(&device.device, buffer.buffer);
        }
    }

    pub(crate) fn raw(&self, handle: BufferHandle) -> Option<&Buffer> {
        self.buffers.get(handle)
    }

    pub(crate) fn raw_mut(&mut self, handle: BufferHandle) -> Option<&mut Buffer> {
        self.buffers.get_mut(handle)
    }

    pub(crate) unsafe fn cpu_visible_create<U>(
        &mut self,
        device: &DeviceContext,
        create_info: CpuVisibleCreateInfo<U>,
    ) -> Result<BufferHandle, BufferError>
    where
        U: Clone,
        U: Into<gfx::buffer::Usage>,
    {
        use gfx::memory::Properties;

        let mut allocator = device.allocator();

        let props = Properties::CPU_VISIBLE | Properties::COHERENT;
        let usage = create_info.usage.clone().into();

        // size should be a multiple of the non-coherent-atom-size
        let size = {
            let inv_pad = create_info.size % (self.atom_size as u64);
            if inv_pad != 0 {
                create_info.size + (self.atom_size as u64 - inv_pad)
            } else {
                create_info.size
            }
        };

        let req = BufferRequest {
            transient: create_info.is_transient,
            properties: props,
            usage,
            size,
        };

        let raw_buffer = allocator.create_buffer(&device.device, req)?;

        let buffer = Buffer {
            size,
            buffer: raw_buffer,
            _properties: props,
            _usage: usage,
        };

        let handle = self.buffers.insert(buffer);
        self.cpu_visible.insert(handle.0);
        Ok(handle)
    }

    pub(crate) unsafe fn cpu_visible_upload<'a, T>(
        &mut self,
        device: &DeviceContext,
        buffer: BufferHandle,
        info: BufferUploadInfo<'a, T>,
    ) -> Result<(), BufferError> {
        if !self.cpu_visible.contains(&buffer.0) {
            return Err(BufferError::HandleInvalid);
        }

        let buffer = self.raw_mut(buffer).ok_or(BufferError::HandleInvalid)?;

        let u8_data = to_u8_slice(info.data);

        let upload_fits = info.offset + u8_data.len() as u64 <= buffer.size;

        if upload_fits {
            write_data_to_buffer(device, &mut buffer.buffer, info.offset, u8_data)
        } else {
            Err(BufferError::UploadOutOfBounds)
        }
    }

    pub(crate) unsafe fn cpu_visible_read<T: Sized>(
        &mut self,
        device: &DeviceContext,
        buffer: BufferHandle,
        out: &mut [T],
    ) -> Option<()> {
        if !self.cpu_visible.contains(&buffer.0) {
            return None;
        }

        let buffer = self.buffers.get_mut(buffer)?;

        read_data_from_buffer(device, &mut buffer.buffer, 0, to_u8_mut_slice(out)).ok()?;

        Some(())
    }

    pub(crate) unsafe fn device_local_create<U>(
        &mut self,
        device: &DeviceContext,
        create_info: DeviceLocalCreateInfo<U>,
    ) -> Result<BufferHandle, BufferError>
    where
        U: Clone,
        U: Into<gfx::buffer::Usage>,
    {
        use gfx::memory::Properties;

        let mut allocator = device.allocator();

        let props = Properties::DEVICE_LOCAL;
        let usage = create_info.usage.clone().into();

        // size should be a multiple of the non-coherent-atom-size
        let size = {
            let inv_pad = create_info.size % (self.atom_size as u64);
            if inv_pad != 0 {
                create_info.size + (self.atom_size as u64 - inv_pad)
            } else {
                create_info.size
            }
        };

        let req = BufferRequest {
            transient: create_info.is_transient,
            properties: props,
            usage,
            size,
        };

        let raw_buffer = allocator.create_buffer(&device.device, req)?;

        let buffer = Buffer {
            size,
            buffer: raw_buffer,
            _properties: props,
            _usage: usage,
        };

        let handle = self.buffers.insert(buffer);
        self.device_local.insert(handle.0);

        Ok(handle)
    }

    pub(crate) unsafe fn device_local_upload<'a, T>(
        &self,
        device: &DeviceContext,
        sync: &mut QueueSyncRefs,
        cmd_pool: &CommandPoolTransfer,
        buffer: BufferHandle,
        info: BufferUploadInfo<'a, T>,
    ) -> Result<(), BufferError> {
        use gfx::buffer::Usage;
        use gfx::memory::Properties;

        let mut alloc = device.allocator();

        if !self.device_local.contains(&buffer.0) {
            return Err(BufferError::HandleInvalid);
        }

        let buffer = self.raw(buffer).ok_or(BufferError::HandleInvalid)?;

        let u8_slice = to_u8_slice(info.data);

        let upload_fits = info.offset + u8_slice.len() as u64 <= buffer.size;

        if !upload_fits {
            return Err(BufferError::UploadOutOfBounds);
        }

        let req = BufferRequest {
            transient: true,
            properties: Properties::CPU_VISIBLE | Properties::COHERENT,
            usage: Usage::TRANSFER_SRC | Usage::TRANSFER_DST,
            size: u8_slice.len() as u64,
        };

        let mut staging_buffer = alloc.create_buffer(&device.device, req)?;

        // write to staging buffer

        if let Err(err) = write_data_to_buffer(device, &mut staging_buffer, 0, u8_slice) {
            alloc.destroy_buffer(&device.device, staging_buffer);
            return Err(dbg!(err))?;
        }

        crate::transfer::copy_buffers(
            device,
            sync.sem_pool,
            sync.sem_list,
            cmd_pool,
            &[crate::transfer::BufferTransfer {
                src: &staging_buffer,
                dst: &buffer.buffer,
                offset: info.offset,
                data: u8_slice,
            }],
        );

        sync.res_list.queue_buffer(staging_buffer);

        Ok(())
    }

    pub fn destroy<B>(&mut self, res_list: &mut ResourceList, buffers: B)
    where
        B: IntoIterator,
        B::Item: std::borrow::Borrow<BufferHandle>,
    {
        for handle in buffers.into_iter() {
            let handle = *handle.borrow();
            let buffer = match self.buffers.remove(handle) {
                Some(buf) => buf,
                None => continue,
            };
            self.device_local.remove(&handle.0);
            self.cpu_visible.remove(&handle.0);
            res_list.queue_buffer(buffer.buffer);
        }
    }
}

unsafe fn to_u8_slice<T>(slice: &[T]) -> &[u8] {
    use std::mem;

    let t_ptr = slice.as_ptr();
    let t_len = slice.len();

    let b_ptr = t_ptr as *const _;
    let b_len = t_len * mem::size_of::<T>();

    std::slice::from_raw_parts(b_ptr, b_len)
}

unsafe fn to_u8_mut_slice<T>(slice: &mut [T]) -> &mut [u8] {
    use std::mem;

    let t_ptr = slice.as_ptr();
    let t_len = slice.len();

    let b_ptr = t_ptr as *mut _;
    let b_len = t_len * mem::size_of::<T>();

    std::slice::from_raw_parts_mut(b_ptr, b_len)
}

unsafe fn write_data_to_buffer(
    device: &DeviceContext,
    buffer: &mut BufferTypeInternal,
    offset: u64,
    data: &[u8],
) -> Result<(), BufferError> {
    use rendy_memory::Block;

    let block = buffer.block_mut();

    let range = offset..(offset + data.len() as u64);

    if (range.start == range.end) || (range.end < range.start) {
        // TODO return error?
        return Ok(());
    }

    let mut map = block.map(&device.device, range.clone())?;

    {
        use rendy_memory::Write;

        let mut writer = map.write::<u8>(&device.device, 0..data.len() as u64)?;

        writer.write(data);
    }

    block.unmap(&device.device);

    Ok(())
}

unsafe fn read_data_from_buffer(
    device: &DeviceContext,
    buffer: &mut BufferTypeInternal,
    offset: u64,
    data: &mut [u8],
) -> Result<(), BufferError> {
    use rendy_memory::Block;

    let block = buffer.block_mut();

    let range = offset..(offset + data.len() as u64);

    if (range.start == range.end) || (range.end < range.start) {
        // TODO return error?
        return Ok(());
    }

    let mut map = block.map(&device.device, range.clone())?;

    {
        let slice = map.read(&device.device, 0..data.len() as u64)?;

        data.copy_from_slice(slice);
    }

    block.unmap(&device.device);

    Ok(())
}
