/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;

use crate::graph::resolve::GraphResourcesResolved;
use crate::graph::ExecutionContext;

use crate::graph::{
    BufferWriteType, ImageReadType, ImageWriteType, ResourceReadType, ResourceWriteType,
};

use gfx::Device;

use smallvec::SmallVec;

use crate::device::DeviceContext;
use crate::resources::command_pool::{CommandPoolCompute, CommandPoolGraphics};
use crate::resources::semaphore_pool::{SemaphoreList, SemaphorePool};

pub(crate) unsafe fn execute(
    device: &DeviceContext,
    sem_pool: &mut SemaphorePool,
    sem_list: &mut SemaphoreList,
    cmd_pool_gfx: &CommandPoolGraphics,
    cmd_pool_cmpt: &CommandPoolCompute,
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
                image: storages.image,
            };

            for _ in 0..batch.passes.len() {
                let sem = sem_pool.alloc();
                sem_list.add_next_semaphore(sem);
            }

            // TODO FEARLESS CONCURRENCY!!!
            for pass in &batch.passes {
                // descriptor set stuff
                let mat_raw = res.pass_mats.get(pass);
                let set_raw = mat_raw
                    .and_then(|hndl| storages.material.raw(hndl.0).map(|mat| (hndl.1, mat)))
                    .and_then(|(inst, mat)| mat.instance_raw(inst).map(|inst| &inst.set));

                if let Some(set) = set_raw {
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
                                    ImageReadType::DepthStencil => {
                                        // this is a not a "real" read type
                                        SmallVec::new()
                                    }
                                }
                            }
                            ResourceReadType::Buffer(_buf) => unimplemented!(),
                            ResourceReadType::Virtual => {
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
                                        ImageWriteType::Storage => {
                                            let img_handle = res.images[rid];
                                            let image = storages.image.raw(img_handle).unwrap();

                                            Some(gfx::pso::DescriptorSetWrite {
                                                set,
                                                binding: (*binding) as u32,
                                                array_offset: 0,
                                                descriptors: std::iter::once(
                                                    gfx::pso::Descriptor::Image(
                                                        &image.view,
                                                        gfx::image::Layout::General,
                                                    ),
                                                ),
                                            })
                                        }
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

                    let framebuffer = res.framebuffers.get(pass);
                    let framebuffer_extent = framebuffer
                        .map(|f| (f.1.width, f.1.height))
                        .unwrap_or((1, 1));

                    let framebuffer = framebuffer.map(|f| &f.0);

                    let viewport = gfx::pso::Viewport {
                        // TODO depth boundaries
                        depth: 0.0..1.0,
                        rect: gfx::pso::Rect {
                            x: 0,
                            y: 0,
                            w: framebuffer_extent.0 as i16,
                            h: framebuffer_extent.1 as i16,
                        },
                    };

                    let submit = {
                        let mut raw_cmd = cmd_pool_gfx.alloc();

                        raw_cmd.bind_graphics_pipeline(&pipeline.pipeline);

                        raw_cmd.set_viewports(0, &[viewport.clone()]);
                        raw_cmd.set_scissors(0, &[viewport.rect]);

                        if let Some(set) = set_raw {
                            raw_cmd.bind_graphics_descriptor_sets(
                                &pipeline.layout,
                                0,
                                Some(set),
                                &[],
                            );
                        }

                        let pass_impl = &graph.passes_gfx_impl[&pass.0];

                        {
                            let mut command = crate::graph::command::GraphicsCommandBuffer {
                                buf: &mut *raw_cmd,
                                storages: &read_storages,
                                framebuffer: framebuffer,
                                viewport_rect: viewport.rect,
                                pipeline_layout: &pipeline.layout,
                                render_pass: render_pass,
                            };

                            pass_impl.execute(store, &mut command);
                        }

                        raw_cmd.finish();
                        raw_cmd
                    };

                    {
                        let submission = gfx::Submission {
                            command_buffers: Some(&*submit),
                            wait_semaphores: sem_pool
                                .list_prev_sems(sem_list)
                                .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
                            signal_semaphores: sem_pool.list_next_sems(sem_list),
                        };

                        device.graphics_queue().submit(submission, None);
                    }
                }

                // process compute pass
                if let Some(handle) = base_res.pipelines_compute.get(pass) {
                    let pipeline = storages.pipeline.raw_compute(*handle).unwrap();

                    let submit = {
                        let mut raw_cmd = cmd_pool_cmpt.alloc();

                        raw_cmd.bind_compute_pipeline(&pipeline.pipeline);

                        if let Some(set) = set_raw {
                            raw_cmd.bind_compute_descriptor_sets(
                                &pipeline.layout,
                                0,
                                Some(set),
                                &[],
                            );
                        }

                        let pass_impl = &graph.passes_cmpt_impl[&pass.0];

                        {
                            let mut cmd_buffer = crate::graph::command::ComputeCommandBuffer {
                                buf: &mut *raw_cmd,
                                storages: &read_storages,
                                pipeline_layout: &pipeline.layout,
                            };

                            pass_impl.execute(store, &mut cmd_buffer);
                        }

                        raw_cmd.finish();
                        raw_cmd
                    };

                    {
                        let submission = gfx::Submission {
                            command_buffers: Some(&*submit),
                            wait_semaphores: sem_pool
                                .list_prev_sems(sem_list)
                                .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
                            signal_semaphores: sem_pool.list_next_sems(sem_list),
                        };

                        device.compute_queue().submit(submission, None);
                    }
                }

                sem_list.advance();
            }
        }
    }
}
