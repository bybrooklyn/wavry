use std::ffi::{c_char, CStr};

#[no_mangle]
pub extern "C" fn wavry_init() {
    env_logger::init();
    log::info!("Wavry Core (FFI) Initialized 🚀");
}

#[no_mangle]
pub unsafe extern "C" fn wavry_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}
#[no_mangle]
pub extern "C" fn wavry_connect() {
    log::info!("FFI: Connect requested");
}
