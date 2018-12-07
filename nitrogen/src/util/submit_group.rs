/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx;
use gfx::Device;

use buffer::BufferTypeInternal;
use image::ImageType;

use crate::*;
use device::DeviceContext;

use resources::semaphore_pool::{SemaphoreList, SemaphorePool};

use smallvec::SmallVec;

use std::sync::Arc;

pub struct SubmitGroup {
    sem_pool: SemaphorePool,

    pool_graphics: types::CommandPool<gfx::Graphics>,
    pool_compute: types::CommandPool<gfx::Compute>,
    pool_transfer: types::CommandPool<gfx::Transfer>,

    sem_list: SemaphoreList,
    res_destroys: ResourceList,
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

            sem_pool: SemaphorePool::new(device.clone()),
            sem_list: SemaphoreList::new(),

            res_destroys: ResourceList::new(device),
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

        ctx.displays[display].present(
            &ctx.device_ctx,
            &mut self.sem_pool,
            &mut self.sem_list,
            &mut self.pool_graphics,
            &ctx.image_storage,
            image,
        );
    }

    pub fn display_setup_swapchain(&mut self, ctx: &mut Context, display: DisplayHandle) {
        ctx.displays[display].setup_swapchain(&ctx.device_ctx, &mut self.res_destroys);
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
            &mut self.res_destroys,
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

    pub fn graph_resources_destroy(&mut self, ctx: &mut Context, res: graph::ExecutionResources) {
        res.release(
            &mut self.res_destroys,
            &mut ctx.image_storage,
            &mut ctx.sampler_storage,
            &mut ctx.buffer_storage,
        );
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
            &mut self.res_destroys,
            &ctx.transfer,
            images,
        )
    }

    pub fn image_destroy(&mut self, ctx: &mut Context, images: &[image::ImageHandle]) {
        ctx.image_storage.destroy(&mut self.res_destroys, images)
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
            &mut self.res_destroys,
            &ctx.transfer,
            data,
        )
    }

    pub fn buffer_destroy(&mut self, ctx: &mut Context, buffers: &[buffer::BufferHandle]) {
        ctx.buffer_storage.destroy(&mut self.res_destroys, buffers);
    }

    pub fn sampler_destroy(&mut self, ctx: &mut Context, samplers: &[sampler::SamplerHandle]) {
        ctx.sampler_storage
            .destroy(&mut self.res_destroys, samplers)
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

            ctx.device_ctx.device.wait_for_fence(&fence, !0).unwrap();
        }

        self.sem_list.advance();

        ctx.device_ctx.device.destroy_fence(fence);

        self.res_destroys.free_resources();

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

pub struct ResourceList {
    device: Arc<DeviceContext>,

    framebuffers: SmallVec<[types::Framebuffer; 16]>,
    buffers: SmallVec<[BufferTypeInternal; 16]>,
    images: SmallVec<[ImageType; 16]>,
    samplers: SmallVec<[types::Sampler; 16]>,
    image_views: SmallVec<[types::ImageView; 16]>,
}

impl ResourceList {
    fn new(device: Arc<DeviceContext>) -> Self {
        ResourceList {
            device,
            framebuffers: SmallVec::new(),
            buffers: SmallVec::new(),
            images: SmallVec::new(),
            samplers: SmallVec::new(),
            image_views: SmallVec::new(),
        }
    }

    pub fn queue_framebuffer(&mut self, fb: types::Framebuffer) {
        self.framebuffers.push(fb);
    }

    pub fn queue_buffer(&mut self, buffer: BufferTypeInternal) {
        self.buffers.push(buffer);
    }

    pub fn queue_image(&mut self, image: ImageType) {
        self.images.push(image);
    }

    pub fn queue_sampler(&mut self, sampler: types::Sampler) {
        self.samplers.push(sampler);
    }

    pub fn queue_image_view(&mut self, image_view: types::ImageView) {
        self.image_views.push(image_view);
    }

    fn free_resources(&mut self) {
        use gfxm::Factory;
        use std::mem::replace;

        let mut alloc = self.device.allocator();

        let device = &self.device.device;

        let buffers = replace(&mut self.buffers, SmallVec::new());
        for buffer in buffers {
            alloc.destroy_buffer(device, buffer);
        }

        let images = replace(&mut self.images, SmallVec::new());
        for image in images {
            alloc.destroy_image(device, image);
        }

        let samplers = replace(&mut self.samplers, SmallVec::new());
        for sampler in samplers {
            device.destroy_sampler(sampler);
        }

        let image_views = replace(&mut self.image_views, SmallVec::new());
        for image_view in image_views {
            device.destroy_image_view(image_view);
        }

        let framebuffers = replace(&mut self.framebuffers, SmallVec::new());
        for framebuffer in framebuffers {
            device.destroy_framebuffer(framebuffer);
        }
    }
}
