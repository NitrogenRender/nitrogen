/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Synchronization management of GPU operations.

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

use std::collections::HashMap;
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
/// [`Context`]: ../../struct.Context.html
/// [`wait`]: #method.wait
/// [`release`]: #method.release
pub struct SubmitGroup {
    sem_pool: SemaphorePool,

    pool_graphics: CommandPoolGraphics,
    pool_compute: CommandPoolCompute,
    pool_transfer: CommandPoolTransfer,

    sem_list: SemaphoreList,
    res_destroys: ResourceList,

    graph_resources: HashMap<graph::GraphHandle, graph::GraphResources>,
}

impl SubmitGroup {
    pub(crate) unsafe fn new(device: Arc<DeviceContext>) -> Self {
        let (gfx, cmpt, trns) = {
            let gfx = device
                .device
                .create_command_pool_typed(
                    device.graphics_queue_group(),
                    gfx::pool::CommandPoolCreateFlags::empty(),
                )
                .unwrap();
            let cmpt = device
                .device
                .create_command_pool_typed(
                    device.compute_queue_group(),
                    gfx::pool::CommandPoolCreateFlags::empty(),
                )
                .unwrap();
            let trns = device
                .device
                .create_command_pool_typed(
                    device.transfer_queue_group(),
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

            graph_resources: HashMap::new(),
        }
    }

    /// Block the calling thread until all queued operations finished executing.
    pub unsafe fn wait(&mut self, ctx: &mut Context) {
        let fence = ctx.device_ctx.device.create_fence(false).unwrap();

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
                >, _, _, _, _>(submit, Some(&fence));

            ctx.device_ctx.device.wait_for_fence(&fence, !0).unwrap();
        }

        self.sem_list.advance();

        self.res_destroys.free_resources(ctx);

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
            &*ctx.image_storage.borrow(),
            image,
        )
    }

    /// Setup a new swapchain for a given `Display`, recreating it if an old one already exists.
    pub unsafe fn display_setup_swapchain(&mut self, ctx: &mut Context, display: DisplayHandle) {
        ctx.displays[display].setup_swapchain(&ctx.device_ctx, &mut self.res_destroys);
    }

    /// Queue the clearing of an `Image` with a given `ClearValue`.
    pub unsafe fn clear_image(
        &mut self,
        ctx: &mut Context,
        image: image::ImageHandle,
        clear: graph::ImageClearValue,
    ) -> Option<()> {
        use graph::ImageClearValue;

        let image_storage = ctx.image_storage.borrow();

        let img = image_storage.raw(image)?;

        let sem = self.sem_pool.alloc();

        self.sem_list.add_next_semaphore(sem);

        let mut cmd = self.pool_graphics.alloc();
        cmd.begin();

        let entry_barrier = gfx::memory::Barrier::Image {
            states: (gfx::image::Access::empty(), gfx::image::Layout::Undefined)
                ..(
                    gfx::image::Access::TRANSFER_WRITE,
                    gfx::image::Layout::TransferDstOptimal,
                ),
            target: img.image.raw(),
            families: None,
            range: gfx::image::SubresourceRange {
                aspects: img.aspect,
                levels: 0..1,
                layers: 0..1,
            },
        };

        cmd.pipeline_barrier(
            gfx::pso::PipelineStage::TOP_OF_PIPE..gfx::pso::PipelineStage::TRANSFER,
            gfx::memory::Dependencies::empty(),
            &[entry_barrier],
        );

        cmd.clear_image(
            img.image.raw(),
            gfx::image::Layout::TransferDstOptimal,
            match clear {
                ImageClearValue::Color(color) => gfx::command::ClearColor::Float(color),
                _ => gfx::command::ClearColor::Float([0.0; 4]),
            },
            match clear {
                ImageClearValue::DepthStencil(depth, stencil) => {
                    gfx::command::ClearDepthStencil(depth, stencil)
                }
                _ => gfx::command::ClearDepthStencil(1.0, 0),
            },
            &[gfx::image::SubresourceRange {
                aspects: img.aspect,
                levels: 0..1,
                layers: 0..1,
            }],
        );

        let exit_barrier = gfx::memory::Barrier::Image {
            states: (
                gfx::image::Access::TRANSFER_WRITE,
                gfx::image::Layout::TransferDstOptimal,
            )..(gfx::image::Access::empty(), gfx::image::Layout::General),
            target: img.image.raw(),
            families: None,
            range: gfx::image::SubresourceRange {
                aspects: img.aspect,
                levels: 0..1,
                layers: 0..1,
            },
        };

        cmd.pipeline_barrier(
            gfx::pso::PipelineStage::TRANSFER..gfx::pso::PipelineStage::BOTTOM_OF_PIPE,
            gfx::memory::Dependencies::empty(),
            &[exit_barrier],
        );

        cmd.finish();

        let mut queue = ctx.device_ctx.graphics_queue();

        {
            let submission = gfx::Submission {
                command_buffers: Some(&*cmd),
                wait_semaphores: self
                    .sem_pool
                    .list_prev_sems(&self.sem_list)
                    .map(|sem| (sem, gfx::pso::PipelineStage::BOTTOM_OF_PIPE)),
                signal_semaphores: self.sem_pool.list_next_sems(&self.sem_list),
            };

            queue.submit(submission, None);
        }

        self.sem_list.advance();

        Some(())
    }

    /// Queue the blitting of one image into another.
    pub unsafe fn blit_image(
        &mut self,
        ctx: &mut Context,
        dst: image::ImageHandle,
        src: image::ImageHandle,
        dst_region: std::ops::Range<gfx::image::Offset>,
        src_region: std::ops::Range<gfx::image::Offset>,
    ) -> Option<()> {
        let image_storage = ctx.image_storage.borrow();

        let dst = image_storage.raw(dst)?;
        let src = image_storage.raw(src)?;

        let blit = transfer::ImageBlit {
            dst: &dst.image,
            src: &src.image,
            filter: gfx::image::Filter::Nearest,
            subresource_range: gfx::image::SubresourceRange {
                aspects: dst.aspect,
                levels: 0..1,
                layers: 0..1,
            },
            // TODO layers? mipmaps?
            copy_information: gfx::command::ImageBlit {
                src_subresource: gfx::image::SubresourceLayers {
                    aspects: src.aspect,
                    level: 0,
                    layers: 0..1,
                },
                src_bounds: src_region,
                dst_subresource: gfx::image::SubresourceLayers {
                    aspects: dst.aspect,
                    level: 0,
                    layers: 0..1,
                },
                dst_bounds: dst_region,
            },
        };

        transfer::blit_image(
            &ctx.device_ctx,
            &self.sem_pool,
            &mut self.sem_list,
            &self.pool_graphics,
            &[blit],
        );

        Some(())
    }

    /// Queue the execution of a graph.
    pub unsafe fn graph_execute(
        &mut self,
        ctx: &mut Context,
        backbuffer: &mut graph::Backbuffer,
        graph: graph::GraphHandle,
        store: &mut graph::Store,
        exec_context: &graph::ExecutionContext,
    ) -> Result<(), graph::GraphExecError> {
        let mut storages = graph::Storages {
            shader: &ctx.shader_storage,
            render_pass: &mut ctx.render_pass_storage,
            pipeline: &mut ctx.pipeline_storage,
            image: &mut ctx.image_storage,
            buffer: &mut ctx.buffer_storage,
            vertex_attrib: &ctx.vertex_attrib_storage,
            sampler: &mut ctx.sampler_storage,
            material: &mut ctx.material_storage,
        };

        let res = self.graph_resources.entry(graph).or_default();

        let mut sync = QueueSyncRefs {
            sem_pool: &self.sem_pool,
            sem_list: &mut self.sem_list,
            res_list: &mut self.res_destroys,
        };

        if let Err(err) = ctx.graph_storage.borrow_mut().execute(
            &ctx.device_ctx,
            &mut sync,
            &mut storages,
            (&self.pool_graphics, &self.pool_compute),
            store,
            graph,
            res,
            backbuffer,
            exec_context,
        ) {
            /*
            if let Some(res) = self.graph_resources.remove(&graph) {
                res.release(&mut self.res_destroys, &mut storages);
            }
            */

            return Err(err)?;
        }

        Ok(())
    }

    /// Queue the deletion of a graph and all its associated resources.
    pub fn graph_destroy<G>(&mut self, ctx: &mut Context, graph: G)
    where
        G: IntoIterator,
        G::Item: std::borrow::Borrow<graph::GraphHandle>,
    {
        use std::borrow::Borrow;

        let mut storages = graph::Storages {
            shader: &ctx.shader_storage,
            render_pass: &mut ctx.render_pass_storage,
            pipeline: &mut ctx.pipeline_storage,
            image: &mut ctx.image_storage,
            buffer: &mut ctx.buffer_storage,
            vertex_attrib: &ctx.vertex_attrib_storage,
            sampler: &mut ctx.sampler_storage,
            material: &mut ctx.material_storage,
        };

        for handle in graph.into_iter() {
            let handle = *handle.borrow();

            if let Some(res) = self.graph_resources.remove(&handle) {
                res.release(&mut self.res_destroys, &mut storages);
            }

            ctx.graph_storage
                .borrow_mut()
                .destroy(&mut self.res_destroys, &mut storages, handle);
        }
    }

    /// Retrieve an `ImageHandle` from a named graph resource.
    pub fn graph_get_image<I: Into<graph::ResourceName>>(
        &self,
        ctx: &Context,
        graph: graph::GraphHandle,
        image: I,
    ) -> Option<image::ImageHandle> {
        None
    }

    /// Queue the deletion of a [`Backbuffer`] object and all its associated resources.
    ///
    /// [`Backbuffer`]: ../../graph/struct.Backbuffer.html
    pub fn backbuffer_destroy(&mut self, ctx: &mut Context, backbuffer: graph::Backbuffer) {
        ctx.image_storage
            .borrow_mut()
            .destroy(&mut self.res_destroys, backbuffer.images.values())
    }

    /// Queue the upload of data to an [`Image`] object.
    ///
    /// [`Image`]: ../../resources/image/struct.Image.html
    pub unsafe fn image_upload_data(
        &mut self,
        ctx: &mut Context,
        image: image::ImageHandle,
        data: image::ImageUploadInfo,
    ) -> Result<(), image::ImageError> {
        ctx.image_storage.borrow_mut().upload_data(
            &ctx.device_ctx,
            &self.sem_pool,
            &mut self.sem_list,
            &self.pool_transfer,
            &mut self.res_destroys,
            image,
            data,
        )
    }

    /// Queue the deletion of an [`Image`] object.
    ///
    /// [`Image`]: ../../resources/image/struct.Image.html
    pub fn image_destroy(&mut self, ctx: &mut Context, images: &[image::ImageHandle]) {
        ctx.image_storage
            .borrow_mut()
            .destroy(&mut self.res_destroys, images)
    }

    /// Queue the upload of data to a [cpu-visible] buffer object.
    ///
    /// [cpu-visible]: ../../resources/buffer/struct.CpuVisibleCreateInfo.html
    pub unsafe fn buffer_cpu_visible_upload<T>(
        &mut self,
        ctx: &mut Context,
        buffer: buffer::BufferHandle,
        info: buffer::BufferUploadInfo<T>,
    ) -> Result<(), buffer::BufferError> {
        ctx.buffer_storage
            .borrow()
            .cpu_visible_upload(&ctx.device_ctx, buffer, info)
    }

    /// Queue the reading of data from a [cpu-visible] buffer object.
    ///
    /// [cpu-visible]: ../../resources/buffer/struct.CpuVisibleCreateInfo.html
    pub unsafe fn buffer_cpu_visible_read<T>(
        &mut self,
        ctx: &Context,
        buffer: buffer::BufferHandle,
        data: &mut [T],
    ) {
        ctx.buffer_storage
            .borrow()
            .cpu_visible_read(&ctx.device_ctx, buffer, data);
    }

    /// Queue the upload of data to a [device-local] buffer object.
    ///
    /// [device-local]: ../../resources/buffer/struct.DeviceLocalCreateInfo.html
    pub unsafe fn buffer_device_local_upload<T>(
        &mut self,
        ctx: &mut Context,
        buffer: buffer::BufferHandle,
        info: buffer::BufferUploadInfo<T>,
    ) -> Result<(), buffer::BufferError> {
        ctx.buffer_storage.borrow().device_local_upload(
            &ctx.device_ctx,
            &self.sem_pool,
            &mut self.sem_list,
            &self.pool_transfer,
            &mut self.res_destroys,
            buffer,
            info,
        )
    }

    /// Queue the deltion of a [`Buffer`] object.
    ///
    /// [`Buffer`]: ../../resources/buffer/struct.Buffer.html
    pub fn buffer_destroy(&mut self, ctx: &mut Context, buffers: &[buffer::BufferHandle]) {
        ctx.buffer_storage
            .borrow_mut()
            .destroy(&mut self.res_destroys, buffers);
    }

    /// Queue the deletion of a [`Sampler`] object.
    ///
    /// [`Sampler`]: ../../resources/sampler/index.html
    pub fn sampler_destroy(&mut self, ctx: &mut Context, samplers: &[sampler::SamplerHandle]) {
        ctx.sampler_storage
            .borrow_mut()
            .destroy(&mut self.res_destroys, samplers)
    }

    /// Queue the deletion of a [`Material`].
    ///
    /// [`Material`]: ../../resources/material/struct.Material.html
    pub fn material_destroy(&mut self, materials: &[material::MaterialHandle]) {
        for m in materials {
            self.res_destroys.queue_material(*m);
        }
    }

    /// Queue the deletion of a [`MaterialInstance`].
    ///
    /// [`MaterialInstance`]: ../../resources/material/struct.MaterialInstance.html
    pub fn material_instance_destroy(&mut self, instances: &[material::MaterialInstanceHandle]) {
        for i in instances {
            self.res_destroys.queue_material_instance(*i);
        }
    }

    /// Release the `SubmitGroup` and all associated resources.
    pub unsafe fn release(mut self, ctx: &mut Context) {
        let mut storages = graph::Storages {
            shader: &ctx.shader_storage,
            render_pass: &mut ctx.render_pass_storage,
            pipeline: &mut ctx.pipeline_storage,
            image: &mut ctx.image_storage,
            buffer: &mut ctx.buffer_storage,
            vertex_attrib: &ctx.vertex_attrib_storage,
            sampler: &mut ctx.sampler_storage,
            material: &mut ctx.material_storage,
        };

        for (_, graph_res) in self.graph_resources.drain() {
            graph_res.release(&mut self.res_destroys, &mut storages);
        }

        self.wait(ctx);
        ctx.wait_idle();

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

pub(crate) struct QueueSyncRefs<'a> {
    pub(crate) sem_pool: &'a SemaphorePool,
    pub(crate) sem_list: &'a mut SemaphoreList,
    pub(crate) res_list: &'a mut ResourceList,
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

    materials: SmallVec<[material::MaterialHandle; 16]>,
    material_instances: SmallVec<[material::MaterialInstanceHandle; 16]>,
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

            materials: SmallVec::new(),
            material_instances: SmallVec::new(),
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

    pub(crate) fn queue_material(&mut self, mat: material::MaterialHandle) {
        self.materials.push(mat);
    }

    pub(crate) fn queue_material_instance(&mut self, mat: material::MaterialInstanceHandle) {
        self.material_instances.push(mat);
    }

    unsafe fn free_resources(&mut self, ctx: &mut Context) {
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

        {
            ctx.material_storage
                .borrow_mut()
                .destroy(&ctx.device_ctx, self.materials.as_slice());
            self.materials.clear();
        }

        {
            ctx.material_storage
                .borrow_mut()
                .destroy_instances(self.material_instances.as_slice());
            self.material_instances.clear();
        }
    }
}
