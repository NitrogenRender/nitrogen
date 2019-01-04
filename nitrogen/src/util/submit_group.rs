/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx::Device;

use crate::buffer::BufferTypeInternal;
use crate::image::ImageType;

use crate::device::DeviceContext;
use crate::*;

use crate::resources::command_pool::{
    CommandPoolCompute, CommandPoolGraphics, CommandPoolTransfer,
};
use crate::resources::semaphore_pool::{SemaphoreList, SemaphorePool};

use smallvec::SmallVec;

use std::sync::Arc;

/// `SubmitGroup`s are used to synchronize access to resources and ensure
/// that draw calls and dispatches to the device don't cause race conditions.
///
/// To acquire a `SubmitGroup`, the [`Context::create_submit_group`] method has to be used.
///
/// All commands on a `SubmitGroup` require a mutable [`Context`] reference, so does **freeing** the
/// object. Dropping a `SubmitGroup` will most likely result in panics later on, instead the
/// [`release`] method has to be used.
///
/// After recording a number of commands using a SubmitGroup, the [`wait`] function can be
/// called to block the caller-thread until the operations finished executing.
///
/// [`Context::create_submit_group`]: ../../struct.Context.html#method.create_submit_group
/// [`wait`]: #method.wait
/// [`release`]: #method.release
pub struct SubmitGroup {
    sem_pool: SemaphorePool,

    pool_graphics: CommandPoolGraphics,
    pool_compute: CommandPoolCompute,
    pool_transfer: CommandPoolTransfer,

    sem_list: SemaphoreList,
    res_destroys: ResourceList,
}

impl SubmitGroup {
    pub(crate) unsafe fn new(device: Arc<DeviceContext>) -> Self {
        let (gfx, cmpt, trns) = {
            let gfx = device
                .device
                .create_command_pool_typed(
                    device.graphics_queue_group(),
                    // gfx::pool::CommandPoolCreateFlags::RESET_INDIVIDUAL,
                    gfx::pool::CommandPoolCreateFlags::empty(),
                )
                .unwrap();
            let cmpt = device
                .device
                .create_command_pool_typed(
                    device.compute_queue_group(),
                    // gfx::pool::CommandPoolCreateFlags::RESET_INDIVIDUAL,
                    gfx::pool::CommandPoolCreateFlags::empty(),
                )
                .unwrap();
            let trns = device
                .device
                .create_command_pool_typed(
                    device.transfer_queue_group(),
                    // gfx::pool::CommandPoolCreateFlags::RESET_INDIVIDUAL,
                    gfx::pool::CommandPoolCreateFlags::empty(),
                )
                .unwrap();

            (
                CommandPoolGraphics::new(gfx),
                CommandPoolCompute::new(cmpt),
                CommandPoolTransfer::new(trns),
            )
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

    pub unsafe fn wait(&mut self, ctx: &mut Context) {
        let mut fence = ctx.device_ctx.device.create_fence(false).unwrap();

        {
            let submit = gfx::Submission {
                command_buffers: None,
                wait_semaphores: self
                    .sem_pool
                    .list_prev_sems(&self.sem_list)
                    .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
                signal_semaphores: self.sem_pool.list_next_sems(&self.sem_list),
            };

            ctx.device_ctx
                .transfer_queue()
                .submit::<gfx::command::CommandBuffer<
                    back::Backend,
                    gfx::Transfer,
                    gfx::command::OneShot,
                    gfx::command::Primary,
                >, _, _, _, _>(submit, Some(&mut fence));

            ctx.device_ctx.device.wait_for_fence(&fence, !0).unwrap();
        }

        self.sem_list.advance();

        self.res_destroys.free_resources();

        self.pool_graphics.reset();
        self.pool_compute.reset();
        self.pool_transfer.reset();

        ctx.device_ctx.device.destroy_fence(fence);

        self.sem_pool.clear();
    }

    /// Present an image to a display.
    ///
    /// If the image to be presented is a result of a graph execution, use
    /// [`Context::graph_get_output_image`] to retrieve the `ImageHandle`.
    ///
    /// [`Context::graph_get_output_image`]: ../../struct.Context.html#method.graph_get_output_image
    pub unsafe fn display_present(
        &mut self,
        ctx: &mut Context,
        display: DisplayHandle,
        image: image::ImageHandle,
    ) -> bool {
        ctx.displays[display].present(
            &ctx.device_ctx,
            &mut self.sem_pool,
            &mut self.sem_list,
            &self.pool_graphics,
            &ctx.image_storage,
            image,
        )
    }

    pub unsafe fn display_setup_swapchain(&mut self, ctx: &mut Context, display: DisplayHandle) {
        ctx.displays[display].setup_swapchain(&ctx.device_ctx, &mut self.res_destroys);
    }

    pub unsafe fn graph_execute(
        &mut self,
        ctx: &mut Context,
        graph: graph::GraphHandle,
        store: &graph::Store,
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
            &self.pool_graphics,
            &self.pool_compute,
            &mut self.res_destroys,
            &mut storages,
            store,
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

    pub unsafe fn image_upload_data(
        &mut self,
        ctx: &mut Context,
        images: &[(image::ImageHandle, image::ImageUploadInfo)],
    ) -> SmallVec<[image::Result<()>; 16]> {
        ctx.image_storage.upload_data(
            &ctx.device_ctx,
            &self.sem_pool,
            &mut self.sem_list,
            &self.pool_transfer,
            &mut self.res_destroys,
            images,
        )
    }

    pub fn image_destroy(&mut self, ctx: &mut Context, images: &[image::ImageHandle]) {
        ctx.image_storage.destroy(&mut self.res_destroys, images)
    }

    pub unsafe fn buffer_cpu_visible_upload<T>(
        &mut self,
        ctx: &mut Context,
        data: &[(buffer::BufferHandle, buffer::BufferUploadInfo<T>)],
    ) -> SmallVec<[buffer::Result<()>; 16]> {
        ctx.buffer_storage.cpu_visible_upload(&ctx.device_ctx, data)
    }

    pub unsafe fn buffer_cpu_visible_read<T>(
        &mut self,
        ctx: &Context,
        buffer: buffer::BufferHandle,
        data: &mut [T],
    ) {
        ctx.buffer_storage
            .cpu_visible_read(&ctx.device_ctx, buffer, data);
    }

    pub unsafe fn buffer_device_local_upload<T>(
        &mut self,
        ctx: &mut Context,
        data: &[(buffer::BufferHandle, buffer::BufferUploadInfo<T>)],
    ) -> SmallVec<[buffer::Result<()>; 16]> {
        ctx.buffer_storage.device_local_upload(
            &ctx.device_ctx,
            &self.sem_pool,
            &mut self.sem_list,
            &self.pool_transfer,
            &mut self.res_destroys,
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

    pub unsafe fn release(mut self, ctx: &mut Context) {
        {
            let pool = self.pool_graphics.0.into_impl();
            ctx.device_ctx
                .device
                .destroy_command_pool(pool.pool.into_raw());
        }
        {
            let pool = self.pool_compute.0.into_impl();
            ctx.device_ctx
                .device
                .destroy_command_pool(pool.pool.into_raw());
        }
        {
            let pool = self.pool_transfer.0.into_impl();
            ctx.device_ctx
                .device
                .destroy_command_pool(pool.pool.into_raw());
        }

        self.sem_pool.reset();
    }
}

pub(crate) struct ResourceList {
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

    pub(crate) fn queue_framebuffer(&mut self, fb: types::Framebuffer) {
        self.framebuffers.push(fb);
    }

    pub(crate) fn queue_buffer(&mut self, buffer: BufferTypeInternal) {
        self.buffers.push(buffer);
    }

    pub(crate) fn queue_image(&mut self, image: ImageType) {
        self.images.push(image);
    }

    pub(crate) fn queue_sampler(&mut self, sampler: types::Sampler) {
        self.samplers.push(sampler);
    }

    pub(crate) fn queue_image_view(&mut self, image_view: types::ImageView) {
        self.image_views.push(image_view);
    }

    pub(crate) fn queue_render_pass(&mut self, render_pass: types::RenderPass) {
        self.render_passes.push(render_pass);
    }

    pub(crate) fn queue_pipeline_graphic(&mut self, pipe: types::GraphicsPipeline) {
        self.pipelines_graphic.push(pipe);
    }

    pub(crate) fn queue_pipeline_compute(&mut self, pipe: types::ComputePipeline) {
        self.pipelines_compute.push(pipe);
    }

    pub(crate) fn queue_pipeline_layout(&mut self, layout: types::PipelineLayout) {
        self.pipelines_layout.push(layout);
    }

    pub(crate) fn queue_desc_set_layout(&mut self, layout: types::DescriptorSetLayout) {
        self.desc_set_layouts.push(layout);
    }

    pub(crate) fn queue_desc_pool(&mut self, pool: types::DescriptorPool) {
        self.desc_pools.push(pool);
    }

    unsafe fn free_resources(&mut self) {
        use crate::util::allocator::Allocator;

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
