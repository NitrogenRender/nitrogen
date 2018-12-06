/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx;
use gfx::Device;

use back;

use crate::*;
use device::DeviceContext;

use resources::semaphore_pool::{Semaphore, SemaphoreList, SemaphorePool};

use smallvec::SmallVec;

use std::ops::Drop;
use std::sync::Arc;

pub struct SubmitGroup {
    sem_pool: SemaphorePool,

    pool_graphics: types::CommandPool<gfx::Graphics>,
    pool_compute: types::CommandPool<gfx::Compute>,
    pool_transfer: types::CommandPool<gfx::Transfer>,

    sem_list: SemaphoreList,
}

impl SubmitGroup {
    pub fn new(device: Arc<DeviceContext>) -> Self {
        let (gfx, cmpt, trns) = {
            let gfx = device
                .device
                .create_command_pool_typed(
                    device.graphics_queue_group(),
                    gfx::pool::CommandPoolCreateFlags::empty(),
                    0,
                )
                .unwrap();
            let cmpt = device
                .device
                .create_command_pool_typed(
                    device.compute_queue_group(),
                    gfx::pool::CommandPoolCreateFlags::empty(),
                    0,
                )
                .unwrap();
            let trns = device
                .device
                .create_command_pool_typed(
                    device.transfer_queue_group(),
                    gfx::pool::CommandPoolCreateFlags::empty(),
                    0,
                )
                .unwrap();

            (gfx, cmpt, trns)
        };

        SubmitGroup {
            pool_graphics: gfx,
            pool_compute: cmpt,
            pool_transfer: trns,

            sem_pool: SemaphorePool::new(device),
            sem_list: SemaphoreList::new(),
        }
    }

    pub fn display_present(
        &mut self,
        ctx: &mut Context,
        display: DisplayHandle,
        resources: &graph::ExecutionResources,
    ) {
        let image_id = if resources.outputs.len() != 1 {
            return;
        } else {
            resources.outputs[0]
        };

        let image = resources.images[&image_id];

        let sampler = resources.samplers[&image_id];

        ctx.displays[display].present(
            &ctx.device_ctx,
            &mut self.sem_pool,
            &mut self.sem_list,
            &mut self.pool_graphics,
            &ctx.image_storage,
            image,
            &ctx.sampler_storage,
            sampler,
        );

        self.sem_list.advance()
    }

    pub fn graph_render(
        &mut self,
        ctx: &mut Context,
        graph: graph::GraphHandle,
        exec_context: &graph::ExecutionContext,
    ) -> graph::ExecutionResources {
        ctx.graph_storage.execute(
            &ctx.device_ctx,
            &mut self.sem_pool,
            &mut self.sem_list,
            &mut self.pool_graphics,
            &mut ctx.render_pass_storage,
            &mut ctx.pipeline_storage,
            &mut ctx.image_storage,
            &mut ctx.buffer_storage,
            &ctx.vertex_attrib_storage,
            &mut ctx.sampler_storage,
            &ctx.material_storage,
            graph,
            exec_context,
        )
    }

    pub fn image_upload_data(
        &mut self,
        ctx: &mut Context,
        images: &[(image::ImageHandle, image::ImageUploadInfo)],
    ) -> SmallVec<[image::Result<()>; 16]> {
        ctx.image_storage.upload_data(
            &ctx.device_ctx,
            &self.sem_pool,
            &mut self.sem_list,
            &mut self.pool_transfer,
            &ctx.transfer,
            images,
        )
    }

    pub fn buffer_upload_data<T>(
        &mut self,
        ctx: &mut Context,
        data: &[(buffer::BufferHandle, buffer::BufferUploadInfo<T>)],
    ) -> SmallVec<[buffer::Result<()>; 16]> {
        ctx.buffer_storage.upload_data(
            &ctx.device_ctx,
            &self.sem_pool,
            &mut self.sem_list,
            &mut self.pool_transfer,
            &ctx.transfer,
            data,
        )
    }

    pub fn wait(&mut self, ctx: &mut Context) {
        let mut fence = ctx.device_ctx.device.create_fence(false).unwrap();

        {
            let submit = gfx::Submission::new().wait_on(
                self.sem_pool
                    .list_prev_sems(&self.sem_list)
                    .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
            );

            ctx.device_ctx
                .transfer_queue()
                .submit(submit, Some(&mut fence));

            ctx.device_ctx.device.wait_for_fence(&fence, !0);
        }

        self.sem_list.advance();

        ctx.device_ctx.device.destroy_fence(fence);

        self.sem_pool.clear();

        self.pool_graphics.reset();
        self.pool_compute.reset();
        self.pool_transfer.reset();
    }

    pub fn release(mut self, ctx: &mut Context) {
        ctx.device_ctx
            .device
            .destroy_command_pool(self.pool_graphics.into_raw());
        ctx.device_ctx
            .device
            .destroy_command_pool(self.pool_compute.into_raw());
        ctx.device_ctx
            .device
            .destroy_command_pool(self.pool_transfer.into_raw());

        self.sem_pool.reset();
    }
}
