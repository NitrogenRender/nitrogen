/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;

use crate::graph::resolve::GraphWithNamesResolved;
use crate::graph::ExecutionContext;

use crate::graph::{
    BufferWriteType, ImageReadType, ImageWriteType, ResourceReadType, ResourceWriteType,
};

use gfx::Device;

use smallvec::SmallVec;

use crate::device::DeviceContext;
use crate::graph::builder::PassType;
use crate::graph::pass::dispatcher::RawComputeDispatcher;
use crate::resources::command_pool::{CommandPoolCompute, CommandPoolGraphics};
use crate::resources::material::{MaterialInstanceHandle, MaterialStorage};
use crate::resources::semaphore_pool::{SemaphoreList, SemaphorePool};
use crate::submit_group::QueueSyncRefs;

pub(crate) unsafe fn execute<'a>(
    device: &'a DeviceContext,
    sync: &mut QueueSyncRefs,
    (pool_gfx, pool_cmpt): (&CommandPoolGraphics, &CommandPoolCompute),
    storages: &'a Storages<'a>,
    store: &crate::graph::Store,
    graph: &'a mut crate::graph::Graph,
    res: &GraphResources,
    _context: &ExecutionContext,
) -> Result<(), GraphExecError> {
    // let exec_graph = &graph.exec_graph;

    for batch in &graph.exec_graph.pass_execution {
        for _ in 0..batch.passes.len() {
            let sem = sync.sem_pool.alloc();
            sync.sem_list.add_next_semaphore(sem);
        }

        for pass in &batch.passes {
            if let Some(mat) = graph.pass_resources.pass_material.get(pass) {
                write_pass_descriptor_set(
                    device,
                    storages,
                    *mat,
                    &graph.compiled_graph.graph_resources,
                    res,
                    *pass,
                );
            }

            let ty = graph.compiled_graph.graph_resources.pass_types[pass];

            match ty {
                PassType::Compute => {
                    let accessor = &graph.compiled_graph.compute_passes[pass];

                    let mut cmd_buf = pool_cmpt.alloc();
                    cmd_buf.begin();

                    {
                        let raw_dispatcher = RawComputeDispatcher {
                            cmd: &mut cmd_buf,
                            device,
                            storages,
                            pass_id: *pass,
                            res: &mut graph.pass_resources,
                        };

                        (accessor.execute)(store, raw_dispatcher)?;
                    }

                    cmd_buf.finish();

                    {
                        let submission = gfx::Submission {
                            command_buffers: Some(&*cmd_buf),
                            wait_semaphores: sync
                                .sem_pool
                                .list_prev_sems(sync.sem_list)
                                .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
                            signal_semaphores: sync.sem_pool.list_next_sems(sync.sem_list),
                        };

                        device.compute_queue().submit(submission, None);
                    }
                }
                PassType::Graphics => unimplemented!("Graphics pass execution"),
            }

            /*
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
                    raw_cmd.begin();

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
                            framebuffer,
                            viewport_rect: viewport.rect,
                            pipeline_layout: &pipeline.layout,
                            render_pass,
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
            */

            /*
            // process compute pass
            if let Some(handle) = base_res.pipelines_compute.get(pass) {
                let pipeline = storages.pipeline.raw_compute(*handle).unwrap();

                let submit = {
                    let mut raw_cmd = cmd_pool_cmpt.alloc();
                    raw_cmd.begin();

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
            */
            sync.sem_list.advance();
        }
    }

    Ok(())
}

/// Write resources to the pass descriptor set.
unsafe fn write_pass_descriptor_set(
    device: &DeviceContext,
    storages: &Storages,
    pass_mat_instance: MaterialInstanceHandle,
    resolved_graph: &GraphWithNamesResolved,
    res: &GraphResources,
    pass: PassId,
) -> Option<()> {
    let image_storage = storages.image.borrow();
    let sampler_storage = storages.sampler.borrow();
    let buffer_storage = storages.buffer.borrow();
    let material_storage = storages.material.borrow();

    let material = material_storage.raw(pass_mat_instance.material())?;

    let instance = material.instance_raw(pass_mat_instance.instance)?;
    let set = &instance.set;

    let reads = resolved_graph.pass_reads[&pass]
        .iter()
        .map(|(rid, ty, binding, samp)| {
            let rid = &resolved_graph.moved_from(*rid).unwrap();

            match ty {
                ResourceReadType::Image(img) => {
                    let img_handle = &res.images[rid];
                    let image = image_storage.raw(*img_handle).unwrap();

                    match img {
                        ImageReadType::Color => {
                            let img_desc = gfx::pso::DescriptorSetWrite {
                                set,
                                binding: u32::from(*binding),
                                array_offset: 0,
                                descriptors: std::iter::once(gfx::pso::Descriptor::Image(
                                    &image.view,
                                    gfx::image::Layout::General,
                                )),
                            };

                            // prepare final descriptor writes
                            let mut vec = SmallVec::<[_; 2]>::new();
                            vec.push(img_desc);

                            if let Some(samp_bind) = *samp {
                                let samp_handle = &res.samplers[rid];
                                let sampler = sampler_storage.raw(*samp_handle).unwrap();

                                let sampler_desc = gfx::pso::DescriptorSetWrite {
                                    set,
                                    binding: u32::from(samp_bind),
                                    array_offset: 0,
                                    descriptors: std::iter::once(gfx::pso::Descriptor::Sampler(
                                        &sampler.0,
                                    )),
                                };
                                vec.push(sampler_desc);
                            }

                            vec
                        }
                        ImageReadType::Storage => {
                            let desc = gfx::pso::DescriptorSetWrite {
                                set,
                                binding: u32::from(*binding),
                                array_offset: 0,
                                descriptors: std::iter::once(gfx::pso::Descriptor::Image(
                                    &image.view,
                                    gfx::image::Layout::General,
                                )),
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
            }
        })
        .flatten();

    device.device.write_descriptor_sets(reads);

    let writes = resolved_graph.pass_writes[&pass]
        .iter()
        .filter_map(|(rid, ty, binding)| {
            let rid = &resolved_graph.moved_from(*rid)?;
            match ty {
                ResourceWriteType::Buffer(buf) => {
                    let buf_handle = &res.buffers[rid];
                    let buffer = buffer_storage.raw(*buf_handle).unwrap();
                    match buf {
                        BufferWriteType::Storage => Some(gfx::pso::DescriptorSetWrite {
                            set,
                            binding: u32::from(*binding),
                            array_offset: 0,
                            descriptors: std::iter::once(gfx::pso::Descriptor::Buffer(
                                buffer.buffer.raw(),
                                None..None,
                            )),
                        }),
                        _ => unimplemented!(),
                    }
                }
                ResourceWriteType::Image(img) => {
                    match img {
                        // those two use render pass attachments, not descriptor sets
                        ImageWriteType::Color | ImageWriteType::DepthStencil => None,
                        ImageWriteType::Storage => {
                            let img_handle = res.images[rid];
                            let image = image_storage.raw(img_handle).unwrap();

                            Some(gfx::pso::DescriptorSetWrite {
                                set,
                                binding: u32::from(*binding),
                                array_offset: 0,
                                descriptors: std::iter::once(gfx::pso::Descriptor::Image(
                                    &image.view,
                                    gfx::image::Layout::General,
                                )),
                            })
                        }
                    }
                }
            }
        });

    device.device.write_descriptor_sets(writes);

    Some(())
}
