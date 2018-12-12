/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;

use crate::graph::resolve::GraphResourcesResolved;
use crate::graph::ExecutionContext;

use crate::graph::{ImageReadType, ResourceReadType};

use crate::types::CommandPool;

use gfx::Device;

use smallvec::SmallVec;

use crate::device::DeviceContext;
use crate::resources::semaphore_pool::{SemaphoreList, SemaphorePool};

pub(crate) fn execute(
    device: &DeviceContext,
    sem_pool: &mut SemaphorePool,
    sem_list: &mut SemaphoreList,
    cmd_pool: &mut CommandPool<gfx::Graphics>,
    storages: &mut Storages,
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
                let pipeline = {
                    let handle = base_res.pipelines_graphic[pass];
                    storages.pipeline.raw_graphics(handle).unwrap()
                };

                let render_pass = {
                    let handle = base_res.render_passes[pass];
                    storages.render_pass.raw(handle).unwrap()
                };

                let (_set_layout, _pool, set) = &base_res.pipelines_desc_set[pass];

                // TODO transition resource layouts

                // fill descriptor set
                {
                    let writes = resolved_graph.pass_reads[pass]
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

                                        let mut vec: SmallVec<[_; 2]> = SmallVec::new();
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
                        })
                        .flatten();

                    device.device.write_descriptor_sets(writes);
                }

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

                // TODO custom clear values (and multiple targets)
                let clear_values = &[gfx::command::ClearValue::Color(
                    gfx::command::ClearColor::Float([0.0, 0.0, 0.0, 0.0]),
                )];

                let submit = {
                    let mut raw_cmd = cmd_pool.acquire_command_buffer(false);
                    raw_cmd.bind_graphics_pipeline(&pipeline.pipeline);

                    raw_cmd.set_viewports(0, &[viewport.clone()]);
                    raw_cmd.set_scissors(0, &[viewport.rect]);

                    raw_cmd.bind_graphics_descriptor_sets(&pipeline.layout, 0, Some(set), &[]);

                    let pass_impl = &graph.passes_impl[pass.0];

                    {
                        let encoder = raw_cmd.begin_render_pass_inline(
                            render_pass,
                            &framebuffer,
                            viewport.rect,
                            clear_values,
                        );
                        let mut command = crate::graph::command::CommandBuffer {
                            encoder,
                            storages: &read_storages,
                            pipeline_layout: &pipeline.layout,
                        };

                        pass_impl.execute(&mut command);
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

                sem_list.advance();
            }
        }
    }
}
