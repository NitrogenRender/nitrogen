/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use back;
use gfx;
use gfxm;

use bitflags::bitflags;
use failure_derive::Fail;

use std;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

use device::DeviceContext;
use transfer::TransferContext;

use util::storage::{Handle, Storage};

use gfxm::Factory;
use gfxm::SmartAllocator;

use smallvec::smallvec;
use smallvec::SmallVec;

use resources::MemoryProperties;

use resources::semaphore_pool::SemaphoreList;
use resources::semaphore_pool::SemaphorePool;
use types;
use types::CommandPool;

type BufferId = usize;
pub type BufferTypeInternal = <SmartAllocator<back::Backend> as Factory<back::Backend>>::Buffer;

#[derive(Copy, Clone)]
pub enum BufferType {
    DeviceAccessible,
    HostAccessible,
    UnAccessible,
}

pub struct Buffer {
    buffer: BufferTypeInternal,
    size: u64,
    usage: gfx::buffer::Usage,
    properties: gfx::memory::Properties,
}

pub type BufferHandle = Handle<BufferType>;

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

pub struct BufferCreateInfo {
    pub size: u64,

    pub is_transient: bool,

    pub properties: MemoryProperties,
    pub usage: BufferUsage,
}

#[derive(Copy, Clone)]
pub struct BufferUploadInfo<'a, T: 'a> {
    pub offset: u64,
    pub data: &'a [T],
}

pub struct BufferStorage {
    local_buffers: BTreeSet<BufferId>,
    host_visible_buffers: BTreeSet<BufferId>,
    other_buffers: BTreeSet<BufferId>,
    buffers: BTreeMap<BufferId, Buffer>,
    storage: Storage<BufferType>,
}

impl BufferStorage {
    pub fn new() -> Self {
        BufferStorage {
            local_buffers: BTreeSet::new(),
            host_visible_buffers: BTreeSet::new(),
            other_buffers: BTreeSet::new(),
            buffers: BTreeMap::new(),
            storage: Storage::new(),
        }
    }

    pub fn release(self) {}

    pub fn create(
        &mut self,
        device: &DeviceContext,
        create_infos: &[BufferCreateInfo],
    ) -> SmallVec<[Result<BufferHandle>; 16]> {
        let mut results = SmallVec::with_capacity(create_infos.len());

        // TODO This is a big critical section, need to check if it's better to do
        // a lot of small locks or just this one big one.
        let mut allocator = device.allocator();

        for create_info in create_infos {
            let (raw_buffer, properties, usage) = {
                let (ty, props) = {
                    let ty = match create_info.is_transient {
                        true => gfxm::Type::ShortLived,
                        false => gfxm::Type::General,
                    };

                    let properties = create_info.properties.into();

                    (ty, properties)
                };

                let usage = create_info.usage.into();

                let buf = match allocator.create_buffer(
                    &device.device,
                    (ty, props),
                    create_info.size,
                    usage,
                ) {
                    Err(e) => {
                        results.push(Err(e.into()));
                        continue;
                    }
                    Ok(buf) => buf,
                };

                (buf, props, usage)
            };

            let ty = buffer_type(properties, usage);

            let buffer = Buffer {
                buffer: raw_buffer,
                size: create_info.size,
                properties,
                usage,
            };

            let (handle, _) = self.storage.insert(ty);

            match ty {
                BufferType::DeviceAccessible => {
                    self.local_buffers.insert(handle.id());
                }
                BufferType::HostAccessible => {
                    self.host_visible_buffers.insert(handle.id());
                }
                BufferType::UnAccessible => {
                    self.other_buffers.insert(handle.id());
                }
            }

            self.buffers.insert(handle.id(), buffer);

            results.push(Ok(handle));
        }

        results
    }

    pub fn destroy(&mut self, device: &DeviceContext, buffers: &[BufferHandle]) {
        let mut allocator = device.allocator();
        for handle in buffers {
            let id = handle.id();
            let buffer = self.buffers.remove(&id).unwrap();
            allocator.destroy_buffer(&device.device, buffer.buffer);
        }
    }

    pub(crate) fn raw(&self, buffer: BufferHandle) -> Option<&BufferTypeInternal> {
        self.storage
            .get(buffer)
            .map(|buf| &self.buffers[&buffer.id()].buffer)
    }

    pub fn upload_data<T>(
        &mut self,
        device: &DeviceContext,
        sem_pool: &SemaphorePool,
        sem_list: &mut SemaphoreList,
        cmd_pool: &mut CommandPool<gfx::Transfer>,
        transfer: &TransferContext,
        data: &[(BufferHandle, BufferUploadInfo<T>)],
    ) -> SmallVec<[Result<()>; 16]> {
        let mut results = smallvec![Ok(()); data.len()];

        // sort for linear access pattern
        let mut data: SmallVec<[_; 16]> = data.iter().enumerate().collect();
        data.as_mut_slice()
            .sort_by_key(|(_, (handle, _))| handle.id());

        // categorize buffers
        let (cpu_accessible, dev_local, other) = {
            let mut cpu_accessible = SmallVec::<[_; 16]>::new();
            let mut dev_local = SmallVec::<[_; 16]>::new();
            let mut other = SmallVec::<[_; 16]>::new();

            for (idx, (handle, data)) in data {
                let handle = *handle;
                if !self.storage.is_alive(handle) {
                    results[idx] = Err(BufferError::HandleInvalid);
                    continue;
                }

                let ty = self.storage[handle];
                match ty {
                    BufferType::HostAccessible => {
                        cpu_accessible.push((idx, handle, data));
                    }
                    BufferType::DeviceAccessible => {
                        dev_local.push((idx, handle, data));
                    }
                    BufferType::UnAccessible => {
                        other.push((idx, handle, data));
                    }
                }
            }

            (cpu_accessible, dev_local, other)
        };

        // Can't write to those...
        for (idx, _, _) in other {
            results[idx] = Err(BufferError::CantWriteToBuffer);
        }

        // Simple memory-mapped writing is enough.
        for (idx, handle, data) in cpu_accessible {
            let buffer = self.buffers.get(&handle.id()).unwrap();

            let upload_fits = data.offset + data.data.len() as u64 <= buffer.size;

            let result = if upload_fits {
                unsafe {
                    write_data_to_buffer(
                        device,
                        &buffer.buffer,
                        data.offset,
                        to_u8_slice(data.data),
                    )
                    .into()
                }
            } else {
                Err(BufferError::UploadOutOfBounds)
            };

            results[idx] = result;
        }

        //
        // DEALING WITH DEVICE LOCAL STUFF HERE.
        //

        let mut allocator = device.allocator();
        let staging_data = {
            dev_local
                .as_slice()
                .iter()
                .map(|(idx, handle, data)| {
                    let buffer = self.buffers.get(&handle.id()).unwrap();

                    let upload_fits = data.offset + data.data.len() as u64 <= buffer.size;

                    if !upload_fits {
                        (*idx, None)
                    } else {
                        (*idx, Some((data, buffer)))
                    }
                })
                .filter_map(|(idx, res)| {
                    let (data, buffer) = match res {
                        None => {
                            results[idx] = Err(BufferError::UploadOutOfBounds);
                            return None;
                        }
                        Some((data, buffer)) => (data, buffer),
                    };

                    use gfx::buffer::Usage;
                    use gfx::memory::Properties;

                    let result = allocator.create_buffer(
                        &device.device,
                        (
                            gfxm::Type::ShortLived,
                            Properties::CPU_VISIBLE | Properties::COHERENT,
                        ),
                        data.data.len() as u64,
                        Usage::TRANSFER_SRC | Usage::TRANSFER_DST,
                    );

                    match result {
                        Err(e) => {
                            results[idx] = Err(e.into());
                            None
                        }
                        Ok(staging) => Some((idx, data, buffer, staging)),
                    }
                })
                .collect::<SmallVec<[_; 16]>>()
        };

        // do copying and writing
        {
            let buffer_transfers = staging_data
                .as_slice()
                .iter()
                .filter_map(|(idx, data, buffer, staging)| {
                    let upload_sice = unsafe { to_u8_slice(data.data) };

                    match write_data_to_buffer(device, staging, 0, upload_sice) {
                        Err(e) => {
                            results[*idx] = Err(e.into());
                            None
                        }
                        Ok(()) => Some((data, buffer, staging)),
                    }
                })
                .map(|(data, buffer, staging)| {
                    use transfer::BufferTransfer;

                    let upload_sice = unsafe { to_u8_slice(data.data) };

                    BufferTransfer {
                        src: &staging,
                        dst: &buffer.buffer,
                        offset: data.offset,
                        data: upload_sice,
                    }
                })
                .collect::<SmallVec<[_; 16]>>();

            transfer.copy_buffers(
                device,
                sem_pool,
                sem_list,
                cmd_pool,
                buffer_transfers.as_slice(),
            );
        }

        staging_data.into_iter().for_each(|(_, _, _, staging)| {
            // FIXME destroy only after the submit group is done
            // FIXME
            // FIXME DO IT THOMAS
            // FIXME
            // FIXME You know you have to do it eventually
            allocator.destroy_buffer(&device.device, staging);
        });

        results
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

    device.device.release_mapping_writer(writer);

    Ok(())
}

fn buffer_type(properties: gfx::memory::Properties, usage: gfx::buffer::Usage) -> BufferType {
    use gfx::buffer::Usage;
    use gfx::memory::Properties;

    let is_device_accessible =
        properties.contains(Properties::DEVICE_LOCAL) && usage.contains(Usage::TRANSFER_DST);
    let is_cpu_accessible = properties.contains(Properties::CPU_VISIBLE | Properties::COHERENT);

    if is_device_accessible {
        BufferType::DeviceAccessible
    } else if is_cpu_accessible {
        BufferType::HostAccessible
    } else {
        BufferType::UnAccessible
    }
}
