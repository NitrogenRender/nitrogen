use device::DeviceContext;
use storage::{Handle, Storage};

use render_pass::{RenderPassHandle, RenderPassStorage};
use vertex_attrib::{VertexAttribHandle, VertexAttribStorage};

use smallvec::SmallVec;

use types;
use types::*;

use gfx;
use gfx::pso;
use gfx::Device;

use back;

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

pub type Result<T> = std::result::Result<T, PipelineError>;

pub type PipelineHandle = Handle<Pipeline>;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Pipeline {
    Graphics,
    Compute,
}

pub(crate) struct GraphicsPipeline {
    pub(crate) pipeline: types::GraphicsPipeline,
}

struct ComputePipeline {}

#[derive(Clone)]
pub struct ShaderInfo<'a> {
    pub content: &'a [u8],
    pub entry: &'a str,
}

#[derive(Ord, PartialOrd, Eq, PartialEq, Copy, Clone, Hash, Debug)]
pub enum Primitive {
    PointList,
    LineList,
    LineStrip,
    TriangleList,
    TriangleStrip,
}

impl From<Primitive> for gfx::Primitive {
    fn from(p: Primitive) -> Self {
        match p {
            Primitive::PointList => gfx::Primitive::PointList,
            Primitive::LineList => gfx::Primitive::LineList,
            Primitive::LineStrip => gfx::Primitive::LineStrip,
            Primitive::TriangleList => gfx::Primitive::TriangleList,
            Primitive::TriangleStrip => gfx::Primitive::TriangleStrip,
        }
    }
}

#[derive(Clone)]
pub struct GraphicsPipelineCreateInfo<'a> {
    pub primitive: Primitive,

    pub vertex_attribs: Option<VertexAttribHandle>,

    pub shader_vertex: ShaderInfo<'a>,
    pub shader_fragment: Option<ShaderInfo<'a>>,
    pub shader_geometry: Option<ShaderInfo<'a>>,
}

pub struct PipelineStorage {
    graphic_pipelines: BTreeMap<usize, GraphicsPipeline>,
    compute_pipelines: BTreeMap<usize, ComputePipeline>,
    storage: Storage<Pipeline>,
}

impl PipelineStorage {
    pub fn new() -> Self {
        PipelineStorage {
            storage: Storage::new(),
            graphic_pipelines: BTreeMap::new(),
            compute_pipelines: BTreeMap::new(),
        }
    }

    pub fn create_graphics_pipelines(
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
                    create_info.clone(),
                )
            }).collect()
    }

    // I'm sorry Mike Acton
    fn create_graphics_pipeline(
        &mut self,
        device: &DeviceContext,
        render_pass_storage: &RenderPassStorage,
        vertex_attrib_storage: &VertexAttribStorage,
        render_pass_handle: RenderPassHandle,
        create_info: GraphicsPipelineCreateInfo,
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

            // TODO what about descriptor sets?
            let layout = { device.device.create_pipeline_layout(&[], &[])? };

            let render_pass = render_pass_storage.raw(render_pass_handle).unwrap();

            let subpass = gfx::pass::Subpass {
                index: 0,
                main_pass: render_pass,
            };

            let mut desc =
                pso::GraphicsPipelineDesc::new(shaders, primitive, rasterizer, &layout, subpass);

            // TODO add attributes
            if let Some(handle) = create_info.vertex_attribs {
                if let Some(data) = vertex_attrib_storage.raw(handle) {
                    desc.attributes.extend_from_slice(&data[..]);
                }
            }

            desc.blender.targets.push(pso::ColorBlendDesc(
                pso::ColorMask::ALL,
                pso::BlendState::ALPHA,
            ));

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

        let (handle, _) = self.storage.insert(Pipeline::Graphics);

        self.graphic_pipelines
            .insert(handle.id(), GraphicsPipeline { pipeline });

        Ok(handle)
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

    pub fn release(self) {}
}
