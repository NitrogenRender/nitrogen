extern crate nitrogen;

pub mod image;
pub mod sampler;

use std::ffi::CStr;
use std::os::raw::*;
use std::ops::Deref;
use std::ops::DerefMut;


#[repr(C)]
pub struct CreationInfoX11 {
    pub name: *const c_char,
    pub version: u32,
    pub display: *mut c_void,
    pub window: c_ulong,
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
pub extern "C" fn context_setup_x11(info: &CreationInfoX11) -> *mut Context {
    use std::mem::transmute;
    let info = unsafe {
        nitrogen::CreationInfoX11 {
            name: CStr::from_ptr(info.name).to_string_lossy().to_string(),
            window: transmute(info.window),
            display: transmute(info.display),
            version: info.version,
        }
    };

    let context = nitrogen::Context::setup_x11(info);
    let context = Box::new(Context(context));
    Box::into_raw(context)
}

#[no_mangle]
pub unsafe extern "C" fn context_release(context: *mut Context) {
    let context = Box::from_raw(context);
    let context = *context;
    context.0.release();
}

