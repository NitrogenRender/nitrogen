use crate::device::DeviceContext;
use crate::graph::builder::resource_descriptor::{
    ResourceReadType, ResourceType, ResourceWriteType,
};
use crate::graph::compilation::CompileError::ResourceTypeMismatch;
use crate::graph::compilation::{CompiledGraph, ResourceId};
use crate::graph::execution::{
    create_pipeline_compute, GraphResources, PassResources, PipelineResources,
};
use crate::graph::pass::command::{ComputeCommandBuffer, ReadStorages};
use crate::graph::pass::{ComputePass, PassId};
use crate::graph::{ComputePassAccessor, Graph, PrepareError, ResourceName, Storages};
use crate::resources::buffer::BufferHandle;
use crate::resources::image::ImageHandle;
use crate::resources::pipeline::PipelineError;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Copy, Clone)]
pub enum ResourceAccessType {
    Read(ResourceType),
    Write(ResourceType),
}

impl ResourceAccessType {
    fn compatible(self, attempted: ResourceAccessType) -> bool {
        use self::ResourceAccessType::*;

        match (self, attempted) {
            (Read(ty_a), Read(ty_b)) => ty_a == ty_b,
            (Write(ty_a), Write(ty_b)) => ty_a == ty_b,
            // write can "coerce" to read.
            (Write(ty_a), Read(ty_b)) => ty_a == ty_b,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ResourceRefError {
    InvalidResourceReferenced {
        pass: PassId,
        name: ResourceName,
    },
    AccessViolation {
        pass: PassId,
        resource: ResourceName,
        attempted: ResourceAccessType,
        expected: ResourceAccessType,
    },
    ResourceNotUsableInPass {
        pass: PassId,
        name: ResourceName,
    },
}

pub struct ImageWriteRef(pub(crate) ImageHandle);
pub struct ImageReadRef(pub(crate) ImageHandle);

pub struct BufferWriteRef(pub(crate) BufferHandle);
pub struct BufferReadRef(pub(crate) BufferHandle);

pub(crate) struct RawComputeDispatcher<'a> {
    pub(crate) cmd: &'a mut crate::resources::command_pool::CmdBufType<gfx::Compute>,
    pub(crate) device: &'a DeviceContext,
    pub(crate) storages: &'a Storages<'a>,
    pub(crate) pass_id: PassId,
    pub(crate) pass_res: &'a mut PassResources,
    pub(crate) graph_res: &'a GraphResources,
    pub(crate) compiled: &'a CompiledGraph,
}

impl<'a> RawComputeDispatcher<'a> {
    pub(crate) fn into_typed_dispatcher<T: ComputePass>(
        self,
        pass_impl: Rc<RefCell<T>>,
    ) -> ComputeDispatcher<'a, T> {
        ComputeDispatcher {
            cmd: self.cmd,
            device: self.device,
            storages: self.storages,
            pass_id: self.pass_id,
            pass_res: self.pass_res,
            graph_res: self.graph_res,
            compiled: self.compiled,
            pass_impl,
        }
    }
}

pub struct ComputeDispatcher<'a, T: ComputePass> {
    pub(crate) cmd: &'a mut crate::resources::command_pool::CmdBufType<gfx::Compute>,
    pub(crate) device: &'a DeviceContext,
    pub(crate) storages: &'a Storages<'a>,
    pub(crate) pass_id: PassId,
    pub(crate) pass_res: &'a mut PassResources,
    pub(crate) graph_res: &'a GraphResources,
    pub(crate) compiled: &'a CompiledGraph,
    pub(crate) pass_impl: Rc<RefCell<T>>,
}

impl<'a, T: ComputePass> ComputeDispatcher<'a, T> {
    // Find the allowed access type of a resource.
    fn resource_ref(
        &self,
        name: ResourceName,
        attempted: ResourceAccessType,
    ) -> Result<ResourceId, ResourceRefError> {
        let res_id = *self.compiled.graph_resources.name_lookup.get(&name).ok_or(
            ResourceRefError::InvalidResourceReferenced {
                pass: self.pass_id,
                name: name.clone(),
            },
        )?;

        let allowed = self
            .compiled
            .graph_resources
            .resource_access_type(self.pass_id, res_id)
            .ok_or(ResourceRefError::ResourceNotUsableInPass {
                pass: self.pass_id,
                name: name.clone(),
            })?;

        if !allowed.compatible(attempted) {
            return Err(ResourceRefError::AccessViolation {
                pass: self.pass_id,
                resource: name,
                expected: allowed,
                attempted,
            });
        }

        Ok(res_id)
    }

    pub fn image_write_ref(
        &self,
        name: impl Into<ResourceName>,
    ) -> Result<ImageWriteRef, ResourceRefError> {
        let attempted = ResourceAccessType::Write(ResourceType::Image);

        let id = self.resource_ref(name.into(), attempted)?;

        let handle = self
            .graph_res
            .images
            .get(&id)
            .expect("GraphResources should be compatible");

        Ok(ImageWriteRef(*handle))
    }

    pub fn image_read_ref(
        &self,
        name: impl Into<ResourceName>,
    ) -> Result<ImageReadRef, ResourceRefError> {
        let attempted = ResourceAccessType::Read(ResourceType::Image);

        let id = self.resource_ref(name.into(), attempted)?;

        let handle = self
            .graph_res
            .images
            .get(&id)
            .expect("GraphResources should be compatible");

        Ok(ImageReadRef(*handle))
    }

    pub fn buffer_write_ref(
        &self,
        name: impl Into<ResourceName>,
    ) -> Result<BufferWriteRef, ResourceRefError> {
        let attempted = ResourceAccessType::Write(ResourceType::Buffer);

        let id = self.resource_ref(name.into(), attempted)?;

        let handle = self
            .graph_res
            .buffers
            .get(&id)
            .expect("GraphResources should be compatible");

        Ok(BufferWriteRef(*handle))
    }

    pub fn buffer_read_ref(
        &self,
        name: impl Into<ResourceName>,
    ) -> Result<BufferReadRef, ResourceRefError> {
        let attempted = ResourceAccessType::Read(ResourceType::Buffer);

        let id = self.resource_ref(name.into(), attempted)?;

        let handle = self
            .graph_res
            .buffers
            .get(&id)
            .expect("GraphResources should be compatible");

        Ok(BufferReadRef(*handle))
    }

    pub unsafe fn with_config<F, R>(&mut self, config: T::Config, f: F) -> Result<R, PrepareError>
    where
        F: FnOnce(&mut ComputeCommandBuffer) -> R,
    {
        // TODO fetch this from a cache rather than calling this every time.
        let desc = self.pass_impl.borrow().configure(config);

        // fetch pipeline from cache or create a new one.
        let compute_pipelines = &mut self.pass_res.compute_pipelines;
        let pass_materials = &mut self.pass_res.pass_material;

        let pipelines = compute_pipelines.entry(self.pass_id).or_default();

        if !pipelines.contains_key(&desc) {
            // create new pipeline!
            let pass_mat = pass_materials.get(&self.pass_id).map(|mat| mat.material());

            let pipe =
                create_pipeline_compute(self.device, self.storages, self.pass_id, pass_mat, &desc)?;

            pipelines.insert(
                desc.clone(),
                PipelineResources {
                    pipeline_handle: pipe,
                },
            );
        }

        let pipeline_storage = self.storages.pipeline.borrow();

        let read_storages = ReadStorages {
            buffer: self.storages.buffer.borrow(),
            material: self.storages.material.borrow(),
            image: self.storages.image.borrow(),
        };

        let pipe = pipelines.get(&desc).unwrap();

        let pipe_raw = pipeline_storage.raw_compute(pipe.pipeline_handle).unwrap();

        self.cmd.bind_compute_pipeline(&pipe_raw.pipeline);

        let mut cmd = {
            ComputeCommandBuffer {
                buf: self.cmd,
                storages: &read_storages,
                pipeline_layout: &pipe_raw.layout,
            }
        };

        let res = f(&mut cmd);

        Ok(res)
    }
}
