/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use bitflags::bitflags;
use failure_derive::Fail;

use std;
use std::borrow::Borrow;
use std::collections::BTreeSet;

use crate::device::DeviceContext;

use crate::util::storage::{Handle, Storage};

use gfxm::Factory;
use gfxm::SmartAllocator;

use smallvec::SmallVec;

use crate::resources::semaphore_pool::SemaphoreList;
use crate::resources::semaphore_pool::SemaphorePool;
use crate::submit_group::ResourceList;
use crate::types::CommandPool;

pub(crate) type BufferTypeInternal =
    <SmartAllocator<back::Backend> as Factory<back::Backend>>::Buffer;

pub struct Buffer {
    pub(crate) buffer: BufferTypeInternal,
    size: u64,
    _usage: gfx::buffer::Usage,
    _properties: gfx::memory::Properties,
}

pub type BufferHandle = Handle<Buffer>;

pub type Result<T> = std::result::Result<T, BufferError>;

#[derive(Debug, Fail, Clone)]
pub enum BufferError {
    #[fail(display = "The specified buffer handle was invalid")]
    HandleInvalid,

    #[fail(display = "Failed to allocate buffer")]
    CantCreate(#[cause] gfxm::FactoryError),

    #[fail(display = "Failed to map the memory of the buffer")]
    MappingError(#[cause] gfx::mapping::Error),

    #[fail(display = "The provided data and offset would cause a buffer overflow")]
    UploadOutOfBounds,

    #[fail(display = "The buffer could not be written to (not CPU visible and not TRANSFER_DST)")]
    CantWriteToBuffer,
}

impl From<gfxm::FactoryError> for BufferError {
    fn from(error: gfxm::FactoryError) -> Self {
        BufferError::CantCreate(error)
    }
}

impl From<gfx::mapping::Error> for BufferError {
    fn from(error: gfx::mapping::Error) -> Self {
        BufferError::MappingError(error)
    }
}

bitflags!(

    /// Buffer usage flags.
    pub struct BufferUsage: u32 {
        const TRANSFER_SRC  = 0x1;
        const TRANSFER_DST = 0x2;
        const UNIFORM_TEXEL = 0x4;
        const STORAGE_TEXEL = 0x8;
        const UNIFORM = 0x10;
        const STORAGE = 0x20;
        const INDEX = 0x40;
        const VERTEX = 0x80;
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

pub struct CpuVisibleCreateInfo<U: Into<gfx::buffer::Usage> + Clone> {
    pub size: u64,

    // TODO persistent mapping?
    pub is_transient: bool,
    pub usage: U,
}

pub struct DeviceLocalCreateInfo<U: Into<gfx::buffer::Usage> + Clone> {
    pub size: u64,

    pub is_transient: bool,
    pub usage: U,
}

#[derive(Copy, Clone)]
pub struct BufferUploadInfo<'a, T: 'a> {
    pub offset: u64,
    pub data: &'a [T],
}

pub(crate) struct BufferStorage {
    cpu_visible: BTreeSet<usize>,
    device_local: BTreeSet<usize>,

    buffers: Storage<Buffer>,
}

impl BufferStorage {
    pub(crate) fn new() -> Self {
        BufferStorage {
            cpu_visible: BTreeSet::new(),
            device_local: BTreeSet::new(),
            buffers: Storage::new(),
        }
    }

    pub(crate) fn release(self, device: &DeviceContext) {
        let mut alloc = device.allocator();

        for (_, buffer) in self.buffers.into_iter() {
            alloc.destroy_buffer(&device.device, buffer.buffer);
        }
    }

    pub(crate) fn raw(&self, handle: BufferHandle) -> Option<&Buffer> {
        self.buffers.get(handle)
    }

    pub(crate) fn cpu_visible_create<I, U>(
        &mut self,
        device: &DeviceContext,
        create_infos: I,
    ) -> SmallVec<[Result<BufferHandle>; 16]>
    where
        I: IntoIterator,
        I::Item: std::borrow::Borrow<CpuVisibleCreateInfo<U>>,
        U: Clone,
        U: Into<gfx::buffer::Usage>,
    {
        use gfx::memory::Properties;

        let mut results = SmallVec::new();

        let mut allocator = device.allocator();

        for create_info in create_infos.into_iter() {
            let create_info = create_info.borrow();

            let ty = match create_info.is_transient {
                true => gfxm::Type::ShortLived,
                false => gfxm::Type::General,
            };

            let props = Properties::CPU_VISIBLE | Properties::COHERENT;
            let usage = create_info.usage.clone().into();

            let raw_buffer =
                match allocator.create_buffer(&device.device, (ty, props), create_info.size, usage)
                {
                    Ok(buf) => buf,
                    Err(err) => {
                        results.push(Err(err.into()));
                        continue;
                    }
                };

            let buffer = Buffer {
                size: create_info.size,
                buffer: raw_buffer,
                _properties: props,
                _usage: usage,
            };

            let handle = self.buffers.insert(buffer);
            self.cpu_visible.insert(handle.0);

            results.push(Ok(handle));
        }

        results
    }

    pub(crate) fn cpu_visible_upload<'a, I, T>(
        &self,
        device: &DeviceContext,
        uploads: I,
    ) -> SmallVec<[Result<()>; 16]>
    where
        T: 'a,
        I: IntoIterator,
        I::Item: std::borrow::Borrow<(BufferHandle, BufferUploadInfo<'a, T>)>,
    {
        let mut results = SmallVec::new();

        for upload in uploads.into_iter() {
            let (buffer, info) = upload.borrow();

            if !self.cpu_visible.contains(&buffer.0) {
                results.push(Err(BufferError::HandleInvalid));
                continue;
            }

            let buffer = match self.raw(*buffer) {
                Some(buf) => buf,
                None => {
                    results.push(Err(BufferError::HandleInvalid));
                    continue;
                }
            };

            let u8_data = unsafe { to_u8_slice(info.data) };

            let upload_fits = info.offset + u8_data.len() as u64 <= buffer.size;

            let res = if upload_fits {
                write_data_to_buffer(device, &buffer.buffer, info.offset, u8_data).into()
            } else {
                Err(BufferError::UploadOutOfBounds)
            };

            results.push(res);
        }

        results
    }

    pub(crate) fn cpu_visible_read<T: Sized>(
        &self,
        device: &DeviceContext,
        buffer: BufferHandle,
        out: &mut [T],
    ) -> Option<()> {
        if !self.cpu_visible.contains(&buffer.0) {
            return None;
        }

        let buffer = self.buffers.get(buffer)?;

        read_data_from_buffer(device, &buffer.buffer, 0, unsafe { to_u8_mut_slice(out) }).ok()?;

        Some(())
    }

    pub(crate) fn device_local_create<I, U>(
        &mut self,
        device: &DeviceContext,
        create_infos: I,
    ) -> SmallVec<[Result<BufferHandle>; 16]>
    where
        I: IntoIterator,
        I::Item: std::borrow::Borrow<DeviceLocalCreateInfo<U>>,
        U: Clone,
        U: Into<gfx::buffer::Usage>,
    {
        use gfx::memory::Properties;

        let mut results = SmallVec::new();

        let mut allocator = device.allocator();

        for create_info in create_infos.into_iter() {
            let create_info = create_info.borrow();

            let ty = match create_info.is_transient {
                true => gfxm::Type::ShortLived,
                false => gfxm::Type::General,
            };

            let props = Properties::DEVICE_LOCAL;
            let usage = create_info.usage.clone().into();

            let raw_buffer =
                match allocator.create_buffer(&device.device, (ty, props), create_info.size, usage)
                {
                    Ok(buf) => buf,
                    Err(err) => {
                        results.push(Err(err.into()));
                        continue;
                    }
                };

            let buffer = Buffer {
                size: create_info.size,
                buffer: raw_buffer,
                _properties: props,
                _usage: usage,
            };

            let handle = self.buffers.insert(buffer);
            self.device_local.insert(handle.0);

            results.push(Ok(handle));
        }

        results
    }

    pub(crate) fn device_local_upload<'a, I, T>(
        &self,
        device: &DeviceContext,
        sem_pool: &SemaphorePool,
        sem_list: &mut SemaphoreList,
        cmd_pool: &mut CommandPool<gfx::Transfer>,
        res_list: &mut ResourceList,
        uploads: I,
    ) -> SmallVec<[Result<()>; 16]>
    where
        T: 'a,
        I: IntoIterator,
        I::Item: std::borrow::Borrow<(BufferHandle, BufferUploadInfo<'a, T>)>,
        I::Item: Clone,
    {
        use gfx::buffer::Usage;
        use gfx::memory::Properties;

        let mut results = SmallVec::new();

        let mut staging_buffers = SmallVec::<[_; 16]>::new();

        let mut transfers = SmallVec::<[_; 16]>::new();

        let mut alloc = device.allocator();
        for upload in uploads.into_iter() {
            let (buffer, info) = upload.borrow();

            if !self.device_local.contains(&buffer.0) {
                results.push(Err(BufferError::HandleInvalid));
                continue;
            }

            let buffer = match self.raw(*buffer) {
                None => {
                    results.push(Err(BufferError::HandleInvalid));
                    continue;
                }
                Some(buf) => buf,
            };

            let u8_slice = unsafe { to_u8_slice(info.data) };

            let upload_fits = info.offset + u8_slice.len() as u64 <= buffer.size;

            if !upload_fits {
                results.push(Err(BufferError::UploadOutOfBounds));
                continue;
            }

            let staging_res = alloc.create_buffer(
                &device.device,
                (
                    gfxm::Type::ShortLived,
                    Properties::CPU_VISIBLE | Properties::COHERENT,
                ),
                u8_slice.len() as u64,
                Usage::TRANSFER_SRC | Usage::TRANSFER_DST,
            );

            let staging_buffer = match staging_res {
                Err(err) => {
                    results.push(Err(err.into()));
                    continue;
                }
                Ok(buffer) => buffer,
            };

            // write to staging buffer

            match write_data_to_buffer(device, &staging_buffer, 0, u8_slice) {
                Err(err) => {
                    results.push(Err(err.into()));
                    continue;
                }
                Ok(_) => {}
            };

            staging_buffers.push(staging_buffer);

            transfers.push((upload, u8_slice));
        }

        crate::transfer::copy_buffers(
            device,
            sem_pool,
            sem_list,
            cmd_pool,
            transfers.as_slice().iter().zip(staging_buffers.iter()).map(
                |((upload, u8_slice), staging_buf)| {
                    let (buf, info) = upload.borrow();
                    let buf = self.raw(*buf).unwrap();

                    crate::transfer::BufferTransfer {
                        src: staging_buf,
                        dst: &buf.buffer,
                        offset: info.offset,
                        data: *u8_slice,
                    }
                },
            ),
        );

        staging_buffers.into_iter().for_each(|buf| {
            res_list.queue_buffer(buf);
        });

        results
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

    let b_ptr = mem::transmute(t_ptr);
    let b_len = t_len * mem::size_of::<T>();

    std::slice::from_raw_parts(b_ptr, b_len)
}

unsafe fn to_u8_mut_slice<T>(slice: &mut [T]) -> &mut [u8] {
    use std::mem;

    let t_ptr = slice.as_ptr();
    let t_len = slice.len();

    let b_ptr = mem::transmute(t_ptr);
    let b_len = t_len * mem::size_of::<T>();

    std::slice::from_raw_parts_mut(b_ptr, b_len)
}

fn write_data_to_buffer(
    device: &DeviceContext,
    buffer: &BufferTypeInternal,
    offset: u64,
    data: &[u8],
) -> Result<()> {
    use gfx::Device;
    use gfxm::Block;

    let offset = offset as usize;

    let range = buffer.range();

    let mut writer = device
        .device
        .acquire_mapping_writer(buffer.memory(), range)?;

    writer[offset..offset + data.len()].copy_from_slice(data);

    device.device.release_mapping_writer(writer).unwrap();

    Ok(())
}

fn read_data_from_buffer(
    device: &DeviceContext,
    buffer: &BufferTypeInternal,
    offset: u64,
    data: &mut [u8],
) -> Result<()> {
    use gfx::Device;
    use gfxm::Block;

    let offset = offset as usize;

    let range = buffer.range();

    let reader = device
        .device
        .acquire_mapping_reader(buffer.memory(), range)?;

    data.copy_from_slice(&reader[offset..offset + data.len()]);

    device.device.release_mapping_reader(reader);

    Ok(())
}
