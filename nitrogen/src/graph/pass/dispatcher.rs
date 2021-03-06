/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Dispatchers are used to retrieve references to graph-resources and dispatch calls to
//! the underlying pipeline objects.

use crate::device::DeviceContext;

use crate::graph::builder::resource_descriptor::ResourceType;
use crate::graph::builder::{ComputePassContext, GraphicsPassContext};
use crate::graph::execution::{
    create_pipeline_compute, GraphResources, PassResources, PipelineResources,
};
use crate::graph::pass::command::{ComputeCommandBuffer, ReadStorages};
use crate::graph::pass::{ComputePass, PassId};
use crate::graph::{CompiledGraph, PrepareError, ResourceId, ResourceName, Storages};
use crate::resources::buffer::BufferHandle;
use crate::resources::image::ImageHandle;
use std::cell::RefCell;
use std::rc::Rc;

/// Access type of a resource.
#[derive(Debug, Copy, Clone)]
pub enum ResourceAccessType {
    /// Read access.
    Read(ResourceType),

    /// Write access.
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

/// Errors that can occur when acquiring references to graph resources.
#[derive(Debug, Clone)]
pub enum ResourceRefError {
    /// An invalid resource was requested.
    ///
    /// This resource was not defined in the graph.
    InvalidResourceReferenced {
        /// Pass which requested the resource.
        pass: PassId,
        /// Name of the resource.
        name: ResourceName,
    },
    /// A resource was requested with an unsupported access type.
    AccessViolation {
        /// Pass which requested the resource.
        pass: PassId,
        /// Name of the resource.
        resource: ResourceName,
        /// The requested access type of the resource.
        attempted: ResourceAccessType,
        /// The support access type of the resource.
        expected: ResourceAccessType,
    },
    /// A resource was requested that is unaccessable from the given pass.
    ResourceNotUsableInPass {
        /// Pass which requested the resource.
        pass: PassId,
        /// Name of the resource.
        name: ResourceName,
    },
}

/// A reference handle to an image resource with write access.
#[derive(Clone, Copy, Debug)]
pub struct ImageWriteRef(pub(crate) ImageHandle);
/// A reference handle to an image resource with read access.
#[derive(Clone, Copy, Debug)]
pub struct ImageReadRef(pub(crate) ImageHandle);

/// A reference handle to a buffer resource with write access.
#[derive(Clone, Copy, Debug)]
pub struct BufferWriteRef(pub(crate) BufferHandle);
/// A reference handle to a buffer resource with read access.
#[derive(Clone, Copy, Debug)]
pub struct BufferReadRef(pub(crate) BufferHandle);

pub use self::compute::*;
mod compute {
    use super::*;

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
            pass_impl: Rc<RefCell<ComputePassContext<T>>>,
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

    /// A dispatcher for compute passes.
    ///
    /// Used to retrieve graph-resource references and dispatch calls to the underlying pipelines.
    pub struct ComputeDispatcher<'a, T: ComputePass> {
        pub(crate) cmd: &'a mut crate::resources::command_pool::CmdBufType<gfx::Compute>,
        pub(crate) device: &'a DeviceContext,
        pub(crate) storages: &'a Storages<'a>,
        pub(crate) pass_id: PassId,
        pub(crate) pass_res: &'a mut PassResources,
        pub(crate) graph_res: &'a GraphResources,
        pub(crate) compiled: &'a CompiledGraph,
        pub(crate) pass_impl: Rc<RefCell<ComputePassContext<T>>>,
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

        // resource access

        /// Retrieve a write-reference to an image resource.
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

        /// Retrieve a read-reference to an image resource.
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

        /// Retrieve a write-reference to a buffer resource.
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

        /// Retrieve a read-reference to a buffer resource.
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

        // pipeline config

        /// Create a command-buffer and execute the given closure with a pipeline that supports
        /// the configuration given by `config`.
        pub unsafe fn with_config<F, R>(
            &mut self,
            config: T::Config,
            f: F,
        ) -> Result<R, PrepareError>
        where
            F: FnOnce(&mut ComputeCommandBuffer) -> R,
        {
            let material_storage = self.storages.material.borrow();

            let pass_impl = self.pass_impl.borrow();

            let mut pipeline_desc_cache = pass_impl.pipeline_infos.borrow_mut();

            let desc = if let Some(desc) = pipeline_desc_cache.get(&config) {
                desc
            } else {
                let desc = pass_impl.pass.configure(&config);
                pipeline_desc_cache.entry(config).or_insert(desc)
            };

            // fetch pipeline from cache or create a new one.
            let compute_pipelines = &mut self.pass_res.compute_pipelines;
            let pass_materials = &mut self.pass_res.pass_material;

            let pass_mat = pass_materials.get(&self.pass_id).cloned();

            let pipelines = compute_pipelines.entry(self.pass_id).or_default();

            if !pipelines.contains_key(&desc) {
                // create new pipeline!
                let pipe = create_pipeline_compute(
                    self.device,
                    self.storages,
                    self.pass_id,
                    pass_mat,
                    &desc,
                )?;

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
                _image: self.storages.image.borrow(),
            };

            let pipe = pipelines.get(&desc).unwrap();

            let pipe_raw = pipeline_storage.raw_compute(pipe.pipeline_handle).unwrap();

            self.cmd.bind_compute_pipeline(&pipe_raw.pipeline);

            // pass material exists, bind it.
            if let Some(mat) = pass_mat {
                let instance = {
                    let mat = material_storage.raw(mat).unwrap();

                    let instance = self.graph_res.pass_mat_instances[&self.pass_id];

                    mat.instance_raw(instance.instance).unwrap()
                };

                self.cmd.bind_compute_descriptor_sets(
                    &pipe_raw.layout,
                    0,
                    Some(&instance.set),
                    &[],
                );
            }

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
}

pub use self::graphics::*;
mod graphics {
    use super::*;
    use crate::graph::builder::resource_descriptor::ImageClearValue;
    use crate::graph::execution::create_pipeline_graphics;
    use crate::graph::pass::command::GraphicsCommandBuffer;
    use crate::graph::pass::GraphicsPass;

    pub(crate) struct RawGraphicsDispatcher<'a> {
        pub(crate) cmd: &'a mut crate::resources::command_pool::CmdBufType<gfx::Graphics>,
        pub(crate) device: &'a DeviceContext,
        pub(crate) storages: &'a Storages<'a>,
        pub(crate) pass_id: PassId,
        pub(crate) pass_res: &'a mut PassResources,
        pub(crate) graph_res: &'a GraphResources,
        pub(crate) compiled: &'a CompiledGraph,
    }

    impl<'a> RawGraphicsDispatcher<'a> {
        pub(crate) fn into_typed_dispatcher<T: GraphicsPass>(
            self,
            pass_impl: Rc<RefCell<GraphicsPassContext<T>>>,
        ) -> GraphicsDispatcher<'a, T> {
            GraphicsDispatcher {
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

    /// A dispatcher for graphic passes.
    ///
    /// Used to retrieve graph-resource references and dispatch calls to the underlying pipelines.
    pub struct GraphicsDispatcher<'a, T: GraphicsPass> {
        pub(crate) cmd: &'a mut crate::resources::command_pool::CmdBufType<gfx::Graphics>,
        pub(crate) device: &'a DeviceContext,
        pub(crate) storages: &'a Storages<'a>,
        pub(crate) pass_id: PassId,
        pub(crate) pass_res: &'a mut PassResources,
        pub(crate) graph_res: &'a GraphResources,
        pub(crate) compiled: &'a CompiledGraph,

        pub(crate) pass_impl: Rc<RefCell<GraphicsPassContext<T>>>,
    }

    impl<'a, T: GraphicsPass> GraphicsDispatcher<'a, T> {
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

        // resource access

        /// Retrieve a write-reference to an image resource.
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

        /// Retrieve a read-reference to an image resource.
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

        /// Retrieve a write-reference to a buffer resource.
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

        /// Retrieve a read-reference to a buffer resource.
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

        // image clearing.

        /// Dispatch a clearing command for image `image` using the clear value `clear`.
        pub unsafe fn clear_image(
            &mut self,
            image: ImageWriteRef,
            clear: ImageClearValue,
        ) -> Option<()> {
            let image_storage = self.storages.image.borrow();

            let img = image_storage.raw(image.0)?;

            let entry_barrier = gfx::memory::Barrier::Image {
                states: (gfx::image::Access::empty(), gfx::image::Layout::Undefined)
                    ..(
                        gfx::image::Access::TRANSFER_WRITE,
                        gfx::image::Layout::TransferDstOptimal,
                    ),
                target: img.image.raw(),
                families: None,
                range: gfx::image::SubresourceRange {
                    aspects: img.aspect,
                    levels: 0..1,
                    layers: 0..1,
                },
            };

            self.cmd.pipeline_barrier(
                gfx::pso::PipelineStage::TOP_OF_PIPE..gfx::pso::PipelineStage::TRANSFER,
                gfx::memory::Dependencies::empty(),
                &[entry_barrier],
            );

            self.cmd.clear_image(
                img.image.raw(),
                gfx::image::Layout::TransferDstOptimal,
                match clear {
                    ImageClearValue::Color(color) => gfx::command::ClearColor::Float(color),
                    _ => gfx::command::ClearColor::Float([0.0; 4]),
                },
                match clear {
                    ImageClearValue::DepthStencil(depth, stencil) => {
                        gfx::command::ClearDepthStencil(depth, stencil)
                    }
                    _ => gfx::command::ClearDepthStencil(1.0, 0),
                },
                &[gfx::image::SubresourceRange {
                    aspects: img.aspect,
                    levels: 0..1,
                    layers: 0..1,
                }],
            );

            let exit_barrier = gfx::memory::Barrier::Image {
                states: (
                    gfx::image::Access::TRANSFER_WRITE,
                    gfx::image::Layout::TransferDstOptimal,
                )
                    ..(gfx::image::Access::empty(), gfx::image::Layout::General),
                target: img.image.raw(),
                families: None,
                range: gfx::image::SubresourceRange {
                    aspects: img.aspect,
                    levels: 0..1,
                    layers: 0..1,
                },
            };

            self.cmd.pipeline_barrier(
                gfx::pso::PipelineStage::TRANSFER..gfx::pso::PipelineStage::BOTTOM_OF_PIPE,
                gfx::memory::Dependencies::empty(),
                &[exit_barrier],
            );

            Some(())
        }

        // pipelines

        /// Create a command-buffer and execute the given closure with a pipeline that supports
        /// the configuration given by `config`.
        pub unsafe fn with_config<F, R>(
            &mut self,
            config: T::Config,
            f: F,
        ) -> Result<R, PrepareError>
        where
            F: FnOnce(&mut GraphicsCommandBuffer) -> R,
        {
            let material_storage = self.storages.material.borrow();
            let render_pass_storage = self.storages.render_pass.borrow();

            let render_pass_handle = self.pass_res.render_passes[&self.pass_id];

            let pass_impl = self.pass_impl.borrow();

            let mut pipeline_desc_cache = pass_impl.pipeline_infos.borrow_mut();

            let desc = if let Some(desc) = pipeline_desc_cache.get(&config) {
                desc
            } else {
                let desc = pass_impl.pass.configure(&config);
                pipeline_desc_cache.entry(config).or_insert(desc)
            };

            let graphics_pipelines = &mut self.pass_res.graphic_pipelines;
            let pass_materials = &mut self.pass_res.pass_material;

            let pass_mat = pass_materials.get(&self.pass_id).cloned();
            let pipelines = graphics_pipelines.entry(self.pass_id).or_default();

            if !pipelines.contains_key(desc) {
                // create new pipeline!!
                let pipe = create_pipeline_graphics(
                    self.device,
                    self.storages,
                    self.pass_id,
                    pass_mat,
                    &desc,
                    render_pass_handle,
                )?;

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
                _image: self.storages.image.borrow(),
            };

            let render_pass = render_pass_storage
                .raw(render_pass_handle)
                .ok_or(PrepareError::InvalidRenderPass)?;

            let pipe = pipelines.get(&desc).unwrap();

            let pipe_raw = pipeline_storage.raw_graphics(pipe.pipeline_handle).unwrap();

            let (fb, fb_extent) = {
                self.graph_res
                    .framebuffers
                    .get(&self.pass_id)
                    .ok_or(PrepareError::InvalidFramebuffer)?
            };

            let viewport = gfx::pso::Viewport {
                // TODO depth boundaries
                depth: 0.0..1.0,
                rect: gfx::pso::Rect {
                    x: 0,
                    y: 0,
                    w: fb_extent.width as i16,
                    h: fb_extent.height as i16,
                },
            };

            let ret = {
                self.cmd.bind_graphics_pipeline(&pipe_raw.pipeline);

                // pass material exists, bind it.
                if let Some(mat) = pass_mat {
                    let instance = {
                        let mat = material_storage.raw(mat).unwrap();

                        let instance = self.graph_res.pass_mat_instances[&self.pass_id];

                        mat.instance_raw(instance.instance).unwrap()
                    };

                    self.cmd.bind_graphics_descriptor_sets(
                        &pipe_raw.layout,
                        0,
                        Some(&instance.set),
                        &[],
                    );
                }

                self.cmd.set_viewports(0, &[viewport.clone()]);
                self.cmd.set_scissors(0, &[viewport.rect]);

                {
                    let encoder =
                        self.cmd
                            .begin_render_pass_inline(render_pass, fb, viewport.rect, &[]);

                    let mut command = GraphicsCommandBuffer {
                        storages: &read_storages,
                        viewport_rect: viewport.rect,
                        pipeline_layout: &pipe_raw.layout,
                        encoder,
                    };

                    f(&mut command)
                }
            };

            Ok(ret)
        }
    }
}
