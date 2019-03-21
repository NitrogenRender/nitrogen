/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;
use crate::types;

use gfx;

use crate::graph::{
    BufferReadType, BufferStorageType, BufferWriteType, ComputePassInfo, ExecutionContext,
    GraphWithNamesResolved, GraphicsPassInfo, ImageInfo, ImageReadType, ImageWriteType, PassInfo,
    ResourceCreateInfo, ResourceReadType, ResourceWriteType,
};

use crate::resources::{image, sampler};

use crate::device::DeviceContext;

use crate::resources::material::{MaterialError, MaterialHandle, MaterialInstanceHandle};
use smallvec::SmallVec;

use crate::graph::ResourceName;
use crate::resources::material::MaterialStorage;
use crate::resources::pipeline::PipelineError;
use crate::resources::render_pass::RenderPassError;
use gfx::device::Device;
use std::collections::BTreeMap;

/// Errors that can occur when trying to prepare resources for a graph execution.
#[allow(missing_docs)]
#[derive(Debug, Clone, From, Display)]
pub enum PrepareError {
    #[display(fmt = "Renderpass invalid. Is the graph compiled?")]
    InvalidRenderPass,

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
}

impl std::error::Error for PrepareError {}

/*
pub(crate) unsafe fn prepare(
    usages: &ResourceUsages,
    backbuffer: &mut Backbuffer,
    device: &DeviceContext,
    storages: &mut Storages,
    exec: &ExecutionGraph,
    resolved: &GraphWithNamesResolved,
    passes: &[(PassName, PassInfo)],
    context: ExecutionContext,
) -> Result<GraphResources, PrepareError> {
    let mut res = GraphResources {
        exec_context: context.clone(),
        external_resources: HashSet::new(),
        images: HashMap::new(),
        samplers: HashMap::new(),
        buffers: HashMap::new(),
    };

    for batch in &exec.pass_execution {
        // TODO this should probably be moved into the execution phase once a better
        // memory allocator is in place
        // Maybe instead things can be aliased if the size and format matches? :thinking:
        for create in &batch.resource_create {
            let res_info = &resolved.infos[create];
            create_resource(
                usages, device, &context, backbuffer, storages, *create, res_info, &mut res,
            );
        }

        for pass in &batch.passes {
            let info = &passes[pass.0].1;

            if let Some(mat) = base.pipelines_mat.get(pass) {
                let instance = storages.material.create_instance(device, *mat).unwrap();

                res.pass_mats.insert(*pass, instance);
            }

            prepare_pass(
                backbuffer, base, device, storages, resolved, *pass, info, &mut res,
            )?;
        }
    }

    Ok(res)
}


pub(crate) unsafe fn prepare_pass(
    backbuffer: &mut Backbuffer,
    base: &GraphBaseResources,
    device: &DeviceContext,
    storages: &mut Storages,
    resolved: &GraphWithNamesResolved,
    pass: PassId,
    info: &PassInfo,
    res: &mut GraphResources,
) -> Result<(), PrepareError> {
    match info {
        PassInfo::Graphics { .. } => {
            // create framebuffers
            let render_pass = base.render_passes[&pass];
            let render_pass_raw = storages
                .render_pass
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
                            ResourceCreateInfo::Image(ImageInfo::BackbufferRead(n)) => {
                                backbuffer.images.get(n).ok_or_else(|| {
                                    PrepareError::InvalidBackbufferResource(n.clone())
                                })?
                            }
                            ResourceCreateInfo::Image(ImageInfo::Create(_)) => res
                                .images
                                .get(&res_id)
                                .ok_or_else(|| PrepareError::InvalidImageResource(res_id))?,
                            // buffer attachments???
                            _ => unreachable!(),
                        };

                        let image = storages
                            .image
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

                                let image = storages
                                    .image
                                    .raw(handle)
                                    .ok_or_else(|| PrepareError::InvalidImageHandle(handle))?;

                                Ok((&image.view, &image.dimension))
                            }),
                    );

                // fold all the results into array, aborting on the first encountered error
                // then bubble up the error
                let view_dims: SmallVec<[_; 16]> = res_view_dims.try_fold(
                    SmallVec::new(),
                    |mut acc, val| -> Result<_, PrepareError> {
                        acc.push(val?);
                        Ok(acc)
                    },
                )?;

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

            res.framebuffers.insert(pass, (framebuffer, extent));

            Ok(())
        }
        PassInfo::Compute(_) => {
            // nothing to prepare for compute passes
            Ok(())
        }
    }
}

*/

unsafe fn create_render_pass(
    device: &DeviceContext,
    backbuffer_usage: &BackbufferUsage,
    storages: &mut Storages,
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
                let (origin, info) = resolved_graph.create_info(*res).unwrap();

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

    let render_pass = storages.render_pass.create(device, create_info)?;
    Ok(render_pass)
}

unsafe fn create_pipeline_compute(
    device: &DeviceContext,
    storages: &mut Storages,
    pass: PassId,
    pass_material: Option<MaterialHandle>,
    info: &ComputePassInfo,
) -> Result<PipelineHandle, PrepareError> {
    let layouts = create_pipeline_base(storages.material, pass_material, &info.materials[..]);

    let layouts = layouts
        .into_iter()
        .map(|(_, data)| data)
        .collect::<Vec<_>>();

    let mut push_constants = SmallVec::<[_; 1]>::new();
    if let Some(range) = &info.push_constants {
        push_constants.push(range.clone());
    }

    let create_info = crate::pipeline::ComputePipelineCreateInfo {
        shader: crate::pipeline::ShaderInfo {
            entry: &info.shader.entry,
            content: &info.shader.content,
        },
        descriptor_set_layout: &layouts[..],
        push_constants: push_constants.as_slice(),
    };

    let pipeline_handle = storages
        .pipeline
        .create_compute_pipeline(device, create_info)?;

    Ok(pipeline_handle)
}

unsafe fn create_pipeline_graphics(
    device: &DeviceContext,
    storages: &mut Storages,
    render_pass: RenderPassHandle,
    pass_material: Option<MaterialHandle>,
    info: &GraphicsPassInfo,
) -> Result<PipelineHandle, PrepareError> {
    use crate::pipeline;

    let layouts = create_pipeline_base(storages.material, pass_material, &info.materials[..]);

    let layouts = layouts
        .into_iter()
        .map(|(_, data)| data)
        .collect::<Vec<_>>();

    let create_info = pipeline::GraphicsPipelineCreateInfo {
        vertex_attribs: info.vertex_attrib,
        primitive: info.primitive,
        shader_vertex: pipeline::ShaderInfo {
            content: &info.shaders.vertex.content,
            entry: &info.shaders.vertex.entry,
        },
        shader_fragment: if info.shaders.fragment.is_some() {
            Some(pipeline::ShaderInfo {
                content: &info.shaders.fragment.as_ref().unwrap().content,
                entry: &info.shaders.fragment.as_ref().unwrap().entry,
            })
        } else {
            None
        },
        // TODO add support for geometry shaders
        shader_geometry: None,
        descriptor_set_layout: &layouts[..],
        push_constants: &info.push_constants[..],
        blend_modes: &info.blend_modes[..],
        depth_mode: info.depth_mode,
    };

    let pipeline_handle = storages.pipeline.create_graphics_pipeline(
        device,
        storages.render_pass,
        storages.vertex_attrib,
        render_pass,
        create_info,
    )?;

    Ok(pipeline_handle)
}

unsafe fn create_resource(
    usages: &ResourceUsages,
    device: &DeviceContext,
    context: &ExecutionContext,
    backbuffer: &mut Backbuffer,
    storages: &mut Storages,
    id: ResourceId,
    info: &ResourceCreateInfo,
    res: &mut GraphResources,
) -> Option<()> {
    match info {
        ResourceCreateInfo::Image(ImageInfo::BackbufferRead { name, .. }) => {
            // read image from backbuffer

            let img = backbuffer.images.get(name)?;
            res.images.insert(id, *img);
            res.external_resources.insert(id);

            if let Some(sampler) = backbuffer.samplers.get(name) {
                res.samplers.insert(id, *sampler);
            }

            Some(())
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

            let img_handle = storages.image.create(device, create_info).ok()?;

            res.images.insert(id, img_handle);

            // If the image is used for sampling then it means some other pass will read from it
            // as a color image. In that case we create a sampler for this image as well
            if usages.0.contains(gfx::image::Usage::SAMPLED) {
                let sampler = storages.sampler.create(
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
                res.samplers.insert(id, sampler);
            }

            Some(())
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

                    storages
                        .buffer
                        .device_local_create(device, create_info)
                        .ok()?
                }
                BufferStorageType::HostVisible => {
                    let create_info = crate::buffer::CpuVisibleCreateInfo {
                        size: buf.size,
                        is_transient: false,
                        usage,
                    };

                    storages
                        .buffer
                        .cpu_visible_create(device, create_info)
                        .ok()?
                }
            };

            res.buffers.insert(id, buffer);

            Some(())
        }
        ResourceCreateInfo::Virtual => {
            // External resources don't really "exist", they are just markers, so nothing to do here
            Some(())
        }
    }
}

unsafe fn create_pipeline_base<'a>(
    material_storage: &'a mut MaterialStorage,
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

/// Create the material (-instance) for a pass.
pub(crate) unsafe fn create_pass_material(
    device: &DeviceContext,
    material_storage: &mut MaterialStorage,
    graph: &GraphWithNamesResolved,
    pass: PassId,
) -> Result<Option<MaterialInstanceHandle>, PrepareError> {
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

    let instance = if let Some(mat) = mat {
        Some(material_storage.create_instance(device, mat)?)
    } else {
        None
    };

    Ok(instance)
}
