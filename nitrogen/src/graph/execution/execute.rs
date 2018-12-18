/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;

use crate::graph::resolve::GraphResourcesResolved;
use crate::graph::ExecutionContext;

use crate::graph::{
    BufferWriteType, ImageReadType, ImageWriteType, ResourceCreateInfo, ResourceReadType,
    ResourceWriteType,
};

use crate::types::CommandPool;

use gfx::Device;

use smallvec::SmallVec;

use crate::device::DeviceContext;
use crate::resources::semaphore_pool::{SemaphoreList, SemaphorePool};

pub(crate) fn execute(
    device: &DeviceContext,
    sem_pool: &mut SemaphorePool,
    sem_list: &mut SemaphoreList,
    cmd_pool_gfx: &mut CommandPool<gfx::Graphics>,
    cmd_pool_cmpt: &mut CommandPool<gfx::Compute>,
    storages: &mut Storages,
    store: &crate::graph::Store,
    exec_graph: &ExecutionGraph,
    resolved_graph: &GraphResourcesResolved,
    graph: &crate::graph::Graph,
    base_res: &GraphBaseResources,
    res: &GraphResources,
    _context: &ExecutionContext,
) {
    for batch in &exec_graph.pass_execution {
        // perform copies
        {}

        // execute passes
        {
            let read_storages = crate::graph::command::ReadStorages {
                buffer: storages.buffer,
                material: storages.material,
            };

            for _ in 0..batch.passes.len() {
                let sem = sem_pool.alloc();
                sem_list.add_next_semaphore(sem);
            }

            // TODO FEARLESS CONCURRENCY!!!
            for pass in &batch.passes {
                // descriptor set stuff
                let (_set_layout, _pool, set) = &base_res.pipelines_desc_set[pass];

                {
                    let reads = resolved_graph.pass_reads[pass]
                        .iter()
                        .map(|(rid, ty, binding, samp)| match ty {
                            ResourceReadType::Image(img) => {
                                let img_handle = &res.images[rid];
                                let image = storages.image.raw(*img_handle).unwrap();

                                match img {
                                    ImageReadType::Color => {
                                        let samp_handle = &res.samplers[rid];
                                        let sampler = storages.sampler.raw(*samp_handle).unwrap();

                                        let img_desc = gfx::pso::DescriptorSetWrite {
                                            set,
                                            binding: (*binding) as u32,
                                            array_offset: 0,
                                            descriptors: std::iter::once(
                                                gfx::pso::Descriptor::Image(
                                                    &image.view,
                                                    gfx::image::Layout::General,
                                                ),
                                            ),
                                        };

                                        let sampler_desc = gfx::pso::DescriptorSetWrite {
                                            set,
                                            binding: samp.clone().unwrap() as u32,
                                            array_offset: 0,
                                            descriptors: std::iter::once(
                                                gfx::pso::Descriptor::Sampler(sampler),
                                            ),
                                        };

                                        let mut vec = SmallVec::<[_; 2]>::new();
                                        vec.push(img_desc);
                                        vec.push(sampler_desc);

                                        vec
                                    }
                                    ImageReadType::Storage => {
                                        let desc = gfx::pso::DescriptorSetWrite {
                                            set,
                                            binding: (*binding) as u32,
                                            array_offset: 0,
                                            descriptors: std::iter::once(
                                                gfx::pso::Descriptor::Image(
                                                    &image.view,
                                                    gfx::image::Layout::General,
                                                ),
                                            ),
                                        };

                                        let mut res: SmallVec<[_; 2]> = SmallVec::new();
                                        res.push(desc);

                                        res
                                    }
                                }
                            }
                            ResourceReadType::Buffer(_buf) => unimplemented!(),
                            ResourceReadType::External => {
                                // Nothing to do...
                                SmallVec::new()
                            }
                        })
                        .flatten();

                    device.device.write_descriptor_sets(reads);

                    let writes =
                        resolved_graph.pass_writes[pass]
                            .iter()
                            .filter_map(|(rid, ty, binding)| match ty {
                                ResourceWriteType::Buffer(buf) => {
                                    let buf_handle = &res.buffers[rid];
                                    let buffer = storages.buffer.raw(*buf_handle).unwrap();
                                    match buf {
                                        BufferWriteType::Storage => {
                                            Some(gfx::pso::DescriptorSetWrite {
                                                set,
                                                binding: (*binding) as u32,
                                                array_offset: 0,
                                                descriptors: std::iter::once(
                                                    gfx::pso::Descriptor::Buffer(
                                                        buffer.buffer.raw(),
                                                        None..None,
                                                    ),
                                                ),
                                            })
                                        }
                                        _ => unimplemented!(),
                                    }
                                }
                                ResourceWriteType::Image(img) => {
                                    match img {
                                        // those two use render pass attachments, not descriptor sets
                                        ImageWriteType::Color | ImageWriteType::DepthStencil => {
                                            None
                                        }
                                        ImageWriteType::Storage => unimplemented!(),
                                    }
                                }
                            });

                    device.device.write_descriptor_sets(writes);
                }

                // process graphics pass
                if let Some(handle) = base_res.pipelines_graphic.get(pass) {
                    let pipeline = storages.pipeline.raw_graphics(*handle).unwrap();

                    let render_pass = {
                        let handle = base_res.render_passes[pass];
                        storages.render_pass.raw(handle).unwrap()
                    };

                    // TODO transition resource layouts

                    let framebuffer = &res.framebuffers[pass];
                    let framebuffer_extent = framebuffer.1;
                    let framebuffer = &framebuffer.0;

                    let viewport = gfx::pso::Viewport {
                        depth: 0.0..1.0,
                        rect: gfx::pso::Rect {
                            x: 0,
                            y: 0,
                            w: framebuffer_extent.width as i16,
                            h: framebuffer_extent.height as i16,
                        },
                    };

                    // clear values for image targets
                    // TODO handle depth and storage clears
                    let mut clear_values = SmallVec::<[_; 16]>::new();

                    'clear_loop: for (id, ty, binding) in &resolved_graph.pass_writes[pass] {
                        if !batch.resource_create.contains(id) {
                            continue 'clear_loop;
                        }

                        // only clear if image
                        match ty {
                            ResourceWriteType::Image(img) => match img {
                                ImageWriteType::Color => {
                                    let info = &resolved_graph.infos[id];
                                    let image_info = match info {
                                        ResourceCreateInfo::Image(info) => info,
                                        _ => unreachable!(),
                                    };
                                    clear_values.push((binding, image_info.clear_color));
                                }
                                _ => unimplemented!(),
                            },
                            ResourceWriteType::Buffer(_) => continue 'clear_loop,
                        }
                    }

                    clear_values
                        .as_mut_slice()
                        .sort_by_key(|(binding, _)| *binding);

                    let clear_values = clear_values.into_iter().map(|(_, color)| {
                        gfx::command::ClearValue::Color(gfx::command::ClearColor::Float(color))
                    });

                    let submit = {
                        let mut raw_cmd = cmd_pool_gfx.acquire_command_buffer(false);
                        raw_cmd.bind_graphics_pipeline(&pipeline.pipeline);

                        raw_cmd.set_viewports(0, &[viewport.clone()]);
                        raw_cmd.set_scissors(0, &[viewport.rect]);

                        raw_cmd.bind_graphics_descriptor_sets(&pipeline.layout, 0, Some(set), &[]);

                        let pass_impl = &graph.passes_gfx_impl[&pass.0];

                        {
                            let encoder = raw_cmd.begin_render_pass_inline(
                                render_pass,
                                &framebuffer,
                                viewport.rect,
                                clear_values,
                            );

                            let mut command = crate::graph::command::GraphicsCommandBuffer {
                                encoder,
                                storages: &read_storages,
                                pipeline_layout: &pipeline.layout,
                            };

                            pass_impl.execute(store, &mut command);
                        }

                        raw_cmd.finish()
                    };

                    {
                        let submission = gfx::Submission::new()
                            .wait_on(
                                sem_pool
                                    .list_prev_sems(sem_list)
                                    .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
                            )
                            .signal(sem_pool.list_next_sems(sem_list))
                            .submit(Some(submit));
                        device.graphics_queue().submit(submission, None);
                    }
                }

                // process compute pass
                if let Some(handle) = base_res.pipelines_compute.get(pass) {
                    let pipeline = storages.pipeline.raw_compute(*handle).unwrap();

                    let submit = {
                        let mut raw_cmd = cmd_pool_cmpt.acquire_command_buffer(false);

                        raw_cmd.bind_compute_pipeline(&pipeline.pipeline);

                        raw_cmd.bind_compute_descriptor_sets(&pipeline.layout, 0, Some(set), &[]);

                        let pass_impl = &graph.passes_cmpt_impl[&pass.0];

                        let raw_cmd = {
                            let mut cmd_buffer = crate::graph::command::ComputeCommandBuffer {
                                buf: raw_cmd,
                                storages: &read_storages,
                                pipeline_layout: &pipeline.layout,
                            };

                            pass_impl.execute(store, &mut cmd_buffer);

                            cmd_buffer.buf
                        };

                        raw_cmd.finish()
                    };

                    {
                        let submission = gfx::Submission::new()
                            .wait_on(
                                sem_pool
                                    .list_prev_sems(sem_list)
                                    .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
                            )
                            .signal(sem_pool.list_next_sems(sem_list))
                            .submit(Some(submit));
                        device.compute_queue().submit(submission, None);
                    }
                }

                sem_list.advance();
            }
        }
    }
}
