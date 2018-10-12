extern crate gfx_backend_vulkan as back;
extern crate gfx_hal as gfx;
extern crate gfx_memory as gfxm;

extern crate failure;
#[macro_use]
extern crate failure_derive;

extern crate ash;

extern crate slab;

pub mod util;
pub use util::storage;

pub mod resources;
pub use resources::image;
pub use resources::sampler;

use gfx::Device;
use gfx::Instance;
use gfx::PhysicalDevice;

use gfxm::MemoryAllocator;
use gfxm::SmartAllocator;

use ash::vk;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;


type Swapchain = <back::Backend as gfx::Backend>::Swapchain;
type Surface = <back::Backend as gfx::Backend>::Surface;
type Framebuffer = <back::Backend as gfx::Backend>::Framebuffer;
type RenderPass = <back::Backend as gfx::Backend>::RenderPass;




pub struct CreationInfoX11 {
    pub name: String,
    pub version: u32,
    pub display: *mut vk::Display,
    pub window: vk::Window,
}

#[repr(u8)]
pub enum QueueType {
    Rendering,
    ImageStorage,
}

pub struct DeviceContext {
    pub memory_allocator: Mutex<SmartAllocator<back::Backend>>,

    pub queue_group: Mutex<gfx::QueueGroup<back::Backend, gfx::General>>,

    pub device: Arc<back::Device>,
    pub adapter: Arc<gfx::Adapter<back::Backend>>,
}

impl DeviceContext {
    pub fn new(instance: &back::Instance, surface: &impl gfx::Surface<back::Backend>) -> Self {
        let mut adapters = instance.enumerate_adapters();

        // TODO select best fitting adapter
        let mut adapter = adapters.remove(0);

        let memory_properties = adapter.physical_device.memory_properties();
        let memory_allocator =
            SmartAllocator::new(memory_properties, 256, 64, 1024, 256 * 1024 * 1024);

        let (device, queue_group) = adapter
            .open_with(2, |family| surface.supports_queue_family(family))
            .unwrap();

        DeviceContext {
            memory_allocator: Mutex::new(memory_allocator),

            queue_group: Mutex::new(queue_group),

            device: Arc::new(device),
            adapter: Arc::new(adapter),
        }
    }

    pub fn allocator(&self) -> MutexGuard<SmartAllocator<back::Backend>> {
        // if we can't access the device-local memory allocator then ... well, RIP
        self.memory_allocator
            .lock()
            .expect("Memory allocator can't be accessed")
    }

    pub fn queue_group(&self) -> MutexGuard<gfx::QueueGroup<back::Backend, gfx::General>> {
        self.queue_group.lock().unwrap()
    }

    pub fn release(self) {
        self.memory_allocator
            .into_inner()
            .unwrap()
            .dispose(&self.device)
            .unwrap();
        self.device.wait_idle().unwrap();
    }
}

// TODO put swapchain and stuff
pub struct DisplayContext {
    pub surface: Surface,

    pub swapchain: Option<Swapchain>,

    pub framebuffers: Vec<Framebuffer>,
    pub current_framebuffer: usize,

    pub display_renderpass: RenderPass,
}

impl DisplayContext {
    pub fn new(
        surface: Surface,
        device: &DeviceContext,
    ) -> Self {

        let renderpass = {
            use gfx::pass;

            let format = gfx::format::Format::Rgba8Unorm;

            let attachment = pass::Attachment {
                format: Some(format),
                samples: 1,
                ops: pass::AttachmentOps {
                    load: pass::AttachmentLoadOp::Clear,
                    store: pass::AttachmentStoreOp::Store,
                },
                stencil_ops: pass::AttachmentOps::DONT_CARE,
                layouts: gfx::image::Layout::Undefined .. gfx::image::Layout::Present,
            };

            let subpass = pass::SubpassDesc {
                colors: &[(0, gfx::image::Layout::ColorAttachmentOptimal)],
                depth_stencil: None,
                inputs: &[],
                resolves: &[],
                preserves: &[],
            };

            let dependency = pass::SubpassDependency {
                passes: pass::SubpassRef::External .. pass::SubpassRef::Pass(0),
                stages: gfx::pso::PipelineStage::COLOR_ATTACHMENT_OUTPUT .. gfx::pso::PipelineStage::COLOR_ATTACHMENT_OUTPUT,
                accesses: gfx::image::Access::empty() .. (gfx::image::Access::COLOR_ATTACHMENT_READ | gfx::image::Access::COLOR_ATTACHMENT_WRITE),
            };

            device.device.create_render_pass(
                &[attachment],
                &[subpass],
                &[dependency]
            )
        };

        DisplayContext {
            surface,
            swapchain: None,
            framebuffers: vec![],
            current_framebuffer: 0,
            display_renderpass: renderpass,
        }
    }

    pub fn create_swapchain(
        &mut self,
        device: &DeviceContext,
    ) {

        use gfx::Surface;

        let surface_capability = self.surface.compatibility(&device.adapter.physical_device);

        let format = gfx::format::Format::Rgba8Unorm;

        let config = gfx::SwapchainConfig::from_caps(&surface_capability.0, format);
        let extent = config.extent.to_extent();

        println!("Swapchain config size: {:?}", extent);

        let old_swapchain = self.swapchain.take();

        let (swapchain, backbuffer) = device.device.create_swapchain(
            &mut self.surface,
            config,
            old_swapchain,
        );

        self.swapchain = Some(swapchain);

        let (images, fbos) = match backbuffer {
            gfx::Backbuffer::Images(images) => {


                let pairs = images
                    .into_iter()
                    .map(|img| {
                        let view = device.device.create_image_view(
                            &img,
                            gfx::image::ViewKind::D2,
                            format,
                            gfx::format::Swizzle::NO,
                            gfx::image::SubresourceRange {
                                aspects: gfx::format::Aspects::COLOR,
                                levels: 0..1,
                                layers: 0..1,
                            }
                        ).unwrap();
                        (img, view)
                    })
                    .collect::<Vec<_>>();
                let fbos = pairs
                    .iter()
                    .map(|&(ref _image, ref view)| {
                        device.device.create_framebuffer(
                            &self.display_renderpass,
                            Some(view),
                            extent,
                        ).unwrap()
                    })
                    .collect::<Vec<_>>();

                (pairs, fbos)
            },
            gfx::Backbuffer::Framebuffer(framebuffer) => {
                (vec![], vec![framebuffer])
            }
        };

        
    }
}

// DON'T CHANGE THE ORDER OF THE MEMBERS HERE!!!!
//
// Rust drops structs by dropping the members in declaration order, so things that need to be
// dropped first need to be first in the struct declaration.
//
// BAD THINGS WILL HAPPEN IF YOU CHANGE IT.
// MOUNTAINS OF CRASHES WILL POUR ONTO YOU.
// So please, just don't.
pub struct Context {
    pub image_storage: image::ImageStorage,
    pub sampler_storage: sampler::SamplerStorage,

    pub device_ctx: DeviceContext,
    pub display_ctx: DisplayContext,
    pub instance: back::Instance,
}

impl Context {
    pub fn setup_x11(info: CreationInfoX11) -> Self {
        let instance = back::Instance::create(&info.name, info.version);
        let surface = instance.create_surface_from_xlib(info.display, info.window);

        let device_ctx = DeviceContext::new(&instance, &surface);

        let mut display_ctx = DisplayContext::new(surface, &device_ctx);

        display_ctx.create_swapchain(&device_ctx);

        Self {
            image_storage: image::ImageStorage::new(&device_ctx),
            sampler_storage: sampler::SamplerStorage::new(),
            instance,
            device_ctx,
            display_ctx,
        }
    }

    pub fn release(self) {
        self.device_ctx.release();
    }
}
