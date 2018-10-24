extern crate gfx_backend_vulkan as back;
extern crate gfx_hal as gfx;
extern crate gfx_memory as gfxm;

extern crate smallvec;
extern crate shaderc;

extern crate failure;
#[macro_use]
extern crate failure_derive;

#[macro_use]
extern crate bitflags;

extern crate ash;

extern crate slab;

#[cfg(feature = "winit_support")]
extern crate winit;


pub mod types;

pub mod display;
use display::Display;

pub mod device;
use device::DeviceContext;


pub mod util;
pub use util::storage;
pub use util::transfer;

use storage::{Storage, Handle};

pub mod resources;
pub use resources::image;
pub use resources::sampler;
pub use resources::buffer;

pub mod graph;


#[cfg(feature = "winit_support")]
pub struct CreationInfo<'a> {
    pub name: String,
    pub version: u32,
    pub window: &'a winit::Window,
}

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

        Context {
            instance,
            device_ctx,
            transfer,
            displays: Storage::new(),
            image_storage,
            sampler_storage,
            buffer_storage,
        }
    }


    #[cfg(feature = "x11")]
    pub fn add_x11_display(&mut self, display: *mut vk::Display, window: vk::Window) -> DisplayHandle {
        use gfx::Surface;

        let surface = self.instance.create_surface_from_xlib(display, window);

        let _ = self.device_ctx.adapter.queue_families
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

        let _ = self.device_ctx.adapter.queue_families
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
        self.image_storage.release();

        for display in self.displays {
            display.release(&self.device_ctx);
        }

        self.transfer.release(&self.device_ctx);

        self.device_ctx.release();
    }

}
