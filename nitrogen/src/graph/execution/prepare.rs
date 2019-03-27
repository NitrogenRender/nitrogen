/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;
use crate::types;

use gfx;

use crate::graph::{
    BufferReadType, BufferStorageType, BufferWriteType, ExecutionContext, Graph,
    GraphWithNamesResolved, ImageInfo, ImageReadType, ImageWriteType, ResourceCreateInfo,
    ResourceReadType, ResourceWriteType,
};

use crate::resources::{image, sampler};

use crate::device::DeviceContext;

use crate::resources::material::{MaterialError, MaterialHandle};
use smallvec::SmallVec;

use crate::graph::builder::PassType;
use crate::graph::compilation::CompiledGraph;
use crate::graph::ResourceName;
use crate::resources::buffer::BufferError;
use crate::resources::image::ImageError;
use crate::resources::material::MaterialStorage;
use crate::resources::pipeline::PipelineError;
use crate::resources::render_pass::RenderPassError;
use std::collections::BTreeMap;

/// Errors that can occur when trying to prepare resources for a graph execution.
#[allow(missing_docs)]
#[derive(Debug, Clone, From, Display)]
pub enum PrepareError {
    #[display(fmt = "Renderpass invalid. Is the graph compiled?")]
    InvalidRenderPass,

    #[display(fmt = "Famebuffer invalid. Is the graph compiled?")]
    InvalidFramebuffer,

    #[display(fmt = "Pipeline could not be created because a mandatory shader handle is invalid")]
    InvalidShaderHandle,

    #[display(fmt = "Resource {:?} is referenced which is invalid", _0)]
    InvalidResource(ResourceId),

    #[display(fmt = "Backbuffer resource \"{}\" does not exist", _0)]
    InvalidBackbufferResource(ResourceName),

    #[display(fmt = "Image {:?} was not created yet. Bug?", _0)]
    InvalidImageResource(ResourceId),

    #[display(fmt = "Image {:?} is invalid", _0)]
    InvalidImageHandle(ImageHandle),

    #[display(fmt = "The framebuffer extent could not be inferred")]
    CantInferFramebufferExtent,

    #[display(fmt = "Out of memory: {}", _0)]
    OutOfMemory(gfx::device::OutOfMemory),

    #[display(fmt = "Error creating renderpass: {}", _0)]
    RenderPassError(RenderPassError),

    #[display(fmt = "Error creating a pipeline: {}", _0)]
    PipelineError(PipelineError),

    #[display(fmt = "Error creating a material or material instance: {}", _0)]
    MaterialError(MaterialError),

    #[display(fmt = "Error creating an image: {}", _0)]
    ImageError(ImageError),

    #[display(fmt = "Error creating a buffer: {}", _0)]
    BufferError(BufferError),
}

impl std::error::Error for PrepareError {}

pub(crate) unsafe fn prepare_graphics_pass_base(
    device: &DeviceContext,
    storages: &Storages,
    pass_res: &mut PassResources,
    pass: PassId,
    compiled: &CompiledGraph,
) -> Result<(), PrepareError> {
    let render_pass = create_render_pass(device, storages, &compiled.graph_resources, pass)?;

    pass_res.render_passes.insert(pass, render_pass);

    Ok(())
}

pub(crate) struct ResourcePrepareOptions {
    pub(crate) create_non_contextual: bool,
    pub(crate) create_contextual: bool,
    pub(crate) create_pass_mat: bool,
}

// this attribute is here because clippy keeps complaining, but there is no good way
// to reduce the number of arguments here..
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn prepare_resources(
    device: &DeviceContext,
    storages: &Storages,
    res_list: &mut ResourceList,
    graph: &Graph,
    res: &mut GraphResources,
    backbuffer: &mut Backbuffer,
    options: ResourcePrepareOptions,
    context: &ExecutionContext,
) -> Result<(), PrepareError> {
    let resolved = &graph.compiled_graph.graph_resources;
    let exec = &graph.exec_graph;
    let compiled = &graph.compiled_graph;
    let usages = &graph.res_usage;

    let pass_res = &graph.pass_resources;

    for batch in &exec.pass_execution {
        for res_id in &batch.resource_create {
            let info = &resolved.infos[res_id];

            let is_contextual = compiled.contextual_resources.contains(res_id);

            let create = (is_contextual && options.create_contextual)
                || (!is_contextual && options.create_non_contextual);

            if create {
                create_resource(
                    device, storages, res_list, usages, res, backbuffer, *res_id, info, context,
                )?;
            }
        }

        if options.create_pass_mat {
            for pass in &batch.passes {
                if let Some(mat) = pass_res.pass_material.get(pass) {
                    let instance = storages
                        .material
                        .borrow_mut()
                        .create_instance(device, *mat)?;

                    res.pass_mat_instances.insert(*pass, instance);
                }
            }
        }
    }

    Ok(())
}

pub(crate) struct GraphicsPassPrepareOptions {
    pub(crate) create_non_contextual: bool,
    pub(crate) create_contextual: bool,
}

pub(crate) unsafe fn prepare_graphics_passes(
    device: &DeviceContext,
    storages: &Storages,
    res_list: &mut ResourceList,
    res: &mut GraphResources,
    backbuffer: &Backbuffer,
    graph: &Graph,
    options: GraphicsPassPrepareOptions,
) -> Result<(), PrepareError> {
    let compiled = &graph.compiled_graph;
    let resolved = &compiled.graph_resources;
    let exec = &graph.exec_graph;

    for batch in &exec.pass_execution {
        for pass in &batch.passes {
            let ty = resolved.pass_types[pass];

            if ty != PassType::Graphics {
                continue;
            }

            let is_contextual = compiled.contextual_passes.contains(pass);
            let _renders_to_backbuffer = compiled
                .passes_that_render_to_the_backbuffer
                .contains_key(pass);

            let create = (is_contextual && options.create_contextual)
                || (!is_contextual && options.create_non_contextual);

            if create {
                prepare_graphics_pass(device, storages, res_list, res, backbuffer, graph, *pass)?;
            }
        }
    }

    Ok(())
}

pub(crate) unsafe fn prepare_graphics_pass(
    device: &DeviceContext,
    storages: &Storages,
    res_list: &mut ResourceList,
    res: &mut GraphResources,
    backbuffer: &Backbuffer,
    graph: &Graph,
    pass: PassId,
) -> Result<(), PrepareError> {
    let pass_res = &graph.pass_resources;

    let render_pass = *pass_res
        .render_passes
        .get(&pass)
        .ok_or(PrepareError::InvalidRenderPass)?;

    let framebuffer_res = create_framebuffer(
        device,
        storages,
        &graph.compiled_graph.graph_resources,
        backbuffer,
        res,
        render_pass,
        pass,
    )?;

    let old = res.framebuffers.insert(pass, framebuffer_res);

    if let Some((fb, _)) = old {
        res_list.queue_framebuffer(fb);
    }

    Ok(())
}

unsafe fn create_render_pass(
    device: &DeviceContext,
    storages: &Storages,
    resolved_graph: &GraphWithNamesResolved,
    pass: PassId,
) -> Result<RenderPassHandle, PrepareError> {
    // create a render pass handle for use in a graphics pass
    //
    // A render pass contains a list of "attachments" which are generally used for writing
    // color values.
    //
    // A special kind of attachment is a depth-stencil attachment. Since depth- and stencil tests
    // are deeply buried in the graphics pipeline, they are also considered an attachment.
    //
    // In order to have a "reading depth test" (just checking but not writing own depth value)
    // a depth attachment has to be present.
    //
    // So in order to create a render pass we use all color images that we write to,
    // check if there is a depth attachment that is written to and use those as attachments.
    // If there is a **reading** depth attachment, we add it as well.

    let mut has_depth_write = false;
    let mut has_depth_read = false;

    let mut attachments = {
        resolved_graph.pass_writes[&pass]
            .iter()
            // we are only interested in images that are written to as color or depth
            .filter(|(_, ty, _)| match ty {
                ResourceWriteType::Image(ImageWriteType::Color) => true,
                ResourceWriteType::Image(ImageWriteType::DepthStencil) => {
                    has_depth_write = true;
                    true
                }
                _ => false,
            })
            .filter_map(|(res, _ty, binding)| {
                let (_origin, info) = resolved_graph.create_info(*res)?;

                let format = match info {
                    ResourceCreateInfo::Image(ImageInfo::Create(img)) => img.format.into(),
                    ResourceCreateInfo::Image(ImageInfo::BackbufferRead { format, .. }) => *format,
                    _ => unreachable!(),
                };

                let load_op = gfx::pass::AttachmentLoadOp::Load;

                let initial_layout = gfx::image::Layout::General;

                let (ops, stencil) = {
                    // applies to color AND depth
                    let ops = gfx::pass::AttachmentOps {
                        load: load_op,
                        store: gfx::pass::AttachmentStoreOp::Store,
                    };

                    let stencil = gfx::pass::AttachmentOps::DONT_CARE;

                    (ops, stencil)
                };

                Some((
                    *binding,
                    gfx::pass::Attachment {
                        format: Some(format),
                        samples: 1,
                        ops,
                        stencil_ops: stencil,
                        // TODO Better layout transitions
                        layouts: initial_layout..gfx::image::Layout::General,
                    },
                ))
            })
            // we might be "reading" from depth, but we still have to mention it as an attachment
            .chain(
                resolved_graph.pass_reads[&pass]
                    .iter()
                    .filter(|(_, ty, _, _)| match ty {
                        ResourceReadType::Image(ImageReadType::DepthStencil) => true,
                        _ => false,
                    })
                    .map(|(res, _, _, _)| {
                        has_depth_read = true;

                        let (_origin, info) = resolved_graph.create_info(*res).unwrap();

                        let format = match info {
                            ResourceCreateInfo::Image(ImageInfo::Create(img)) => img.format.into(),
                            ResourceCreateInfo::Image(ImageInfo::BackbufferRead {
                                format, ..
                            }) => *format,
                            _ => unreachable!(),
                        };

                        (
                            u8::max_value(),
                            gfx::pass::Attachment {
                                format: Some(format),
                                samples: 1,
                                ops: gfx::pass::AttachmentOps {
                                    load: gfx::pass::AttachmentLoadOp::Load,
                                    store: gfx::pass::AttachmentStoreOp::DontCare,
                                },
                                stencil_ops: gfx::pass::AttachmentOps::DONT_CARE,
                                layouts: gfx::image::Layout::General..gfx::image::Layout::General,
                            },
                        )
                    }),
            )
            .collect::<SmallVec<[_; 16]>>()
    };

    let has_depth = has_depth_write || has_depth_read;

    attachments
        .as_mut_slice()
        .sort_by_key(|(binding, _)| *binding);

    let depth_binding = if has_depth {
        // if depth is the only binding then we don't want to underflow :)
        attachments.len().max(1) - 1
    } else {
        attachments.len()
    };

    if has_depth {
        attachments[depth_binding].0 = depth_binding as _;
    }

    let mut attachments_desc = attachments
        .as_slice()
        .iter()
        .enumerate()
        .map(|(i, _)| (i, gfx::image::Layout::ColorAttachmentOptimal))
        .collect::<SmallVec<[_; 16]>>();

    if has_depth {
        attachments_desc[depth_binding].1 = gfx::image::Layout::DepthStencilAttachmentOptimal;
    }

    let color_desc = &attachments_desc[0..depth_binding];

    let depth_stencil_desc = if has_depth {
        Some(&attachments_desc[depth_binding])
    } else {
        None
    };

    let subpass = gfx::pass::SubpassDesc {
        colors: color_desc,
        depth_stencil: depth_stencil_desc,
        inputs: &[],
        resolves: &[],
        preserves: &[],
    };

    let dependencies = gfx::pass::SubpassDependency {
        passes: gfx::pass::SubpassRef::External..gfx::pass::SubpassRef::Pass(0),
        stages: gfx::pso::PipelineStage::COLOR_ATTACHMENT_OUTPUT
            ..gfx::pso::PipelineStage::COLOR_ATTACHMENT_OUTPUT,
        accesses: gfx::image::Access::empty()
            ..(gfx::image::Access::COLOR_ATTACHMENT_READ
                | gfx::image::Access::COLOR_ATTACHMENT_WRITE),
    };

    use crate::render_pass::RenderPassCreateInfo;

    let attachments = attachments
        .into_iter()
        .map(|(_, data)| data)
        .collect::<SmallVec<[_; 16]>>();

    let create_info = RenderPassCreateInfo {
        attachments: attachments.as_slice(),
        subpasses: &[subpass],
        dependencies: &[dependencies],
    };

    let render_pass = storages
        .render_pass
        .borrow_mut()
        .create(device, create_info)?;
    Ok(render_pass)
}

unsafe fn create_framebuffer(
    device: &DeviceContext,
    storages: &Storages,
    resolved: &GraphWithNamesResolved,
    backbuffer: &Backbuffer,
    res: &GraphResources,
    render_pass: RenderPassHandle,
    pass: PassId,
) -> Result<(crate::types::Framebuffer, gfx::image::Extent), PrepareError> {
    let image_storage = storages.image.borrow();
    let render_pass_storage = storages.render_pass.borrow();

    let render_pass_raw = render_pass_storage
        .raw(render_pass)
        .ok_or(PrepareError::InvalidRenderPass)?;

    // get all image views and dimensions for framebuffer creation
    let (views, dims): (SmallVec<[_; 16]>, SmallVec<[_; 16]>) = {
        // we only care about images that are used as a color or depth-stencil attachment
        let mut sorted_attachments = resolved.pass_writes[&pass]
            .iter()
            .filter(|(_, ty, _)| match ty {
                ResourceWriteType::Image(ImageWriteType::Color) => true,
                ResourceWriteType::Image(ImageWriteType::DepthStencil) => true,
                _ => false,
            })
            .collect::<SmallVec<[_; 16]>>();

        // Sort them by binding
        sorted_attachments
            .as_mut_slice()
            .sort_by_key(|(_, _, binding)| binding);

        // resolve all the references, preserve error
        let mut res_view_dims = sorted_attachments
            .into_iter()
            .map(|(res_id, _, _)| -> Result<_, PrepareError> {
                let res_id = resolved
                    .moved_from(*res_id)
                    .ok_or_else(|| PrepareError::InvalidResource(*res_id))?;
                let (_, create_info) = resolved
                    .create_info(res_id)
                    .ok_or_else(|| PrepareError::InvalidResource(res_id))?;

                let handle = match create_info {
                    ResourceCreateInfo::Image(ImageInfo::BackbufferRead { name, .. }) => backbuffer
                        .images
                        .get(name)
                        .ok_or_else(|| PrepareError::InvalidBackbufferResource(name.clone()))?,
                    ResourceCreateInfo::Image(ImageInfo::Create(_)) => res
                        .images
                        .get(&res_id)
                        .ok_or_else(|| PrepareError::InvalidImageResource(res_id))?,
                    // buffer attachments???
                    _ => unreachable!(),
                };

                let image = image_storage
                    .raw(*handle)
                    .ok_or_else(|| PrepareError::InvalidImageHandle(*handle))?;

                Ok((&image.view, &image.dimension))
            })
            // depth textures might be "read" from when using for testing without writing
            .chain(
                resolved.pass_reads[&pass]
                    .iter()
                    .filter(|(_, ty, _, _)| match ty {
                        ResourceReadType::Image(ImageReadType::DepthStencil) => true,
                        _ => false,
                    })
                    .map(|(res_id, _, _, _)| -> Result<_, PrepareError> {
                        let res_id = resolved
                            .moved_from(*res_id)
                            .ok_or_else(|| PrepareError::InvalidResource(*res_id))?;

                        let handle = res.images[&res_id];

                        let image = image_storage
                            .raw(handle)
                            .ok_or_else(|| PrepareError::InvalidImageHandle(handle))?;

                        Ok((&image.view, &image.dimension))
                    }),
            );

        // fold all the results into array, aborting on the first encountered error
        // then bubble up the error
        let view_dims: SmallVec<[_; 16]> =
            res_view_dims.try_fold(SmallVec::new(), |mut acc, val| -> Result<_, PrepareError> {
                acc.push(val?);
                Ok(acc)
            })?;

        // split into views and dimensions
        view_dims.into_iter().unzip()
    };

    // find "THE" extent of the framebuffer
    // TODO check that all dimensions are the same using `all()`?
    let extent = {
        dims.as_slice()
            .iter()
            .map(|img_dim| img_dim.as_triple(1))
            .map(|(x, y, z)| gfx::image::Extent {
                width: x,
                height: y,
                depth: z,
            })
            .next()
            .ok_or(PrepareError::CantInferFramebufferExtent)?
    };

    use gfx::Device;

    // wheeeey
    let framebuffer = device
        .device
        .create_framebuffer(render_pass_raw, views, extent)?;

    Ok((framebuffer, extent))
}

pub(crate) unsafe fn create_pipeline_compute(
    device: &DeviceContext,
    storages: &Storages,
    _pass: PassId,
    pass_material: Option<MaterialHandle>,
    info: &ComputePipelineInfo,
) -> Result<PipelineHandle, PrepareError> {
    let material_storage = storages.material.borrow();
    let shader_storage = storages.shader.borrow();
    let mut pipeline_storage = storages.pipeline.borrow_mut();

    let layouts = create_pipeline_base(&*material_storage, pass_material, &info.materials[..]);

    let layouts = layouts
        .into_iter()
        .map(|(_, data)| data)
        .collect::<Vec<_>>();

    let mut push_constants = SmallVec::<[_; 1]>::new();
    if let Some(range) = &info.push_constant_range {
        push_constants.push((range.start / 4)..(range.end / 4));
    }

    let shader = shader_storage
        .raw_compute(info.shader.handle)
        .ok_or(PrepareError::InvalidShaderHandle)?;

    let create_info = crate::pipeline::ComputePipelineCreateInfo {
        shader: crate::pipeline::ShaderInfo {
            content: shader.spirv_content.as_slice(),
            entry: &shader.entry_point,
            specialization: &info.shader.specialization,
        },
        descriptor_set_layout: &layouts[..],
        push_constants: push_constants.as_slice(),
    };

    let pipeline_handle = pipeline_storage.create_compute_pipeline(device, create_info)?;

    Ok(pipeline_handle)
}

pub(crate) unsafe fn create_pipeline_graphics(
    device: &DeviceContext,
    storages: &Storages,
    _pass: PassId,
    pass_material: Option<MaterialHandle>,
    info: &GraphicsPipelineInfo,
    render_pass: RenderPassHandle,
) -> Result<PipelineHandle, PrepareError> {
    use crate::pipeline;

    let material_storage = storages.material.borrow();
    let shader_storage = storages.shader.borrow();
    let mut pipeline_storage = storages.pipeline.borrow_mut();

    let layouts = create_pipeline_base(&*material_storage, pass_material, &info.materials[..]);

    let layouts = layouts
        .into_iter()
        .map(|(_, data)| data)
        .collect::<Vec<_>>();

    let mut push_constants = SmallVec::<[_; 1]>::new();
    if let Some(range) = &info.push_constants {
        push_constants.push((range.start / 4)..(range.end / 4));
    }

    let vertex_shader = {
        let raw = shader_storage
            .raw_vertex(info.shaders.vertex.handle)
            .ok_or(PrepareError::InvalidShaderHandle)?;

        pipeline::ShaderInfo {
            content: raw.spirv_content.as_slice(),
            entry: &raw.entry_point,
            specialization: &info.shaders.vertex.specialization,
        }
    };

    let fragment_shader = if let Some(sh) = &info.shaders.fragment {
        let raw = shader_storage
            .raw_fragment(sh.handle)
            .ok_or(PrepareError::InvalidShaderHandle)?;

        let info = pipeline::ShaderInfo {
            content: raw.spirv_content.as_slice(),
            entry: &raw.entry_point,
            specialization: &sh.specialization,
        };
        Some(info)
    } else {
        None
    };

    let geometry_shader = if let Some(sh) = &info.shaders.geometry {
        let raw = shader_storage
            .raw_geometry(sh.handle)
            .ok_or(PrepareError::InvalidShaderHandle)?;

        let info = pipeline::ShaderInfo {
            content: raw.spirv_content.as_slice(),
            entry: &raw.entry_point,
            specialization: &sh.specialization,
        };
        Some(info)
    } else {
        None
    };

    let create_info = pipeline::GraphicsPipelineCreateInfo {
        vertex_attribs: info.vertex_attrib,
        primitive: info.primitive,
        shader_vertex: vertex_shader,
        shader_fragment: fragment_shader,
        shader_geometry: geometry_shader,
        descriptor_set_layout: &layouts[..],
        push_constants: push_constants.as_slice(),
        blend_modes: &info.blend_modes[..],
        depth_mode: info.depth_mode,
    };

    let pipeline_handle = pipeline_storage.create_graphics_pipeline(
        device,
        &*storages.render_pass.borrow(),
        &*storages.vertex_attrib.borrow(),
        render_pass,
        create_info,
    )?;

    Ok(pipeline_handle)
}

// this attribute is here because clippy keeps complaining, but there is no good way
// to reduce the number of arguments here..
#[allow(clippy::too_many_arguments)]
unsafe fn create_resource(
    device: &DeviceContext,
    storages: &Storages,
    res_list: &mut ResourceList,
    usages: &ResourceUsages,
    res: &mut GraphResources,
    backbuffer: &mut Backbuffer,
    id: ResourceId,
    info: &ResourceCreateInfo,
    context: &ExecutionContext,
) -> Result<(), PrepareError> {
    let mut image_storage = storages.image.borrow_mut();
    let mut sampler_storage = storages.sampler.borrow_mut();
    let mut buffer_storage = storages.buffer.borrow_mut();

    match info {
        ResourceCreateInfo::Image(ImageInfo::BackbufferRead { name, .. }) => {
            // read image from backbuffer

            // NOTE: do **not** destroy previous resources when a backbuffer resource overrides it.
            // This is because a backbuffer resource can only override another backbuffer resource.
            // (At least it should.)

            let img = backbuffer
                .images
                .get(name)
                .ok_or_else(|| PrepareError::InvalidBackbufferResource(name.clone()))?;
            res.external_resources.insert(id);

            res.images.insert(id, *img);

            if let Some(sampler) = backbuffer.samplers.get(name) {
                let _old_sampler = res.samplers.insert(id, *sampler);
            }

            Ok(())
        }

        ResourceCreateInfo::Image(ImageInfo::Create(img)) => {
            // find out the size and kind of the image

            let raw_dim = img.size_mode.absolute(context.reference_size);
            let dim = image::ImageDimension::D2 {
                x: raw_dim.0,
                y: raw_dim.1,
            };

            let kind = image::ViewKind::D2;

            let format = img.format;

            let is_depth_stencil = format.is_depth_stencil();

            let num_mips = if is_depth_stencil { 0 } else { 1 };

            // any flags that will be needed
            let usages = &usages.image[&id];

            // let's go!
            let create_info = image::ImageCreateInfo {
                dimension: dim,
                num_layers: 1,
                num_samples: 1,
                num_mipmaps: num_mips,
                format,
                swizzle: image::Swizzle::NO,
                kind,
                usage: usages.0,
                is_transient: false,
            };

            let img_handle = image_storage.create(device, create_info)?;

            let old_image = res.images.insert(id, img_handle);

            // If the image is used for sampling then it means some other pass will read from it
            // as a color image. In that case we create a sampler for this image as well
            let old_sampler = if usages.0.contains(gfx::image::Usage::SAMPLED) {
                let sampler = sampler_storage.create(
                    device,
                    sampler::SamplerCreateInfo {
                        min_filter: sampler::Filter::Linear,
                        mip_filter: sampler::Filter::Linear,
                        mag_filter: sampler::Filter::Linear,
                        wrap_mode: (
                            sampler::WrapMode::Clamp,
                            sampler::WrapMode::Clamp,
                            sampler::WrapMode::Clamp,
                        ),
                    },
                );
                res.samplers.insert(id, sampler)
            } else {
                None
            };

            if let Some(old_img) = old_image {
                image_storage.destroy(res_list, &[old_img]);
            }
            if let Some(old_samp) = old_sampler {
                sampler_storage.destroy(res_list, &[old_samp]);
            }

            Ok(())
        }
        ResourceCreateInfo::Buffer(buf) => {
            let usage = usages.buffer[&id];

            let buffer = match buf.storage {
                BufferStorageType::DeviceLocal => {
                    let create_info = crate::buffer::DeviceLocalCreateInfo {
                        size: buf.size,
                        is_transient: false,
                        usage,
                    };

                    buffer_storage.device_local_create(device, create_info)?
                }
                BufferStorageType::HostVisible => {
                    let create_info = crate::buffer::CpuVisibleCreateInfo {
                        size: buf.size,
                        is_transient: false,
                        usage,
                    };

                    buffer_storage.cpu_visible_create(device, create_info)?
                }
            };

            let old_buf = res.buffers.insert(id, buffer);

            if let Some(old_buf) = old_buf {
                buffer_storage.destroy(res_list, &[old_buf]);
            }

            Ok(())
        }
        ResourceCreateInfo::Virtual => {
            // External resources don't really "exist", they are just markers, so nothing to do here
            Ok(())
        }
    }
}

unsafe fn create_pipeline_base<'a>(
    material_storage: &'a MaterialStorage,
    pass_material: Option<MaterialHandle>,
    materials: &[(usize, MaterialHandle)],
) -> BTreeMap<usize, &'a types::DescriptorSetLayout> {
    let mut sets = BTreeMap::new();

    // base material
    if let Some(pass_material) = pass_material {
        if let Some(mat) = material_storage.raw(pass_material) {
            sets.insert(0, &mat.desc_set_layout);
        }
    }

    // other materials
    for (set, material) in materials {
        let mat = match material_storage.raw(*material) {
            Some(mat) => mat,
            None => continue,
        };

        let layout = &mat.desc_set_layout;

        sets.insert(*set, layout);
    }

    sets
}

/// Create the material for a pass.
pub(crate) unsafe fn create_pass_material(
    device: &DeviceContext,
    material_storage: &mut MaterialStorage,
    graph: &GraphWithNamesResolved,
    pass: PassId,
) -> Result<Option<MaterialHandle>, PrepareError> {
    use gfx::Device;

    let (core_desc, core_range) = {
        let reads = graph.pass_reads[&pass]
            .iter()
            .filter(|(_id, _ty, _, _)| match _ty {
                ResourceReadType::Virtual => false,
                ResourceReadType::Image(ImageReadType::DepthStencil) => false,
                ResourceReadType::Image(_) => true,
                ResourceReadType::Buffer(_) => true,
            });

        let samplers = reads.clone().filter(|(_, _, _, sampler)| sampler.is_some());

        let sampler_descriptors =
            samplers
                .clone()
                .map(|(_, _, _, binding)| gfx::pso::DescriptorSetLayoutBinding {
                    binding: u32::from(binding.unwrap()),
                    ty: gfx::pso::DescriptorType::Sampler,
                    count: 1,
                    stage_flags: gfx::pso::ShaderStageFlags::ALL,
                    immutable_samplers: false,
                });

        // writing to resources that are not color or depth images happens via descriptors as well
        let writes = graph.pass_writes[&pass]
            .iter()
            .filter(|(_res, ty, _binding)| {
                match ty {
                    ResourceWriteType::Buffer(buf) => {
                        match buf {
                            BufferWriteType::Storage => true,
                            // TODO storage texel
                            _ => false,
                        }
                    }
                    ResourceWriteType::Image(img) => match img {
                        ImageWriteType::Storage => true,
                        _ => false,
                    },
                }
            });

        let write_descriptors =
            writes
                .clone()
                .map(|(_res, ty, binding)| gfx::pso::DescriptorSetLayoutBinding {
                    binding: u32::from(*binding),
                    ty: match ty {
                        ResourceWriteType::Image(img) => match img {
                            ImageWriteType::Storage => gfx::pso::DescriptorType::StorageImage,
                            _ => unreachable!(),
                        },
                        ResourceWriteType::Buffer(buf) => match buf {
                            BufferWriteType::Storage => gfx::pso::DescriptorType::StorageBuffer,
                            _ => unimplemented!(),
                        },
                    },
                    count: 1,
                    stage_flags: gfx::pso::ShaderStageFlags::ALL,
                    immutable_samplers: false,
                });

        let descriptors = reads
            .clone()
            .map(|(_res, ty, binding, _)| {
                gfx::pso::DescriptorSetLayoutBinding {
                    binding: u32::from(*binding),
                    ty: match ty {
                        ResourceReadType::Image(img) => match img {
                            ImageReadType::Color => gfx::pso::DescriptorType::SampledImage,
                            ImageReadType::Storage => gfx::pso::DescriptorType::StorageImage,
                            ImageReadType::DepthStencil => unreachable!(),
                        },
                        ResourceReadType::Buffer(buf) => {
                            match buf {
                                BufferReadType::Uniform => gfx::pso::DescriptorType::UniformBuffer,
                                BufferReadType::UniformTexel => {
                                    // TODO test this
                                    // does this need samplers? I think so. Let's find out!
                                    gfx::pso::DescriptorType::UniformTexelBuffer
                                }
                                BufferReadType::Storage => gfx::pso::DescriptorType::StorageBuffer,
                                BufferReadType::StorageTexel => {
                                    //  TODO test this
                                    // does this need samplers? I think so. Let's find out!
                                    gfx::pso::DescriptorType::StorageTexelBuffer
                                }
                            }
                        }
                        ResourceReadType::Virtual => unreachable!(),
                    },
                    count: 1,
                    stage_flags: gfx::pso::ShaderStageFlags::ALL,
                    immutable_samplers: false,
                }
            })
            .chain(sampler_descriptors)
            .chain(write_descriptors);

        let range = reads
            .map(|(_, ty, _, _)| {
                gfx::pso::DescriptorRangeDesc {
                    ty: match ty {
                        ResourceReadType::Image(img) => match img {
                            ImageReadType::Color => gfx::pso::DescriptorType::SampledImage,
                            ImageReadType::Storage => gfx::pso::DescriptorType::StorageImage,
                            ImageReadType::DepthStencil => unreachable!(),
                        },
                        ResourceReadType::Buffer(buf) => {
                            match buf {
                                BufferReadType::Uniform => gfx::pso::DescriptorType::UniformBuffer,
                                BufferReadType::UniformTexel => {
                                    // TODO test this
                                    // does this need samplers? I think so. Let's find out!
                                    gfx::pso::DescriptorType::UniformTexelBuffer
                                }
                                BufferReadType::Storage => gfx::pso::DescriptorType::StorageBuffer,
                                BufferReadType::StorageTexel => {
                                    //  TODO test this
                                    // does this need samplers? I think so. Let's find out!
                                    gfx::pso::DescriptorType::StorageTexelBuffer
                                }
                            }
                        }
                        ResourceReadType::Virtual => unreachable!(),
                    },
                    count: 1,
                }
            })
            .chain(samplers.map(|_| gfx::pso::DescriptorRangeDesc {
                ty: gfx::pso::DescriptorType::Sampler,
                count: 1,
            }))
            .chain(writes.map(|(_, ty, _)| gfx::pso::DescriptorRangeDesc {
                ty: match ty {
                    ResourceWriteType::Image(img) => match img {
                        ImageWriteType::Storage => gfx::pso::DescriptorType::StorageImage,
                        _ => unreachable!(),
                    },
                    ResourceWriteType::Buffer(buf) => match buf {
                        BufferWriteType::Storage => gfx::pso::DescriptorType::StorageBuffer,
                        _ => unimplemented!(),
                    },
                },
                count: 1,
            }));

        (descriptors, range)
    };

    let pass_set_layout = device.device.create_descriptor_set_layout(core_desc, &[])?;

    let mat = material_storage.create_raw(device, pass_set_layout, core_range, 16);

    Ok(mat)
}
