/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::device::DeviceContext;
use crate::storage::{Handle, Storage};

use crate::graph::BlendMode;
use crate::render_pass::{RenderPassHandle, RenderPassStorage};
use crate::vertex_attrib::{VertexAttribHandle, VertexAttribStorage};

use smallvec::SmallVec;

use crate::types;
use crate::types::*;

use crate::submit_group::ResourceList;

use gfx::pso;
use gfx::Device;

use std::collections::BTreeMap;

use failure_derive::Fail;

#[derive(Clone, Debug, Fail)]
pub enum PipelineError {
    #[fail(display = "Creation of pipeline was unsuccessful")]
    CreationError(#[cause] gfx::pso::CreationError),

    #[fail(display = "Shader module could not be created")]
    ShaderError(#[cause] gfx::device::ShaderError),

    #[fail(display = "Ran out of memory")]
    OutOfMemory(#[cause] gfx::device::OutOfMemory),
}

impl From<gfx::pso::CreationError> for PipelineError {
    fn from(err: gfx::pso::CreationError) -> Self {
        PipelineError::CreationError(err)
    }
}
impl From<gfx::device::ShaderError> for PipelineError {
    fn from(err: gfx::device::ShaderError) -> Self {
        PipelineError::ShaderError(err)
    }
}

impl From<gfx::device::OutOfMemory> for PipelineError {
    fn from(err: gfx::device::OutOfMemory) -> Self {
        PipelineError::OutOfMemory(err)
    }
}

pub(crate) type Result<T> = std::result::Result<T, PipelineError>;

pub(crate) type PipelineHandle = Handle<Pipeline>;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum Pipeline {
    Graphics,
    Compute,
}

pub(crate) struct GraphicsPipeline {
    pub(crate) pipeline: types::GraphicsPipeline,
    pub(crate) layout: types::PipelineLayout,
}

pub(crate) struct ComputePipeline {
    pub(crate) pipeline: types::ComputePipeline,
    pub(crate) layout: types::PipelineLayout,
}

#[derive(Clone)]
pub(crate) struct ShaderInfo<'a> {
    pub(crate) content: &'a [u8],
    pub(crate) entry: &'a str,
}

#[derive(Clone)]
pub(crate) struct GraphicsPipelineCreateInfo<'a> {
    pub(crate) primitive: crate::graph::Primitive,

    pub(crate) vertex_attribs: Option<VertexAttribHandle>,

    pub(crate) descriptor_set_layout: &'a [&'a types::DescriptorSetLayout],
    // TODO shader stage flags
    pub(crate) push_constants: &'a [std::ops::Range<u32>],
    pub(crate) blend_modes: &'a [BlendMode],

    pub(crate) shader_vertex: ShaderInfo<'a>,
    pub(crate) shader_fragment: Option<ShaderInfo<'a>>,
    pub(crate) shader_geometry: Option<ShaderInfo<'a>>,
}

#[derive(Clone)]
pub(crate) struct ComputePipelineCreateInfo<'a> {
    pub(crate) descriptor_set_layout: &'a [&'a types::DescriptorSetLayout],
    pub(crate) shader: ShaderInfo<'a>,
    // TODO shader stage flags
    pub(crate) push_constants: &'a [std::ops::Range<u32>],
}

pub(crate) struct PipelineStorage {
    graphic_pipelines: BTreeMap<usize, GraphicsPipeline>,
    compute_pipelines: BTreeMap<usize, ComputePipeline>,
    storage: Storage<Pipeline>,
}

impl PipelineStorage {
    pub(crate) fn new() -> Self {
        PipelineStorage {
            storage: Storage::new(),
            graphic_pipelines: BTreeMap::new(),
            compute_pipelines: BTreeMap::new(),
        }
    }

    pub(crate) fn create_graphics_pipelines(
        &mut self,
        device: &DeviceContext,
        render_pass_storage: &RenderPassStorage,
        vertex_attrib_storage: &VertexAttribStorage,
        render_pass_handle: RenderPassHandle,
        create_infos: &[GraphicsPipelineCreateInfo],
    ) -> SmallVec<[Result<PipelineHandle>; 16]> {
        create_infos
            .iter()
            .map(|create_info| {
                self.create_graphics_pipeline(
                    device,
                    render_pass_storage,
                    vertex_attrib_storage,
                    render_pass_handle,
                    create_info,
                )
            })
            .collect()
    }

    // I'm sorry Mike Acton
    fn create_graphics_pipeline(
        &mut self,
        device: &DeviceContext,
        render_pass_storage: &RenderPassStorage,
        vertex_attrib_storage: &VertexAttribStorage,
        render_pass_handle: RenderPassHandle,
        create_info: &GraphicsPipelineCreateInfo,
    ) -> Result<PipelineHandle> {
        struct ShaderModules {
            vertex: ShaderModule,
            fragment: Option<ShaderModule>,
            geometry: Option<ShaderModule>,
        }

        let module = ShaderModules {
            vertex: device
                .device
                .create_shader_module(create_info.shader_vertex.content)?,

            // I'd love to use Option::map() here, but then I can't use ? for errors.
            fragment: if let Some(ref frag) = create_info.shader_fragment {
                Some(device.device.create_shader_module(frag.content)?)
            } else {
                None
            },

            geometry: if let Some(ref geom) = create_info.shader_geometry {
                Some(device.device.create_shader_module(geom.content)?)
            } else {
                None
            },
        };

        let layout = device.device.create_pipeline_layout(
            create_info.descriptor_set_layout.iter().map(|d| *d),
            create_info
                .push_constants
                .iter()
                .map(|range| (pso::ShaderStageFlags::ALL, range.clone())),
        )?;

        let pipeline = {
            struct ShaderEntries<'a> {
                vertex: pso::EntryPoint<'a, back::Backend>,
                fragment: Option<pso::EntryPoint<'a, back::Backend>>,
                geometry: Option<pso::EntryPoint<'a, back::Backend>>,
            };

            let shader_entries = {
                ShaderEntries {
                    vertex: pso::EntryPoint {
                        entry: create_info.shader_vertex.entry,
                        module: &module.vertex,
                        specialization: pso::Specialization {
                            constants: &[],
                            data: &[],
                        },
                    },
                    fragment: create_info
                        .shader_fragment
                        .as_ref()
                        .map(|s| pso::EntryPoint {
                            entry: s.entry,
                            module: module.fragment.as_ref().unwrap(),
                            specialization: pso::Specialization {
                                constants: &[],
                                data: &[],
                            },
                        }),
                    geometry: create_info
                        .shader_geometry
                        .as_ref()
                        .map(|s| pso::EntryPoint {
                            entry: s.entry,
                            module: module.geometry.as_ref().unwrap(),
                            specialization: pso::Specialization {
                                constants: &[],
                                data: &[],
                            },
                        }),
                }
            };

            let shaders = pso::GraphicsShaderSet {
                vertex: shader_entries.vertex,
                hull: None,
                domain: None,
                geometry: shader_entries.geometry,
                fragment: shader_entries.fragment,
            };

            let primitive = create_info.primitive.into();

            let rasterizer = { pso::Rasterizer::FILL };

            let render_pass = render_pass_storage.raw(render_pass_handle).unwrap();

            let subpass = gfx::pass::Subpass {
                index: 0,
                main_pass: render_pass,
            };

            let mut desc =
                pso::GraphicsPipelineDesc::new(shaders, primitive, rasterizer, &layout, subpass);

            // TODO add attributes
            if let Some(attrib) = &create_info.vertex_attribs {
                if let Some(data) = vertex_attrib_storage.raw(*attrib) {
                    for buffer in &data.buffers {
                        desc.vertex_buffers.push(pso::VertexBufferDesc {
                            binding: buffer.binding as _,
                            stride: buffer.stride as _,
                            rate: 0,
                        });
                    }

                    desc.attributes.extend_from_slice(&data.attribs[..]);
                }
            }

            // TODO allow for finer control over blend modes?
            desc.blender
                .targets
                .extend(create_info.blend_modes.iter().map(|mode| {
                    pso::ColorBlendDesc(
                        pso::ColorMask::ALL,
                        match mode {
                            BlendMode::Add => pso::BlendState::ADD,
                            BlendMode::Alpha => pso::BlendState::ALPHA,
                            BlendMode::Mul => pso::BlendState::MULTIPLY,
                        },
                    )
                }));

            device.device.create_graphics_pipeline(&desc, None)?
        };

        // destroy shader modules
        {
            let ShaderModules {
                vertex,
                fragment,
                geometry,
            } = module;

            device.device.destroy_shader_module(vertex);

            fragment.map(|frag| device.device.destroy_shader_module(frag));

            geometry.map(|geom| device.device.destroy_shader_module(geom));
        }

        let handle = self.storage.insert(Pipeline::Graphics);

        self.graphic_pipelines
            .insert(handle.id(), GraphicsPipeline { pipeline, layout });

        Ok(handle)
    }

    pub(crate) fn create_compute_pipelines(
        &mut self,
        device: &DeviceContext,
        create_infos: &[ComputePipelineCreateInfo],
    ) -> SmallVec<[Result<PipelineHandle>; 16]> {
        create_infos
            .iter()
            .map(|create_info| self.create_compute_pipeline(device, create_info))
            .collect()
    }

    pub(crate) fn create_compute_pipeline(
        &mut self,
        device: &DeviceContext,
        create_info: &ComputePipelineCreateInfo,
    ) -> Result<PipelineHandle> {
        let shader_module = device
            .device
            .create_shader_module(create_info.shader.content)?;

        let layout = device.device.create_pipeline_layout(
            create_info.descriptor_set_layout.iter().map(|d| *d),
            create_info
                .push_constants
                .iter()
                .map(|range| (pso::ShaderStageFlags::COMPUTE, range.clone())),
        )?;

        let pipeline = {
            let shader_entry = pso::EntryPoint {
                entry: create_info.shader.entry,
                module: &shader_module,
                specialization: pso::Specialization {
                    constants: &[],
                    data: &[],
                },
            };

            let desc = pso::ComputePipelineDesc::new(shader_entry, &layout);

            device.device.create_compute_pipeline(&desc, None)?
        };

        device.device.destroy_shader_module(shader_module);

        let handle = self.storage.insert(Pipeline::Compute);

        self.compute_pipelines
            .insert(handle.id(), ComputePipeline { pipeline, layout });

        Ok(handle)
    }

    pub(crate) fn destroy<P>(&mut self, res_list: &mut ResourceList, pipelines: P)
    where
        P: IntoIterator,
        P::Item: std::borrow::Borrow<PipelineHandle>,
    {
        use std::borrow::Borrow;

        for handle in pipelines.into_iter() {
            let handle = *handle.borrow();

            if self.storage.remove(handle).is_some() {
                if let Some(gfx) = self.graphic_pipelines.remove(&handle.0) {
                    res_list.queue_pipeline_graphic(gfx.pipeline);
                    res_list.queue_pipeline_layout(gfx.layout);
                }
                if let Some(cmpt) = self.compute_pipelines.remove(&handle.0) {
                    res_list.queue_pipeline_compute(cmpt.pipeline);
                    res_list.queue_pipeline_layout(cmpt.layout);
                }
            }
        }
    }

    pub(crate) fn raw_graphics(&self, handle: PipelineHandle) -> Option<&GraphicsPipeline> {
        if self.storage.is_alive(handle) {
            let ty = self.storage[handle];
            if ty == Pipeline::Graphics {
                Some(&self.graphic_pipelines[&handle.0])
            } else {
                None
            }
        } else {
            None
        }
    }

    pub(crate) fn raw_compute(&self, handle: PipelineHandle) -> Option<&ComputePipeline> {
        if self.storage.is_alive(handle) {
            let ty = self.storage[handle];
            if ty == Pipeline::Compute {
                Some(&self.compute_pipelines[&handle.0])
            } else {
                None
            }
        } else {
            None
        }
    }
}
