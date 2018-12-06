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

use resources::semaphore_pool::{SemaphoreList, SemaphorePool};

use std;
use std::sync::Arc;
use submit_group::ResourceList;

pub struct Display {
    pub surface: Surface,

    pub surface_size: (usize, usize),

    pub swapchain: Option<Swapchain>,
    pub display_format: gfx::format::Format,

    pub images: Vec<(Image, ImageView)>,
}

impl Display {
    /// Create a new `DisplayContext` which uses the provided surface.
    pub fn new(surface: Surface, device: &DeviceContext) -> Self {
        use gfx::pso;
        use gfx::DescriptorPool;
        use gfx::Surface;

        let (_, formats, _) = surface.compatibility(&device.adapter.physical_device);

        let format = formats.unwrap().remove(0);

        Display {
            surface,
            surface_size: (1, 1),
            swapchain: None,
            display_format: format,
            images: vec![],
        }
    }

    /// Setup the swapchain and associated framebuffers/images.
    ///
    /// Destroys the old swapchain, so the caller needs to make sure that it's no longer in use
    pub fn setup_swapchain(&mut self, device: &DeviceContext, res_list: &mut ResourceList) {
        use gfx::Surface;

        {
            use std::mem::replace;
            let images = replace(&mut self.images, vec![]);

            for (_, image_view) in images {
                res_list.queue_image_view(image_view);
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
        config.image_usage |= gfx::image::Usage::TRANSFER_DST;
        config.image_layers = 1;

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
        // Since we directly blit, we don't need any framebuffers. This also means we ignore the
        // case where a framebuffer is handed to us.
        let images = match backbuffer {
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
                            )
                            .unwrap();
                        (img, view)
                    })
                    .collect::<Vec<_>>();
                pairs
            }
            gfx::Backbuffer::Framebuffer(_framebuffer) => unimplemented!(),
        };

        self.images = images;
    }

    /// Present an image to the screen.
    ///
    /// The image has to be the same size as the swapchain images in order to preserve aspect ratio.
    pub(crate) fn present<'a>(
        &'a mut self,
        device: &DeviceContext,
        sem_pool: &mut SemaphorePool,
        sem_list: &mut SemaphoreList,
        command_pool: &mut CommandPool<gfx::Graphics>,
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
            let mut sem_acquire = sem_pool.alloc();

            let index = match swapchain.acquire_image(!0, gfx::FrameSync::Semaphore(&*sem_acquire))
            {
                Err(_) => return false,
                Ok(image) => image,
            };

            sem_list.add_prev_semaphore(sem_acquire);

            let submit = {
                let mut cmd = command_pool.acquire_command_buffer(false);

                let src_image = image.image.raw();
                let dst_image = &self.images[index as usize].0;

                let subres_range = gfx::image::SubresourceRange {
                    aspects: gfx::format::Aspects::COLOR,
                    levels: 0..1,
                    layers: 0..1,
                };

                // entry barrier
                {
                    use gfx::image::Access;
                    use gfx::image::Layout;
                    use gfx::pso::PipelineStage;

                    let src_barrier = gfx::memory::Barrier::Image {
                        states: (Access::empty(), Layout::Undefined)
                            ..(Access::TRANSFER_WRITE, Layout::TransferSrcOptimal),
                        target: src_image,
                        range: subres_range.clone(),
                    };
                    let dst_barrier = gfx::memory::Barrier::Image {
                        states: (Access::empty(), Layout::Undefined)
                            ..(Access::TRANSFER_WRITE, Layout::TransferDstOptimal),
                        target: dst_image,
                        range: subres_range.clone(),
                    };

                    cmd.pipeline_barrier(
                        PipelineStage::TOP_OF_PIPE..PipelineStage::TRANSFER,
                        gfx::memory::Dependencies::empty(),
                        &[src_barrier, dst_barrier],
                    );
                }

                // perform blit
                {
                    let src_layout = gfx::image::Layout::TransferSrcOptimal;

                    let dst_layout = gfx::image::Layout::TransferDstOptimal;

                    let filter = gfx::image::Filter::Linear;

                    let subresource = gfx::image::SubresourceLayers {
                        aspects: gfx::format::Aspects::COLOR,
                        level: 0,
                        layers: 0..1,
                    };

                    let origin_bound = gfx::image::Offset { x: 0, y: 0, z: 0 };
                    let src_bounds = {
                        let (x, y, z) = image.dimension.as_triple(1);
                        gfx::image::Offset {
                            x: x as _,
                            y: y as _,
                            z: z as _,
                        }
                    };
                    let dst_bounds = gfx::image::Offset {
                        x: self.surface_size.0 as _,
                        y: self.surface_size.1 as _,
                        z: 1,
                    };

                    cmd.blit_image(
                        src_image,
                        src_layout,
                        dst_image,
                        dst_layout,
                        filter,
                        &[gfx::command::ImageBlit {
                            src_subresource: subresource.clone(),
                            src_bounds: origin_bound..src_bounds,
                            dst_subresource: subresource,
                            dst_bounds: origin_bound..dst_bounds,
                        }],
                    );
                }

                // exit barrier
                {
                    use gfx::image::Access;
                    use gfx::image::Layout;
                    use gfx::pso::PipelineStage;

                    let src_barrier = gfx::memory::Barrier::Image {
                        states: (Access::TRANSFER_WRITE, Layout::TransferSrcOptimal)
                            ..(Access::empty(), Layout::General),
                        target: src_image,
                        range: subres_range.clone(),
                    };
                    let dst_barrier = gfx::memory::Barrier::Image {
                        states: (Access::TRANSFER_WRITE, Layout::TransferDstOptimal)
                            ..(Access::empty(), Layout::Present),
                        target: dst_image,
                        range: subres_range.clone(),
                    };

                    cmd.pipeline_barrier(
                        PipelineStage::TRANSFER..PipelineStage::BOTTOM_OF_PIPE,
                        gfx::memory::Dependencies::empty(),
                        &[src_barrier, dst_barrier],
                    );
                }

                cmd.finish()
            };

            let sem_blit = sem_pool.alloc();
            let sem_present = sem_pool.alloc();

            sem_list.add_next_semaphore(sem_blit);

            let mut queue = device.graphics_queue();

            {
                let submission = gfx::Submission::new()
                    .wait_on(
                        sem_pool
                            .list_prev_sems(sem_list)
                            .map(|sem| (sem, pso::PipelineStage::BOTTOM_OF_PIPE)),
                    )
                    .signal(&[&*sem_present])
                    .signal(sem_pool.list_next_sems(sem_list))
                    .submit(Some(submit));
                queue.submit(submission, None);
            }

            let res = swapchain
                .present(&mut *queue, index, std::iter::once(sem_present))
                .is_ok();

            sem_list.advance();

            res
        } else {
            false
        }
    }

    /// Release the display context, destroys all associated graphics resources.
    pub fn release(mut self, device: &DeviceContext) {
        use gfx::DescriptorPool;

        for (_, image_view) in self.images {
            device.device.destroy_image_view(image_view);
        }

        if let Some(swapchain) = self.swapchain {
            device.device.destroy_swapchain(swapchain);
        }
    }
}
