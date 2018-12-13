/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;
use crate::types;

use gfx;

use crate::graph::{
    BufferReadType, BufferWriteType, ComputePassInfo, ExecutionContext, GraphResourcesResolved,
    GraphicsPassInfo, ImageReadType, ImageWriteType, PassInfo, ResourceCreateInfo,
    ResourceReadType, ResourceWriteType,
};

use crate::resources::{image, sampler};

use crate::device::DeviceContext;

use crate::resources::material::MaterialHandle;
use smallvec::SmallVec;

use crate::resources::material::MaterialStorage;
use std::collections::BTreeMap;

pub(crate) fn prepare_base(
    device: &DeviceContext,
    storages: &mut Storages,
    exec: &ExecutionGraph,
    resolved: &GraphResourcesResolved,
    passes: &[(PassName, PassInfo)],
) -> GraphBaseResources {
    let mut res = GraphBaseResources::default();

    for batch in &exec.pass_execution {
        for pass in &batch.passes {
            let info = &passes[pass.0].1;

            prepare_pass_base(device, storages, resolved, *pass, info, &mut res);
        }
    }

    res
}

pub(crate) fn prepare_pass_base(
    device: &DeviceContext,
    storages: &mut Storages,
    resolved: &GraphResourcesResolved,
    pass: PassId,
    info: &PassInfo,
    res: &mut GraphBaseResources,
) -> Option<()> {
    // TODO compute
    match info {
        PassInfo::Graphics(info) => {
            // Create render pass
            let render_pass = create_render_pass(device, storages, resolved, pass)?;

            // insert into resources
            res.render_passes.insert(pass, render_pass);

            // create pipeline object
            let (pipe, layout, pool, set) =
                create_pipeline_graphics(device, storages, resolved, render_pass, pass, info)?;

            res.pipelines_graphic.insert(pass, pipe);
            res.pipelines_desc_set.insert(pass, (layout, pool, set));

            Some(())
        }
        PassInfo::Compute(info) => {
            let (pipeline, layout, pool, set) =
                create_pipeline_compute(device, storages, resolved, pass, info)?;

            res.pipelines_compute.insert(pass, pipeline);
            res.pipelines_desc_set.insert(pass, (layout, pool, set));

            Some(())
        }
    }
}

pub(crate) fn prepare(
    usages: &ResourceUsages,
    base: &GraphBaseResources,
    device: &DeviceContext,
    storages: &mut Storages,
    exec: &ExecutionGraph,
    resolved: &GraphResourcesResolved,
    passes: &[(PassName, PassInfo)],
    context: &ExecutionContext,
) -> GraphResources {
    let mut res = GraphResources::default();

    for batch in &exec.pass_execution {
        // TODO this should probably be moved into the execution phase once a better
        // memory allocator is in place
        // Maybe instead things can be aliased if the size and format matches? :thinking:
        for create in &batch.resource_create {
            let res_info = &resolved.infos[create];
            create_resource(
                usages, device, context, storages, *create, res_info, &mut res,
            );
        }

        for _copy in &batch.resource_copies {
            unimplemented!();
        }

        for pass in &batch.passes {
            let info = &passes[pass.0].1;

            prepare_pass(base, device, storages, resolved, *pass, info, &mut res);
        }
    }

    res
}

pub(crate) fn prepare_pass(
    base: &GraphBaseResources,
    device: &DeviceContext,
    storages: &mut Storages,
    resolved: &GraphResourcesResolved,
    pass: PassId,
    info: &PassInfo,
    res: &mut GraphResources,
) -> Option<()> {
    // TODO compute
    match info {
        PassInfo::Graphics { .. } => {
            // create framebuffers
            let render_pass = base.render_passes[&pass];
            let render_pass_raw = storages.render_pass.raw(render_pass)?;

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

                sorted_attachments
                    .into_iter()
                    .filter_map(|(res_id, _, _)| {
                        let res_id = resolved.moved_from(*res_id)?;

                        let handle = res.images[&res_id];

                        let image = storages.image.raw(handle)?;

                        Some((&image.view, &image.dimension))
                    })
                    .unzip()
            };

            let extent = {
                dims.as_slice()
                    .iter()
                    .map(|img_dim| img_dim.as_triple(1))
                    .map(|(x, y, z)| gfx::image::Extent {
                        width: x,
                        height: y,
                        depth: z,
                    })
                    .next()?
            };

            use gfx::Device;

            let framebuffer = device
                .device
                .create_framebuffer(render_pass_raw, views, extent)
                .ok()?;

            res.framebuffers.insert(pass, (framebuffer, extent));

            Some(())
        }
        PassInfo::Compute(_) => {
            // nothing to prepare for compute passes
            Some(())
        }
    }
}

fn create_render_pass(
    device: &DeviceContext,
    storages: &mut Storages,
    resolved_graph: &GraphResourcesResolved,
    pass: PassId,
) -> Option<RenderPassHandle> {
    let attachments = {
        resolved_graph.pass_writes[&pass]
            .iter()
            // we are only interested in images that are written to as color or depth
            .filter(|(_, ty, _)| match ty {
                ResourceWriteType::Image(ImageWriteType::Color) => true,
                ResourceWriteType::Image(ImageWriteType::DepthStencil) => true,
                _ => false,
            })
            .map(|(res, _ty, _binding)| {
                let (origin, info) = resolved_graph.create_info(*res).unwrap();

                let info = match info {
                    ResourceCreateInfo::Image(img) => img,
                    _ => unreachable!(),
                };

                let clear = origin == *res;

                let load_op = if clear {
                    gfx::pass::AttachmentLoadOp::Clear
                } else {
                    gfx::pass::AttachmentLoadOp::Load
                };

                let initial_layout = if clear {
                    gfx::image::Layout::Undefined
                } else {
                    gfx::image::Layout::Preinitialized
                };

                gfx::pass::Attachment {
                    format: Some(info.format.into()),
                    samples: 0,
                    ops: gfx::pass::AttachmentOps {
                        load: load_op,
                        store: gfx::pass::AttachmentStoreOp::Store,
                    },
                    // TODO stencil and depth
                    stencil_ops: gfx::pass::AttachmentOps::DONT_CARE,
                    // TODO depth/stencil
                    // TODO Better layout transitions
                    layouts: initial_layout..gfx::image::Layout::General,
                }
            })
            .collect::<SmallVec<[_; 16]>>()
    };

    let color_attachments = attachments
        .as_slice()
        .iter()
        .enumerate()
        .map(|(i, _)| (i, gfx::image::Layout::ColorAttachmentOptimal))
        .collect::<SmallVec<[_; 16]>>();

    let subpass = gfx::pass::SubpassDesc {
        colors: color_attachments.as_slice(),
        // TODO OOOOOOOOOO
        depth_stencil: None,
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

    let create_info = RenderPassCreateInfo {
        attachments: attachments.as_slice(),
        subpasses: &[subpass],
        dependencies: &[dependencies],
    };

    storages
        .render_pass
        .create(device, &[create_info])
        .remove(0)
        .ok()
}

fn create_pipeline_compute(
    device: &DeviceContext,
    storages: &mut Storages,
    resolved: &GraphResourcesResolved,
    pass: PassId,
    info: &ComputePassInfo,
) -> Option<(
    PipelineHandle,
    types::DescriptorSetLayout,
    types::DescriptorPool,
    types::DescriptorSet,
)> {
    let (mut layouts, pass_stuff) = create_pipeline_base(
        device,
        resolved,
        storages.material,
        pass,
        &info.materials[..],
    )?;

    // insert the pass set layout
    layouts.insert(0, &pass_stuff.0);

    let layouts = layouts
        .into_iter()
        .map(|(_, data)| data)
        .collect::<Vec<_>>();

    let create_info = crate::pipeline::ComputePipelineCreateInfo {
        shader: crate::pipeline::ShaderInfo {
            entry: &info.shader.entry,
            content: &info.shader.content,
        },
        descriptor_set_layout: &layouts[..],
    };

    let pipeline_handle = storages
        .pipeline
        .create_compute_pipelines(device, &[create_info])
        .remove(0)
        .ok();

    pipeline_handle.map(move |handle| (handle, pass_stuff.0, pass_stuff.1, pass_stuff.2))
}

fn create_pipeline_graphics(
    device: &DeviceContext,
    storages: &mut Storages,
    resolved_graph: &GraphResourcesResolved,
    render_pass: RenderPassHandle,
    pass: PassId,
    info: &GraphicsPassInfo,
) -> Option<(
    PipelineHandle,
    types::DescriptorSetLayout,
    types::DescriptorPool,
    types::DescriptorSet,
)> {
    use crate::pipeline;

    let (mut layouts, pass_stuff) = create_pipeline_base(
        device,
        resolved_graph,
        storages.material,
        pass,
        &info.materials[..],
    )?;

    // insert the pass set layout
    layouts.insert(0, &pass_stuff.0);

    let layouts = layouts
        .into_iter()
        .map(|(_, data)| data)
        .collect::<Vec<_>>();

    let create_info = pipeline::GraphicsPipelineCreateInfo {
        vertex_attribs: info.vertex_attrib.clone(),
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
        blend_modes: &info.blend_modes[..],
    };

    let pipeline_handle = storages
        .pipeline
        .create_graphics_pipelines(
            device,
            storages.render_pass,
            storages.vertex_attrib,
            render_pass,
            &[create_info],
        )
        .remove(0)
        .ok();

    pipeline_handle.map(move |handle| (handle, pass_stuff.0, pass_stuff.1, pass_stuff.2))
}

fn create_resource(
    usages: &ResourceUsages,
    device: &DeviceContext,
    context: &ExecutionContext,
    storages: &mut Storages,
    id: ResourceId,
    info: &ResourceCreateInfo,
    res: &mut GraphResources,
) -> Option<()> {
    match info {
        ResourceCreateInfo::Image(img) => {
            // find out the size and kind of the image

            let raw_dim = img.size_mode.absolute(context.reference_size);
            let dim = image::ImageDimension::D2 {
                x: raw_dim.0,
                y: raw_dim.1,
            };

            let kind = image::ViewKind::D2;

            let format = img.format;

            // any flags that will be needed
            let usages = &usages.image[&id];

            // let's go!
            let create_info = image::ImageCreateInfo {
                dimension: dim,
                num_layers: 1,
                num_samples: 1,
                num_mipmaps: 1,
                format,
                kind,
                usage: usages.0.clone(),
                is_transient: false,
            };

            let img_handle = storages
                .image
                .create(device, &[create_info])
                .remove(0)
                .ok()?;

            res.images.insert(id, img_handle);

            // If the image is used for sampling then it means some other pass will read from it
            // as a color image. In that case we create a sampler for this image as well
            if usages.0.contains(gfx::image::Usage::SAMPLED) {
                let sampler = storages
                    .sampler
                    .create(
                        device,
                        &[sampler::SamplerCreateInfo {
                            min_filter: sampler::Filter::Linear,
                            mip_filter: sampler::Filter::Linear,
                            mag_filter: sampler::Filter::Linear,
                            wrap_mode: (
                                sampler::WrapMode::Clamp,
                                sampler::WrapMode::Clamp,
                                sampler::WrapMode::Clamp,
                            ),
                        }],
                    )
                    .remove(0);
                res.samplers.insert(id, sampler);
            }

            Some(())
        }
        ResourceCreateInfo::Buffer(buf) => {
            let (usage, properties) = usages.buffer[&id];

            let create_info = crate::buffer::BufferCreateInfo {
                size: buf.size,
                is_transient: false,
                usage,
                properties,
            };

            let buffer = storages
                .buffer
                .create(device, &[create_info])
                .remove(0)
                .ok()?;

            res.buffers.insert(id, buffer);

            Some(())
        }
    }
}

fn create_pipeline_base<'a>(
    device: &DeviceContext,
    resolved: &GraphResourcesResolved,
    material_storage: &'a MaterialStorage,
    pass: PassId,
    materials: &[(usize, MaterialHandle)],
) -> Option<(
    BTreeMap<usize, &'a types::DescriptorSetLayout>,
    (
        types::DescriptorSetLayout,
        types::DescriptorPool,
        types::DescriptorSet,
    ),
)> {
    let mut sets = BTreeMap::new();

    let (core_desc, core_range) = {
        let reads = resolved.pass_reads[&pass].iter();

        let samplers = reads.clone().filter(|(_, _, _, sampler)| sampler.is_some());

        let sampler_descriptors =
            samplers
                .clone()
                .map(|(_, _, _, binding)| gfx::pso::DescriptorSetLayoutBinding {
                    binding: binding.unwrap() as u32,
                    ty: gfx::pso::DescriptorType::Sampler,
                    count: 1,
                    stage_flags: gfx::pso::ShaderStageFlags::ALL,
                    immutable_samplers: false,
                });

        // writing to resources that are not color or depth images happens via descriptors as well
        let writes = resolved.pass_writes[&pass]
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
                    binding: *binding as u32,
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
                    binding: (*binding as u32),
                    ty: match ty {
                        ResourceReadType::Image(img) => match img {
                            ImageReadType::Color => gfx::pso::DescriptorType::SampledImage,
                            ImageReadType::Storage => gfx::pso::DescriptorType::StorageImage,
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

    // material sets
    {
        for (set, material) in materials {
            let mat = match material_storage.raw(*material) {
                Some(mat) => mat,
                None => continue,
            };

            let layout = &mat.desc_set_layout;

            sets.insert(*set, layout);
        }
    }

    use gfx::DescriptorPool;
    use gfx::Device;

    let pass_set_layout = device
        .device
        .create_descriptor_set_layout(core_desc, &[])
        .ok()?;

    let mut pass_set_pool = device.device.create_descriptor_pool(1, core_range).ok()?;

    let pass_set = pass_set_pool.allocate_set(&pass_set_layout).ok()?;

    Some((sets, (pass_set_layout, pass_set_pool, pass_set)))
}
