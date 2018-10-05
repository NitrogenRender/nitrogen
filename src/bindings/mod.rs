use std::ffi::CStr;
use std::os::raw::*;

use super::image::{ImageHandle, ImageCreateInfo, ImageDimension};

#[repr(C)]
pub struct BindingCreationInfoX11 {
    name: *const c_char,
    version: u32,
    display: *mut c_void,
    window: c_ulong,
}

#[no_mangle]
pub extern "C" fn context_setup_x11(info: &BindingCreationInfoX11) -> *mut super::Context {
    use std::mem::transmute;
    let info = unsafe {
        super::CreationInfoX11 {
            name: CStr::from_ptr(info.name).to_string_lossy().to_string(),
            window: transmute(info.window),
            display: transmute(info.display),
            version: info.version,
        }
    };

    let context = super::Context::setup_x11(info);
    let context = Box::new(context);
    Box::into_raw(context)
}

#[no_mangle]
pub extern "C" fn context_release(context: *mut super::Context) {
    let context = unsafe { Box::from_raw(context) };
    context.release();
}

#[no_mangle]
pub extern "C" fn image_create(context: &mut super::Context, create_info: ImageCreateInfo) -> ImageHandle {
    context.image_storage.create(create_info)
}

#[no_mangle]
pub extern "C" fn image_destroy(context: &mut super::Context, image: ImageHandle) {
    context.image_storage.destroy(image);
}