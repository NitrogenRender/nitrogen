extern crate gfx_backend_vulkan as back;
extern crate gfx_hal as gfx;
extern crate gfx_memory as gfxm;

extern crate failure;
#[macro_use]
extern crate failure_derive;

extern crate ash;

extern crate slab;

pub mod resources;

pub use resources::image;

use gfx::Device;
use gfx::Instance;
use gfx::PhysicalDevice;

use gfxm::MemoryAllocator;
use gfxm::SmartAllocator;

use ash::vk;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

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
        let memory_allocator = SmartAllocator::new(memory_properties, 256, 64, 1024, 256 * 1024 * 1024);

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
    pub surface: Box<dyn gfx::window::Surface<back::Backend>>,
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

    pub device_ctx: DeviceContext,
    pub display_ctx: DisplayContext,
    pub instance: back::Instance,
}

impl Context {
    pub fn setup_x11(info: CreationInfoX11) -> Self {
        let instance = back::Instance::create(&info.name, info.version);
        let surface = instance.create_surface_from_xlib(info.display, info.window);

        let device_ctx = DeviceContext::new(&instance, &surface);

        let display_ctx = DisplayContext {
            surface: Box::new(surface),
        };

        Self {
            image_storage: image::ImageStorage::new(&device_ctx),
            instance,
            device_ctx,
            display_ctx,
        }
    }

    pub fn release(self) {
        self.device_ctx.release();
    }
}
