extern crate gfx_backend_vulkan as back;
extern crate gfx_hal as gfx;

extern crate ash;

extern crate slab;

pub mod image;

pub mod bindings;

use gfx::window::Surface;
use gfx::Instance;
use gfx::Device;

use ash::vk;

use std::os::raw::*;
use std::sync::Arc;

pub struct CreationInfoX11 {
    name: String,
    version: u32,
    display: *mut vk::Display,
    window: vk::Window,
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
    image_storage: image::ImageStorage,

    queue_group: gfx::QueueGroup<back::Backend, gfx::Graphics>,
    device: Arc<back::Device>,
    surface: Box<dyn gfx::window::Surface<back::Backend>>,
    adapter: gfx::Adapter<back::Backend>,
    instance: back::Instance,
}

impl Context {
    pub fn setup_x11(info: CreationInfoX11) -> Self {
        let instance = back::Instance::create(&info.name, info.version);
        let surface = instance.create_surface_from_xlib(info.display, info.window);
        let mut adapters = instance.enumerate_adapters();

        // TODO select adapter(s)
        for adapter in &adapters {
            println!("{:?}", adapter.info);
        }

        let mut adapter = adapters.remove(0);

        let (device, queue_group) = adapter
            .open_with::<_, gfx::Graphics>(1, |family| surface.supports_queue_family(family))
            .unwrap();

        let device = Arc::new(device);

        Self {
            image_storage: image::ImageStorage::new(device.clone()),
            instance,
            surface: Box::new(surface),
            adapter,
            device,
            queue_group,
        }
    }

    pub fn release(self) {
        self.device.wait_idle().unwrap();
    }
}
