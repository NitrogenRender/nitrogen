use std::ffi::CStr;
use std::os::raw::*;

use super::image::{ImageCreateInfo, ImageDimension, ImageFormat, ImageHandle, ImageUploadInfo};

#[repr(C)]
pub struct BindingImageUploadInfo {
    pub data: *const u8,
    pub data_len: u64,
    pub format: ImageFormat,
    pub dimension: ImageDimension,
    pub target_offset: [u32; 3],
}

#[repr(C)]
pub struct BindingCreationInfoX11 {
    pub name: *const c_char,
    pub version: u32,
    pub display: *mut c_void,
    pub window: c_ulong,
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
pub unsafe extern "C" fn context_release(context: *mut super::Context) {
    let context = Box::from_raw(context);
    context.release();
}

#[no_mangle]
pub unsafe extern "C" fn image_create(
    context: *mut super::Context,
    create_info: ImageCreateInfo,
    handle: *mut ImageHandle,
) -> bool {
    let context = &mut *context;

    let result = context
        .image_storage
        .create(&context.device_ctx, create_info);

    match result {
        Ok(t) => {
            *handle = t;
            true
        }
        Err(_) => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn image_get_dimension(
    context: *const super::Context,
    handle: ImageHandle,
    dimension: *mut ImageDimension,
) -> bool {
    let context = &*context;

    match context.image_storage.get_dimension(handle) {
        None => false,
        Some(x) => {
            *dimension = x;
            true
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn image_upload_data(
    context: *mut super::Context,
    image: ImageHandle,
    data: BindingImageUploadInfo,
) -> bool {
    let context = &mut *context;

    use std::slice;

    let upload_info = ImageUploadInfo {
        data: slice::from_raw_parts(data.data, data.data_len as usize),
        format: data.format,
        dimension: data.dimension,
        target_offset: (
            data.target_offset[0],
            data.target_offset[1],
            data.target_offset[2],
        ),
    };

    let result = context
        .image_storage
        .upload_data(&context.device_ctx, image, upload_info);

    result.is_ok()
}

#[no_mangle]
pub unsafe extern "C" fn image_destroy(context: *mut super::Context, image: ImageHandle) -> bool {
    let context = &mut *context;

    context.image_storage.destroy(&context.device_ctx, image)
}
