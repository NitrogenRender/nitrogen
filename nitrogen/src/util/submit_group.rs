/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx::Device;

use crate::buffer::BufferTypeInternal;
use crate::image::ImageType;

use crate::device::DeviceContext;
use crate::*;

use crate::resources::semaphore_pool::{SemaphoreList, SemaphorePool};

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
        graph: graph::GraphHandle,
    ) -> bool {
        if let Some(graph) = ctx.graph_storage.storage.get(graph) {
            if let Some((_, res)) = graph.exec_resources.as_ref() {
                let mut image = None;
                for output in &res.outputs {
                    if let Some(id) = res.images.get(output) {
                        image = Some(*id);
                        break;
                    }
                }

                if image == None {
                    return false;
                }

                let image = image.unwrap();

                ctx.displays[display].present(
                    &ctx.device_ctx,
                    &mut self.sem_pool,
                    &mut self.sem_list,
                    &mut self.pool_graphics,
                    &ctx.image_storage,
                    image,
                );

                return true;
            }
        }

        false
    }

    pub fn display_setup_swapchain(&mut self, ctx: &mut Context, display: DisplayHandle) {
        ctx.displays[display].setup_swapchain(&ctx.device_ctx, &mut self.res_destroys);
    }

    pub fn graph_execute(
        &mut self,
        ctx: &mut Context,
        graph: graph::GraphHandle,
        exec_context: &graph::ExecutionContext,
    ) {
        let mut storages = graph::Storages {
            render_pass: &mut ctx.render_pass_storage,
            pipeline: &mut ctx.pipeline_storage,
            image: &mut ctx.image_storage,
            buffer: &mut ctx.buffer_storage,
            vertex_attrib: &ctx.vertex_attrib_storage,
            sampler: &mut ctx.sampler_storage,
            material: &ctx.material_storage,
        };

        ctx.graph_storage.execute(
            &ctx.device_ctx,
            &mut self.sem_pool,
            &mut self.sem_list,
            &mut self.pool_graphics,
            &mut self.pool_compute,
            &mut self.res_destroys,
            &mut storages,
            graph,
            exec_context,
        )
    }

    pub fn graph_destroy<G>(&mut self, ctx: &mut Context, graph: G)
    where
        G: IntoIterator,
        G::Item: std::borrow::Borrow<graph::GraphHandle>,
    {
        use std::borrow::Borrow;

        let mut storages = graph::Storages {
            render_pass: &mut ctx.render_pass_storage,
            pipeline: &mut ctx.pipeline_storage,
            image: &mut ctx.image_storage,
            buffer: &mut ctx.buffer_storage,
            vertex_attrib: &ctx.vertex_attrib_storage,
            sampler: &mut ctx.sampler_storage,
            material: &ctx.material_storage,
        };

        for handle in graph.into_iter() {
            let handle = *handle.borrow();

            ctx.graph_storage
                .destroy(&mut self.res_destroys, &mut storages, handle);
        }
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

    pub fn buffer_read_data<T>(
        &mut self,
        ctx: &Context,
        buffer: buffer::BufferHandle,
        data: &mut [T],
    ) {
        ctx.buffer_storage.read_data(
            &ctx.device_ctx,
            &self.sem_pool,
            &mut self.sem_list,
            &mut self.pool_transfer,
            &mut self.res_destroys,
            &ctx.transfer,
            buffer,
            data,
        );
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
    render_passes: SmallVec<[types::RenderPass; 16]>,
    pipelines_graphic: SmallVec<[types::GraphicsPipeline; 16]>,
    pipelines_compute: SmallVec<[types::ComputePipeline; 16]>,
    pipelines_layout: SmallVec<[types::PipelineLayout; 16]>,
    desc_set_layouts: SmallVec<[types::DescriptorSetLayout; 16]>,
    desc_pools: SmallVec<[types::DescriptorPool; 16]>,
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
            render_passes: SmallVec::new(),
            pipelines_graphic: SmallVec::new(),
            pipelines_compute: SmallVec::new(),
            pipelines_layout: SmallVec::new(),
            desc_set_layouts: SmallVec::new(),
            desc_pools: SmallVec::new(),
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

    pub fn queue_render_pass(&mut self, render_pass: types::RenderPass) {
        self.render_passes.push(render_pass);
    }

    pub fn queue_pipeline_graphic(&mut self, pipe: types::GraphicsPipeline) {
        self.pipelines_graphic.push(pipe);
    }

    pub fn queue_pipeline_compute(&mut self, pipe: types::ComputePipeline) {
        self.pipelines_compute.push(pipe);
    }

    pub fn queue_pipeline_layout(&mut self, layout: types::PipelineLayout) {
        self.pipelines_layout.push(layout);
    }

    pub fn queue_desc_set_layout(&mut self, layout: types::DescriptorSetLayout) {
        self.desc_set_layouts.push(layout);
    }

    pub fn queue_desc_pool(&mut self, pool: types::DescriptorPool) {
        self.desc_pools.push(pool);
    }

    fn free_resources(&mut self) {
        use gfxm::Factory;

        let mut alloc = self.device.allocator();

        let device = &self.device.device;

        for buffer in self.buffers.drain() {
            alloc.destroy_buffer(device, buffer);
        }

        for image in self.images.drain() {
            alloc.destroy_image(device, image);
        }

        for sampler in self.samplers.drain() {
            device.destroy_sampler(sampler);
        }

        for image_view in self.image_views.drain() {
            device.destroy_image_view(image_view);
        }

        for framebuffer in self.framebuffers.drain() {
            device.destroy_framebuffer(framebuffer);
        }

        for render_pass in self.render_passes.drain() {
            device.destroy_render_pass(render_pass);
        }

        for pipe in self.pipelines_graphic.drain() {
            device.destroy_graphics_pipeline(pipe);
        }

        for pipe in self.pipelines_compute.drain() {
            device.destroy_compute_pipeline(pipe);
        }

        for layout in self.pipelines_layout.drain() {
            device.destroy_pipeline_layout(layout);
        }

        for desc_pool in self.desc_pools.drain() {
            // implicitly resets and frees all sets
            device.destroy_descriptor_pool(desc_pool);
        }

        for desc_layout in self.desc_set_layouts.drain() {
            device.destroy_descriptor_set_layout(desc_layout);
        }
    }
}
