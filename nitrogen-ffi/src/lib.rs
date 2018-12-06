extern crate env_logger;
extern crate nitrogen;
extern crate smallvec;

pub mod image;
pub mod sampler;

use std::ffi::CStr;
use std::ops::Deref;
use std::ops::DerefMut;
use std::os::raw::*;

#[repr(C)]
pub struct DisplayHandle(pub usize, pub u64);

impl DisplayHandle {
    pub fn into(self) -> nitrogen::DisplayHandle {
        nitrogen::DisplayHandle::new(self.0, self.1)
    }
}

pub struct Context(nitrogen::Context);

impl Deref for Context {
    type Target = nitrogen::Context;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Context {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[no_mangle]
pub unsafe extern "C" fn context_create(name: *const c_char, version: u32) -> *mut Context {
    // FIXME HACK TO SEE VALIDATION LAYER OUTPUT IN GODOT.
    // I need to find a better solution for that.
    env_logger::init();
    let context = nitrogen::Context::new(&CStr::from_ptr(name).to_string_lossy(), version);

    let context = Box::new(Context(context));
    Box::into_raw(context)
}

#[no_mangle]
pub unsafe extern "C" fn context_add_x11_display(
    context: *mut Context,
    display: *mut c_void,
    window: c_ulong,
) -> DisplayHandle {
    let context = &mut (*context);

    use std::mem::transmute;
    let handle = context.add_x11_display(transmute(display), window);

    DisplayHandle(handle.id(), handle.generation())
}

#[no_mangle]
pub unsafe extern "C" fn context_remove_display(
    context: *mut Context,
    display: DisplayHandle,
) -> bool {
    let context = &mut (*context);
    context.remove_display(display.into())
}

#[no_mangle]
pub unsafe extern "C" fn context_release(context: *mut Context) {
    let context = Box::from_raw(context);
    let context = *context;
    context.0.release();
}

#[no_mangle]
pub unsafe extern "C" fn display_setup_swapchain(context: *mut Context, display: DisplayHandle) {
    let device_ctx = &(*context).device_ctx;
    let display_ctx = &mut (*context).displays[display.into()];

    // display_ctx.setup_swapchain(device_ctx);
}

#[no_mangle]
pub unsafe extern "C" fn display_present(
    context: *mut Context,
    display: DisplayHandle,
    image: image::ImageHandle,
    sampler: sampler::SamplerHandle,
) -> bool {
    let device_ctx = &(*context).device_ctx;
    let display_ctx = &mut (*context).displays[display.into()];

    /*
    display_ctx.present(
        &device_ctx,
        &(*context).image_storage,
        image.into(),
        &(*context).sampler_storage,
        sampler.into(),
    )
    */
    true
}
