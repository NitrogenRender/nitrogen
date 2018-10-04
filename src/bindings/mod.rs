use std::os::raw::*;
use std::ffi::CStr;

#[repr(C)]
pub struct BindingCreationInfoX11 {
    name: *const c_char,
    version: u32,
    display: *mut c_void,
    window: c_ulong,
}

#[no_mangle]
pub unsafe extern "C" fn give_me_five() -> u64 {
    println!("High five!");
    5
}

#[no_mangle]
pub extern "C" fn setup_x11(info: &BindingCreationInfoX11) {
    use std::mem::transmute;
    let info = unsafe {
        super::CreationInfoX11 {
            name: CStr::from_ptr(info.name).to_string_lossy().to_string(),
            window: transmute(info.window),
            display: transmute(info.display),
            version: info.version,
        }
    };

    super::setup_x11(info);
}
