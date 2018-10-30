extern crate gfx_backend_vulkan as back;
extern crate gfx_hal as gfx;
extern crate gfx_memory as gfxm;

extern crate shaderc;
extern crate smallvec;

extern crate failure;
extern crate failure_derive;

extern crate bitflags;

extern crate ash;

extern crate slab;

#[cfg(feature = "winit_support")]
extern crate winit;

use smallvec::SmallVec;

pub mod types;

pub mod display;
use display::Display;

pub mod device;
use device::DeviceContext;

pub mod util;
pub use util::storage;
pub use util::transfer;

use storage::{Handle, Storage};

pub mod resources;
pub use resources::buffer;
pub use resources::image;
pub use resources::pipeline;
pub use resources::render_pass;
pub use resources::sampler;

pub mod graph;

#[cfg(feature = "x11")]
use ash::vk;

pub type DisplayHandle = Handle<Display>;

// DON'T CHANGE THE ORDER OF THE MEMBERS HERE!!!!
//
// Rust drops structs by dropping the members in declaration order, so things that need to be
// dropped first need to be first in the struct declaration.
//
// BAD THINGS WILL HAPPEN IF YOU CHANGE IT.
// MOUNTAINS OF CRASHES WILL POUR ONTO YOU.
// So please, just don't.
pub struct Context {
    pub graph_storage: graph::GraphStorge,

    pub pipeline_storage: pipeline::PipelineStorage,
    pub image_storage: image::ImageStorage,
    pub sampler_storage: sampler::SamplerStorage,
    pub buffer_storage: buffer::BufferStorage,

    pub displays: Storage<Display>,
    pub transfer: transfer::TransferContext,
    pub device_ctx: DeviceContext,
    pub instance: back::Instance,
}

impl Context {
    pub fn new(name: &str, version: u32) -> Self {
        let instance = back::Instance::create(name, version);
        let device_ctx = DeviceContext::new(&instance);

        let transfer = transfer::TransferContext::new(&device_ctx);

        let image_storage = image::ImageStorage::new();
        let sampler_storage = sampler::SamplerStorage::new();
        let buffer_storage = buffer::BufferStorage::new();
        let pipeline_storage = pipeline::PipelineStorage::new();

        let graph_storage = graph::GraphStorge::new();

        Context {
            instance,
            device_ctx,
            transfer,
            displays: Storage::new(),
            pipeline_storage,
            image_storage,
            sampler_storage,
            buffer_storage,
            graph_storage,
        }
    }

    #[cfg(feature = "x11")]
    pub fn add_x11_display(
        &mut self,
        display: *mut vk::Display,
        window: vk::Window,
    ) -> DisplayHandle {
        use gfx::Surface;

        let surface = self.instance.create_surface_from_xlib(display, window);

        let _ = self
            .device_ctx
            .adapter
            .queue_families
            .iter()
            .position(|fam| surface.supports_queue_family(fam))
            .expect("No queue family that supports this surface was found.");

        let display = Display::new(surface, &self.device_ctx);

        self.displays.insert(display).0
    }

    #[cfg(feature = "winit_support")]
    pub fn add_display(&mut self, window: &winit::Window) -> Handle<Display> {
        use gfx::Surface;

        let surface = self.instance.create_surface(window);

        let _ = self
            .device_ctx
            .adapter
            .queue_families
            .iter()
            .position(|fam| surface.supports_queue_family(fam))
            .expect("No queue family that supports this surface was found.");

        let display = Display::new(surface, &self.device_ctx);

        self.displays.insert(display).0
    }

    pub fn remove_display(&mut self, display: DisplayHandle) -> bool {
        match self.displays.remove(display) {
            None => false,
            Some(display) => {
                display.release(&self.device_ctx);
                true
            }
        }
    }

    pub fn release(self) {
        self.buffer_storage.release();
        self.image_storage.release(&self.device_ctx);

        for (_, display) in self.displays {
            display.release(&self.device_ctx);
        }

        self.transfer.release(&self.device_ctx);

        self.device_ctx.release();
    }

    // convenience functions that delegate the work

    // image

    pub fn image_create(
        &mut self,
        create_infos: &[image::ImageCreateInfo],
    ) -> SmallVec<[image::Result<image::ImageHandle>; 16]> {
        self.image_storage.create(&self.device_ctx, create_infos)
    }

    pub fn image_upload_data(
        &mut self,
        images: &[(image::ImageHandle, image::ImageUploadInfo)],
    ) -> SmallVec<[image::Result<()>; 16]> {
        self.image_storage
            .upload_data(&self.device_ctx, &mut self.transfer, images)
    }

    pub fn image_destroy(&mut self, handles: &[image::ImageHandle]) {
        self.image_storage.destroy(&self.device_ctx, handles)
    }

    // sampler

    pub fn sampler_create(&mut self, create_infos: &[sampler::SamplerCreateInfo]) -> SmallVec<[sampler::SamplerHandle; 16]> {
        self.sampler_storage.create(&self.device_ctx, create_infos)
    }

    pub fn sampler_destroy(&mut self, handles: &[sampler::SamplerHandle]) {
        self.sampler_storage.destroy(&self.device_ctx, handles)
    }

    // graph

    pub fn graph_create(&mut self) -> graph::GraphHandle {
        self.graph_storage.create()
    }

    pub fn graph_add_pass(&mut self, graph: graph::GraphHandle, name: &str, info: graph::PassInfo, pass_impl: Box<dyn graph::PassImpl>) -> graph::PassId {
        self.graph_storage.add_pass(graph, name, info, pass_impl)
    }

    pub fn graph_add_output_image(&mut self, graph: graph::GraphHandle, image_name: &str) -> bool {
        self.graph_storage.add_output_image(graph, image_name)
    }

    pub fn graph_destroy(&mut self, graph: graph::GraphHandle) {
        self.graph_storage.destroy(graph);
    }

    pub fn graph_construct(&mut self, graph: graph::GraphHandle) {
        self.graph_storage.construct(graph)
    }
}
