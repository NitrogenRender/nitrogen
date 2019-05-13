/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Description of "input data to shaders".

use crate::util::storage::{Handle, Storage};

use crate::device::DeviceContext;

use crate::resources::buffer::{BufferHandle, BufferStorage};
use crate::resources::image::{ImageHandle, ImageStorage};
use crate::resources::sampler::{SamplerHandle, SamplerStorage};

use crate::types;

use std::ops::Range;

use smallvec::SmallVec;

/// Opaque handle to a [`Material`].
///
/// [`Material`]: ./struct.Material.html
pub type MaterialHandle = Handle<Material>;

const MAX_SETS_PER_POOL: u8 = 16;

/// Opaque handle to a [`MaterialInstance`].
///
/// [`MaterialInstance`]: ./struct.MaterialInstance.html
#[repr(C)]
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct MaterialInstanceHandle {
    pub(crate) material: MaterialHandle,
    pub(crate) instance: Handle<MaterialInstance>,
}

impl MaterialInstanceHandle {
    /// Return the material this instance represents.
    pub fn material(self) -> MaterialHandle {
        self.material
    }
}

// Implementation detail:
// A material contains its parameters for validation and later pool creation.
// Since the number of material instances is not always known at program startup,
// a list of pools is maintained and expanded if needed.

/// A "blue print" for input-data to shader programs.
///
/// Materials are a collection of associated data that acts as a "description" for abstract objects.
///
/// ## Conceptual example:
///
/// A PBR pipeline might require mesh instances to provide following information:
///  - albedo texture
///  - roughness texture
///  - normal texture
///  - emission texture
///  - uniform buffer (model matrix, roughness multiplier, normal multiplier, ...)
///  - uniform buffer (bone transforms, ...)
///
/// A 2D pipeline instead might only require canvas-objects to provide following information:
///  - Color texture
///
/// Materials are used to "talk about" those properties, so that pipelines can be prepared
/// accordingly. Material instances provide concrete values for each of those properties.
pub struct Material {
    sets_per_pool: u8,

    parameters: Vec<(u32, MaterialParameterType)>,
    pub(crate) desc_set_layout: types::DescriptorSetLayout,
    pool_allocated: Vec<u8>,
    pool_used: Vec<u8>,
    pools: Vec<types::DescriptorPool>,

    instances: Storage<MaterialInstance>,
}

/// Information needed to create a material object.
pub struct MaterialCreateInfo<'a> {
    // TODO add support for arrays later on?
    /// Paramters of the material.
    pub parameters: &'a [(u32, MaterialParameterType)],
}

/// An instance of a material.
///
/// Material instances are specific instantiations of a material which can be used as bindings in
/// shader programs. (DescriptorSets in Vulkan terminology)
pub struct MaterialInstance {
    pool: usize,
    pub(crate) set: types::DescriptorSet,
}

pub(crate) struct MaterialStorage {
    storage: Storage<Material>,
}

/// Type of material parameter.
#[allow(missing_docs)]
#[derive(Copy, Clone, Debug)]
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

/// Description of a data-write to a material-instance.
pub struct InstanceWrite {
    /// Binding point of the data.
    pub binding: u32,
    /// Data to write.
    pub data: InstanceWriteData,
}

/// Data to write to an instance.
pub enum InstanceWriteData {
    /// A sampler object.
    Sampler {
        /// Handle to sampler object.
        sampler: SamplerHandle,
    },
    /// An image object.
    Image {
        /// Handle to image object.
        image: ImageHandle,
    },
    /// A buffer object.
    Buffer {
        /// Handle to buffer object.
        buffer: BufferHandle,
        /// Region of the buffer to map (in bytes).
        region: Range<Option<u64>>,
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
        create_info: MaterialCreateInfo,
    ) -> Result<MaterialHandle, MaterialError> {
        use gfx::Device;

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

        if descriptors.is_empty() {
            return Err(MaterialError::CreateEmptyMaterial);
        }

        let set = device
            .device
            .create_descriptor_set_layout(descriptors.as_slice(), &[])?;

        let mut parameters = create_info.parameters.to_vec();
        parameters.sort_by_key(|(binding, _)| *binding);

        let mat = Material {
            sets_per_pool: MAX_SETS_PER_POOL,
            parameters,
            desc_set_layout: set,
            instances: Storage::new(),
            pool_allocated: Vec::new(),
            pool_used: Vec::new(),
            pools: Vec::new(),
        };
        let handle = self.storage.insert(mat);
        Ok(handle)
    }

    pub(crate) unsafe fn create_raw<P>(
        &mut self,
        device: &DeviceContext,
        layout: types::DescriptorSetLayout,
        params: P,
        sets_per_pool: u8,
    ) -> Option<MaterialHandle>
    where
        P: Iterator<Item = gfx::pso::DescriptorRangeDesc>,
    {
        let mat = Material {
            parameters: params
                .map(|desc| {
                    use gfx::pso::DescriptorType;
                    // Since the binding is only used when creating a material
                    // from a user-facing description, we can just put anything in here
                    // since it will never be used afterwards.
                    let ty = match desc.ty {
                        DescriptorType::Sampler => MaterialParameterType::Sampler,
                        DescriptorType::SampledImage => MaterialParameterType::SampledImage,
                        DescriptorType::StorageImage => MaterialParameterType::StorageImage,
                        DescriptorType::UniformTexelBuffer => {
                            MaterialParameterType::UniformTexelBuffer
                        }
                        DescriptorType::StorageTexelBuffer => {
                            MaterialParameterType::StorageTexelBuffer
                        }
                        DescriptorType::UniformBuffer => MaterialParameterType::UniformBuffer,
                        DescriptorType::StorageBuffer => MaterialParameterType::StorageBuffer,
                        DescriptorType::UniformBufferDynamic => {
                            MaterialParameterType::UniformBufferDynamic
                        }
                        DescriptorType::StorageBufferDynamic => {
                            MaterialParameterType::StorageBufferDynamic
                        }
                        _ => unreachable!(),
                    };

                    (0, ty)
                })
                .collect(),
            sets_per_pool,
            desc_set_layout: layout,
            instances: Storage::new(),
            pool_allocated: vec![],
            pool_used: vec![],
            pools: vec![],
        };

        if mat.parameters.is_empty() {
            use gfx::Device;

            device
                .device
                .destroy_descriptor_set_layout(mat.desc_set_layout);
            return None;
        }

        Some(self.storage.insert(mat))
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

    pub(crate) unsafe fn create_instance(
        &mut self,
        device: &DeviceContext,
        material: MaterialHandle,
    ) -> Result<MaterialInstanceHandle, MaterialError> {
        let mat = self
            .storage
            .get_mut(material)
            .ok_or(MaterialError::InvalidHandle)?;

        let instance = mat.create_instance(device)?;

        Ok(MaterialInstanceHandle { material, instance })
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

        let mat = self.storage.get(instance.material)?;

        let instance = mat.instances.get(instance.instance)?;

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
                        gfx::pso::Descriptor::Sampler(&raw.0)
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
        for MaterialInstanceHandle { material, instance } in instances {
            let mat = match self.storage.get_mut(*material) {
                Some(mat) => mat,
                None => continue,
            };

            mat.free_instance(*instance);
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
            if *allocd < self.sets_per_pool {
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
                count: self.sets_per_pool as usize,
                ty: ty.clone().into(),
            })
            .collect::<SmallVec<[_; 16]>>();

        let pool = device
            .device
            .create_descriptor_pool(
                self.sets_per_pool as usize,
                descriptors.as_slice(),
                gfx::pso::DescriptorPoolCreateFlags::empty(),
            )
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

    pub(crate) fn instance_raw(
        &self,
        handle: Handle<MaterialInstance>,
    ) -> Option<&MaterialInstance> {
        self.instances.get(handle)
    }
}

// error stuff

/// Possible errors that can occur when creating materials or material instances.
#[allow(missing_docs)]
#[derive(Clone, Display, From, Debug)]
pub enum MaterialError {
    #[display(fmt = "Cannot create empty material")]
    CreateEmptyMaterial,

    #[display(fmt = "Invalid handle used")]
    InvalidHandle,

    #[display(fmt = "Material could not be created because of insufficient memory")]
    CreateError(gfx::device::OutOfMemory),

    #[display(fmt = "Material instance could not be allocated")]
    AllocationError(gfx::pso::AllocationError),
}

impl std::error::Error for MaterialError {}
