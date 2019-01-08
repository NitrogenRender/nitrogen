/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::util::storage::{Handle, Storage};

use crate::device::DeviceContext;

use crate::resources::buffer::{BufferHandle, BufferStorage};
use crate::resources::image::{ImageHandle, ImageStorage};
use crate::resources::sampler::{SamplerHandle, SamplerStorage};

use crate::types;

use smallvec::{smallvec, SmallVec};

use failure_derive::Fail;

pub type MaterialHandle = Handle<Material>;

const MAX_SETS_PER_POOL: u8 = 16;

pub type MaterialInstanceHandle = (MaterialHandle, Handle<MaterialInstance>);

// A material contains its parameters for validation and later pool creation.
// Since the number of material instances is not always known at program startup,
// a list of pools is maintained and expanded if needed.
pub struct Material {
    parameters: Vec<(u32, MaterialParameterType)>,
    pub(crate) desc_set_layout: types::DescriptorSetLayout,
    pool_allocated: Vec<u8>,
    pool_used: Vec<u8>,
    pools: Vec<types::DescriptorPool>,

    instances: Storage<MaterialInstance>,
}

pub struct MaterialCreateInfo<'a> {
    // TODO add support for arrays later on?
    pub parameters: &'a [(u32, MaterialParameterType)],
}

pub struct MaterialInstance {
    pool: usize,
    pub(crate) set: types::DescriptorSet,
}

pub(crate) struct MaterialStorage {
    storage: Storage<Material>,
}

#[derive(Copy, Clone)]
pub enum MaterialParameterType {
    Sampler,
    SampledImage,
    StorageImage,
    UniformTexelBuffer,
    StorageTexelBuffer,
    UniformBuffer,
    StorageBuffer,
    UniformBufferDynamic,
    StorageBufferDynamic,
    // TODO input attachments??
}

impl From<MaterialParameterType> for gfx::pso::DescriptorType {
    fn from(ty: MaterialParameterType) -> Self {
        use gfx::pso::DescriptorType;
        match ty {
            MaterialParameterType::Sampler => DescriptorType::Sampler,
            MaterialParameterType::SampledImage => DescriptorType::SampledImage,
            MaterialParameterType::StorageImage => DescriptorType::StorageImage,
            MaterialParameterType::UniformTexelBuffer => DescriptorType::UniformTexelBuffer,
            MaterialParameterType::StorageTexelBuffer => DescriptorType::StorageTexelBuffer,
            MaterialParameterType::UniformBuffer => DescriptorType::UniformBuffer,
            MaterialParameterType::StorageBuffer => DescriptorType::StorageBuffer,
            MaterialParameterType::UniformBufferDynamic => DescriptorType::UniformBufferDynamic,
            MaterialParameterType::StorageBufferDynamic => DescriptorType::StorageBufferDynamic,
        }
    }
}

pub struct InstanceWrite {
    pub binding: u32,
    pub data: InstanceWriteData,
}

pub enum InstanceWriteData {
    Sampler {
        sampler: SamplerHandle,
    },
    Image {
        image: ImageHandle,
    },
    Buffer {
        buffer: BufferHandle,
        region: ::std::ops::Range<Option<u64>>,
    },
    // TODO buffer views for texel buffers?
    /*
    UniformTexelBuffer {
        buffer: BufferHandle,
    },
    StorageTexelBuffer {
        buffer: BufferHandle,
    }
    */
}

impl MaterialStorage {
    pub(crate) fn new() -> Self {
        MaterialStorage {
            storage: Storage::new(),
        }
    }

    pub(crate) unsafe fn create(
        &mut self,
        device: &DeviceContext,
        create_infos: &[MaterialCreateInfo],
    ) -> SmallVec<[Result<MaterialHandle, MaterialError>; 16]> {
        use gfx::Device;

        let mut results = smallvec![];

        for create_info in create_infos {
            let descriptors = create_info
                .parameters
                .iter()
                .map(
                    |(binding, desc_type)| gfx::pso::DescriptorSetLayoutBinding {
                        binding: *binding,
                        ty: desc_type.clone().into(),
                        count: 1,
                        stage_flags: gfx::pso::ShaderStageFlags::ALL,
                        immutable_samplers: false,
                    },
                )
                .collect::<SmallVec<[_; 16]>>();

            let res = device
                .device
                .create_descriptor_set_layout(descriptors.as_slice(), &[]);

            match res {
                Ok(set) => {
                    let mut parameters = create_info.parameters.to_vec();
                    parameters.sort_by_key(|(binding, _)| *binding);

                    let mat = Material {
                        parameters,
                        desc_set_layout: set,
                        instances: Storage::new(),
                        pool_allocated: Vec::new(),
                        pool_used: Vec::new(),
                        pools: Vec::new(),
                    };
                    let handle = self.storage.insert(mat);
                    results.push(Ok(handle));
                }
                Err(err) => {
                    results.push(Err(err.into()));
                }
            }
        }

        results
    }

    pub(crate) unsafe fn create_raw(
        &mut self,
        device: &DeviceContext,
        layout: types::DescriptorSetLayout,
    ) -> MaterialHandle {
        let mat = Material {
            parameters: vec![],
            desc_set_layout: layout,
            instances: Storage::new(),
            pool_allocated: vec![],
            pool_used: vec![],
            pools: vec![],
        };

        self.storage.insert(mat)
    }

    pub(crate) unsafe fn destroy(&mut self, device: &DeviceContext, materials: &[MaterialHandle]) {
        for handle in materials {
            if let Some(mat) = self.storage.remove(*handle) {
                mat.release(device);
            }
        }
    }

    pub(crate) fn raw(&self, material: MaterialHandle) -> Option<&Material> {
        self.storage.get(material)
    }

    pub(crate) unsafe fn create_instances(
        &mut self,
        device: &DeviceContext,
        materials: &[MaterialHandle],
    ) -> SmallVec<[Result<MaterialInstanceHandle, MaterialError>; 16]> {
        let mut results = smallvec![];

        for material in materials {
            let mat_res = self
                .storage
                .get_mut(*material)
                .ok_or(MaterialError::InvalidHandle);

            let mat = match mat_res {
                Ok(mat) => mat,
                Err(err) => {
                    results.push(Err(err.into()));
                    continue;
                }
            };

            let instance = match mat.create_instance(device) {
                Ok(inst) => inst,
                Err(err) => {
                    results.push(Err(err.into()));
                    continue;
                }
            };

            results.push(Ok((*material, instance)));
        }

        results
    }

    pub(crate) unsafe fn write_instance<I>(
        &self,
        device: &DeviceContext,
        sampler_storage: &SamplerStorage,
        image_storage: &ImageStorage,
        buffer_storage: &BufferStorage,
        instance: MaterialInstanceHandle,
        data: I,
    ) -> Option<()>
    where
        I: IntoIterator,
        I::Item: ::std::borrow::Borrow<InstanceWrite>,
    {
        use gfx::Device;

        let mat = self.storage.get(instance.0)?;

        let instance = mat.instances.get(instance.1)?;

        // TODO verify that types match?
        let writes = data.into_iter().filter_map(|write| {
            use std::borrow::Borrow;

            let write = write.borrow();
            Some(gfx::pso::DescriptorSetWrite {
                set: &instance.set,
                binding: write.binding,
                array_offset: 0,
                descriptors: Some(match write.data {
                    InstanceWriteData::Sampler { sampler } => {
                        let raw = sampler_storage.raw(sampler)?;
                        gfx::pso::Descriptor::Sampler(raw)
                    }
                    InstanceWriteData::Image { image } => {
                        let raw = image_storage.raw(image)?;
                        gfx::pso::Descriptor::Image(&raw.view, gfx::image::Layout::Undefined)
                    }
                    InstanceWriteData::Buffer { buffer, ref region } => {
                        let raw = buffer_storage.raw(buffer)?;
                        gfx::pso::Descriptor::Buffer(raw.buffer.raw(), region.clone())
                    }
                }),
            })
        });

        device.device.write_descriptor_sets(writes);

        Some(())
    }

    pub(crate) unsafe fn destroy_instances(&mut self, instances: &[MaterialInstanceHandle]) {
        for (mat_handle, inst) in instances {
            let mat = match self.storage.get_mut(*mat_handle) {
                Some(mat) => mat,
                None => continue,
            };

            mat.free_instance(*inst);
        }
    }

    pub(crate) unsafe fn release(self, device: &DeviceContext) {
        for (_id, mat) in self.storage {
            mat.release(device);
        }
    }
}

impl Material {
    fn next_nonempty_pool(&self) -> Option<usize> {
        for (i, allocd) in self.pool_allocated.iter().enumerate() {
            if *allocd < MAX_SETS_PER_POOL {
                return Some(i);
            }
        }

        None
    }

    unsafe fn create_new_pool(&mut self, device: &DeviceContext) -> usize {
        use gfx::Device;

        let descriptors = self
            .parameters
            .iter()
            .map(|(_binding, ty)| gfx::pso::DescriptorRangeDesc {
                count: 1,
                ty: ty.clone().into(),
            })
            .collect::<SmallVec<[_; 16]>>();

        let pool = device
            .device
            .create_descriptor_pool(MAX_SETS_PER_POOL as usize, descriptors.as_slice())
            .expect("Can't allocate new descriptor pool, out of memory");

        let new_pool_idx = self.pools.len();

        self.pool_used.push(0);
        self.pool_allocated.push(0);
        self.pools.push(pool);

        new_pool_idx
    }

    unsafe fn release(self, device: &DeviceContext) {
        use gfx::Device;

        for pool in self.pools {
            device.device.destroy_descriptor_pool(pool);
        }

        device
            .device
            .destroy_descriptor_set_layout(self.desc_set_layout);
    }

    unsafe fn create_instance(
        &mut self,
        device: &DeviceContext,
    ) -> Result<Handle<MaterialInstance>, gfx::pso::AllocationError> {
        use gfx::pso::DescriptorPool;

        let pool_idx = self
            .next_nonempty_pool()
            .unwrap_or_else(|| self.create_new_pool(device));

        let pool = &mut self.pools[pool_idx];
        let set = pool.allocate_set(&self.desc_set_layout)?;

        self.pool_used[pool_idx] += 1;
        self.pool_allocated[pool_idx] += 1;

        let instance = MaterialInstance {
            pool: pool_idx,
            set,
        };

        Ok(self.instances.insert(instance))
    }

    unsafe fn free_instance(&mut self, handle: Handle<MaterialInstance>) -> Option<()> {
        use gfx::pso::DescriptorPool;
        use std;

        let instance = self.instances.remove(handle)?;

        // mark as no longer used
        self.pool_used[instance.pool] -= 1;

        if self.pool_used[instance.pool] == 0 {
            // the whole thing is full, so we can reset the whole pool
            self.pools[instance.pool].reset();
            self.pool_allocated[instance.pool] = 0;
        }

        Some(())
    }

    pub(crate) fn intance_raw(
        &self,
        handle: Handle<MaterialInstance>,
    ) -> Option<&MaterialInstance> {
        self.instances.get(handle)
    }
}

// error stuff

#[derive(Clone, Fail, Debug)]
pub enum MaterialError {
    #[fail(display = "Invalid handle used")]
    InvalidHandle,

    #[fail(display = "Material could not be created because of insufficient memory")]
    CreateError(#[cause] gfx::device::OutOfMemory),

    #[fail(display = "Material instance could not be allocated")]
    AllocationError(#[cause] gfx::pso::AllocationError),
}

impl From<gfx::device::OutOfMemory> for MaterialError {
    fn from(err: gfx::device::OutOfMemory) -> Self {
        MaterialError::CreateError(err)
    }
}

impl From<gfx::pso::AllocationError> for MaterialError {
    fn from(err: gfx::pso::AllocationError) -> Self {
        MaterialError::AllocationError(err)
    }
}
