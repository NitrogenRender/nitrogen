extern crate gfx_hal as gfx;
extern crate gfx_backend_vulkan as back;

extern crate ash;

pub mod bindings;

use gfx::Instance;

use ash::vk;

use std::os::raw::*;

pub struct CreationInfoX11 {
    name: String,
    version: u32,
    display: *mut vk::Display,
    window: vk::Window,
}

pub fn setup_x11(info: CreationInfoX11) {


    let instance = back::Instance::create(&info.name, info.version);
    let surface = instance.create_surface_from_xlib(info.display, info.window);
    let adapters = instance.enumerate_adapters();

    for adapter in adapters {
        println!("{:?}", adapter.info);
    }
}

