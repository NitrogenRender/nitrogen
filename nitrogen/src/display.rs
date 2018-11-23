/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use gfx;
use gfx::Device;

use back;

use device::DeviceContext;
use image;
use sampler;

use types::*;

use std;

pub struct Display {
    pub surface: Surface,

    pub surface_size: (usize, usize),

    pub swapchain: Option<Swapchain>,
    pub display_format: gfx::format::Format,

    pub command_pool: gfx::CommandPool<back::Backend, gfx::General>,

    pub framebuffers: Vec<Framebuffer>,
    pub images: Vec<(Image, ImageView)>,

    pub display_renderpass: RenderPass,
    pub display_pipeline: GraphicsPipeline,
    pub display_pipeline_layout: PipelineLayout,
    pub display_desc_set: DescriptorSet,
    pub display_desc_set_layout: DescriptorSetLayout,
    pub display_desc_pool: DescriptorPool,

    pub clear_color: (f32, f32, f32, f32),
}

impl Display {
    /// Create a new `DisplayContext` which uses the provided surface.
    pub fn new(surface: Surface, device: &DeviceContext) -> Self {
        use gfx::pso;
        use gfx::DescriptorPool;
        use gfx::Surface;

        let command_pool = {
            let queue_group = device.queue_group();

            device
                .device
                .create_command_pool_typed(
                    &queue_group,
                    gfx::pool::CommandPoolCreateFlags::empty(),
                    1,
                ).expect("Can't create command pool")
        };

        let (_, formats, _) = surface.compatibility(&device.adapter.physical_device);

        let format = formats.unwrap().remove(0);

        let renderpass = {
            use gfx::pass;

            let attachment = pass::Attachment {
                format: Some(format),
                samples: 1,
                ops: pass::AttachmentOps {
                    load: pass::AttachmentLoadOp::Clear,
                    store: pass::AttachmentStoreOp::Store,
                },
                stencil_ops: pass::AttachmentOps::DONT_CARE,
                layouts: gfx::image::Layout::Undefined..gfx::image::Layout::Present,
            };

            let subpass = pass::SubpassDesc {
                colors: &[(0, gfx::image::Layout::ColorAttachmentOptimal)],
                depth_stencil: None,
                inputs: &[],
                resolves: &[],
                preserves: &[],
            };

            let dependency = pass::SubpassDependency {
                passes: pass::SubpassRef::External..pass::SubpassRef::Pass(0),
                stages: gfx::pso::PipelineStage::COLOR_ATTACHMENT_OUTPUT
                    ..gfx::pso::PipelineStage::COLOR_ATTACHMENT_OUTPUT,
                accesses: gfx::image::Access::empty()
                    ..(gfx::image::Access::COLOR_ATTACHMENT_READ
                        | gfx::image::Access::COLOR_ATTACHMENT_WRITE),
            };

            device
                .device
                .create_render_pass(&[attachment], &[subpass], &[dependency])
                .expect("Can't create renderpass")
        };

        let set_layout = device
            .device
            .create_descriptor_set_layout(
                &[
                    pso::DescriptorSetLayoutBinding {
                        binding: 0,
                        ty: pso::DescriptorType::SampledImage,
                        count: 1,
                        stage_flags: pso::ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    },
                    pso::DescriptorSetLayoutBinding {
                        binding: 1,
                        ty: pso::DescriptorType::Sampler,
                        count: 1,
                        stage_flags: pso::ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    },
                ],
                &[],
            ).expect("Can't create descriptor set layout");

        let mut set_pool = device
            .device
            .create_descriptor_pool(
                1,
                &[
                    pso::DescriptorRangeDesc {
                        ty: pso::DescriptorType::SampledImage,
                        count: 1,
                    },
                    pso::DescriptorRangeDesc {
                        ty: pso::DescriptorType::Sampler,
                        count: 1,
                    },
                ],
            ).expect("Can't create descriptor pool");

        let desc_set = set_pool.allocate_set(&set_layout).unwrap();

        let pipeline_layout = device
            .device
            .create_pipeline_layout(
                std::iter::once(&set_layout),
                &[], // TODO push constants
            ).expect("Can't create pipeline layout");

        let pipeline = {
            let vs_mod = {
                let binary = include_bytes!(concat!(env!("OUT_DIR"), "/present.hlsl.vert.spirv"));

                device.device.create_shader_module(binary).unwrap()
            };

            let fs_mod = {
                let binary = include_bytes!(concat!(env!("OUT_DIR"), "/present.hlsl.frag.spirv"));

                device.device.create_shader_module(binary).unwrap()
            };

            let pipe = {
                let (vs_entry, fs_entry) = (
                    pso::EntryPoint {
                        entry: "VertexMain",
                        module: &vs_mod,
                        specialization: pso::Specialization::default(),
                    },
                    pso::EntryPoint {
                        entry: "FragmentMain",
                        module: &fs_mod,
                        specialization: pso::Specialization::default(),
                    },
                );

                let shaders = pso::GraphicsShaderSet {
                    vertex: vs_entry,
                    hull: None,
                    domain: None,
                    geometry: None,
                    fragment: Some(fs_entry),
                };

                let subpass = gfx::pass::Subpass {
                    index: 0,
                    main_pass: &renderpass,
                };

                let desc = {
                    let mut desc = pso::GraphicsPipelineDesc::new(
                        shaders,
                        gfx::Primitive::TriangleList,
                        pso::Rasterizer::FILL,
                        &pipeline_layout,
                        subpass,
                    );

                    desc.blender.targets.push(pso::ColorBlendDesc(
                        pso::ColorMask::ALL,
                        pso::BlendState::ALPHA,
                    ));

                    desc
                };

                device.device.create_graphics_pipeline(&desc, None)
            };

            device.device.destroy_shader_module(vs_mod);
            device.device.destroy_shader_module(fs_mod);

            pipe.unwrap()
        };

        Display {
            surface,
            surface_size: (1, 1),
            command_pool,
            swapchain: None,
            display_format: format,
            framebuffers: vec![],
            images: vec![],
            display_pipeline: pipeline,
            display_desc_set: desc_set,
            display_desc_set_layout: set_layout,
            display_desc_pool: set_pool,
            display_pipeline_layout: pipeline_layout,
            display_renderpass: renderpass,
            clear_color: (0.0, 0.0, 0.0, 1.0),
        }
    }

    /// Setup the swapchain and associated framebuffers/images.
    ///
    /// Destroys the old swapchain, so the caller needs to make sure that it's no longer in use
    pub fn setup_swapchain(&mut self, device: &DeviceContext) {
        use gfx::Surface;

        {
            use std::mem::replace;
            let framebuffers = replace(&mut self.framebuffers, vec![]);
            let images = replace(&mut self.images, vec![]);

            for framebuffer in framebuffers {
                device.device.destroy_framebuffer(framebuffer);
            }

            for (_, image_view) in images {
                device.device.destroy_image_view(image_view);
            }

            // FIXME this is a workaround for an issue with gfx:
            // when you provide an old swapchain upon swapchain creation, it takes ownership of the
            // old one. The implementation doesn't actually free it, so the swapchain is leaked.
            if let Some(old_swapchain) = self.swapchain.take() {
                device.device.destroy_swapchain(old_swapchain);
            }
        }

        let (surface_capability, _, _) =
            self.surface.compatibility(&device.adapter.physical_device);

        let format = self.display_format;

        let mut config = gfx::SwapchainConfig::from_caps(&surface_capability, format);

        config.present_mode = gfx::PresentMode::Immediate;

        let extent = config.extent.to_extent();

        self.surface_size = (extent.width as _, extent.height as _);

        let old_swapchain = self.swapchain.take();

        let (swapchain, backbuffer) = device
            .device
            .create_swapchain(&mut self.surface, config, old_swapchain)
            .expect("Can't create swapchain");

        self.swapchain = Some(swapchain);

        // A swapchain might give us a list of images as a backbuffer or alternatively a single
        // framebuffer.
        // For each image an associated framebuffer is created.
        let (images, fbos) = match backbuffer {
            gfx::Backbuffer::Images(images) => {
                let pairs = images
                    .into_iter()
                    .map(|img| {
                        let view = device
                            .device
                            .create_image_view(
                                &img,
                                gfx::image::ViewKind::D2,
                                format,
                                gfx::format::Swizzle::NO,
                                gfx::image::SubresourceRange {
                                    aspects: gfx::format::Aspects::COLOR,
                                    levels: 0..1,
                                    layers: 0..1,
                                },
                            ).unwrap();
                        (img, view)
                    }).collect::<Vec<_>>();
                let fbos = pairs
                    .iter()
                    .map(|&(ref _image, ref view)| {
                        device
                            .device
                            .create_framebuffer(&self.display_renderpass, Some(view), extent)
                            .unwrap()
                    }).collect::<Vec<_>>();

                (pairs, fbos)
            }
            gfx::Backbuffer::Framebuffer(framebuffer) => (vec![], vec![framebuffer]),
        };

        self.framebuffers = fbos;
        self.images = images;
    }

    /// Present an image to the screen.
    ///
    /// The image has to be the same size as the swapchain images in order to preserve aspect ratio.
    pub fn present(
        &mut self,
        device: &DeviceContext,
        image_storage: &image::ImageStorage,
        image: image::ImageHandle,
        sampler_storage: &sampler::SamplerStorage,
        sampler: sampler::SamplerHandle,
    ) -> bool {
        use gfx::pso;
        use gfx::Swapchain;

        let image = {
            if let Some(raw) = image_storage.raw(image) {
                raw
            } else {
                return false;
            }
        };

        let sampler = {
            if let Some(raw) = sampler_storage.raw(sampler) {
                raw
            } else {
                return false;
            }
        };

        if let Some(ref mut swapchain) = self.swapchain {
            let mut swapchain_sem = device
                .device
                .create_semaphore()
                .expect("Can't create swapchain semaphore");

            let index =
                match swapchain.acquire_image(!0, gfx::FrameSync::Semaphore(&mut swapchain_sem)) {
                    Err(_) => return false,
                    Ok(image) => image,
                };

            let viewport = gfx::pso::Viewport {
                depth: 0.0..1.0,
                rect: gfx::pso::Rect {
                    x: 0,
                    y: 0,
                    w: self.surface_size.0 as _,
                    h: self.surface_size.1 as _,
                },
            };

            device.device.write_descriptor_sets(vec![
                pso::DescriptorSetWrite {
                    set: &self.display_desc_set,
                    binding: 0,
                    array_offset: 0,
                    descriptors: Some(pso::Descriptor::Image(
                        &image.view,
                        gfx::image::Layout::Undefined,
                    )),
                },
                pso::DescriptorSetWrite {
                    set: &self.display_desc_set,
                    binding: 1,
                    array_offset: 0,
                    descriptors: Some(pso::Descriptor::Sampler(sampler)),
                },
            ]);

            let submit = {
                let mut cmd = self.command_pool.acquire_command_buffer(false);

                cmd.set_viewports(0, &[viewport.clone()]);
                cmd.set_scissors(0, &[viewport.rect]);
                cmd.bind_graphics_pipeline(&self.display_pipeline);
                cmd.bind_graphics_descriptor_sets(
                    &self.display_pipeline_layout,
                    0,
                    std::iter::once(&self.display_desc_set),
                    &[],
                );

                {
                    let mut encoder = cmd.begin_render_pass_inline(
                        &self.display_renderpass,
                        &self.framebuffers[index as usize],
                        viewport.rect,
                        &[gfx::command::ClearValue::Color(
                            gfx::command::ClearColor::Float([
                                self.clear_color.0,
                                self.clear_color.1,
                                self.clear_color.2,
                                self.clear_color.3,
                            ]),
                        )],
                    );

                    encoder.draw(0..6, 0..1);
                }

                cmd.finish()
            };

            let mut submit_fence = device
                .device
                .create_fence(false)
                .expect("can't create submission fence");

            {
                let submission = gfx::Submission::new()
                    .wait_on(&[(&swapchain_sem, pso::PipelineStage::BOTTOM_OF_PIPE)])
                    .submit(Some(submit));
                device.queue_group().queues[0].submit(submission, Some(&mut submit_fence));
            }

            device.device.wait_for_fence(&submit_fence, !0);
            device.device.destroy_fence(submit_fence);
            device.device.destroy_semaphore(swapchain_sem);

            // without this call we are leaking memory. sigh...
            self.command_pool.reset();

            swapchain
                .present(&mut device.queue_group().queues[0], index, &[])
                .is_ok()
        } else {
            false
        }
    }

    /// Release the display context, destroys all associated graphics resources.
    pub fn release(mut self, device: &DeviceContext) {
        use gfx::DescriptorPool;

        self.display_desc_pool.reset();
        device
            .device
            .destroy_descriptor_pool(self.display_desc_pool);
        device
            .device
            .destroy_pipeline_layout(self.display_pipeline_layout);
        device
            .device
            .destroy_descriptor_set_layout(self.display_desc_set_layout);
        device
            .device
            .destroy_graphics_pipeline(self.display_pipeline);

        device
            .device
            .destroy_command_pool(self.command_pool.into_raw());

        for framebuffer in self.framebuffers {
            device.device.destroy_framebuffer(framebuffer);
        }

        for (_, image_view) in self.images {
            device.device.destroy_image_view(image_view);
        }

        if let Some(swapchain) = self.swapchain {
            device.device.destroy_swapchain(swapchain);
        }

        device.device.destroy_render_pass(self.display_renderpass);
    }
}
