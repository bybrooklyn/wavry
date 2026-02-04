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

use std::sync::Mutex;
use wavry_media::MacVideoRenderer;


use wavry_media::{MacInputInjector, InputInjector, InputEvent};

static VIDEO_RENDERER: Mutex<Option<MacVideoRenderer>> = Mutex::new(None);
static INPUT_INJECTOR: Mutex<Option<MacInputInjector>> = Mutex::new(None);

#[no_mangle]
pub extern "C" fn wavry_init_renderer(layer_ptr: *mut std::ffi::c_void) -> i32 {
    log::info!("FFI: Init renderer with ptr {:?}", layer_ptr);
    match MacVideoRenderer::new(layer_ptr) {
        Ok(renderer) => {
            let mut guard = VIDEO_RENDERER.lock().unwrap();
            *guard = Some(renderer);
            log::info!("FFI: Renderer initialized successfully");
            0
        }
        Err(e) => {
            log::error!("Failed to init renderer: {}", e);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn wavry_init_injector(width: u32, height: u32) -> i32 {
    let injector = MacInputInjector::new(width, height);
    let mut guard = INPUT_INJECTOR.lock().unwrap();
    *guard = Some(injector);
    log::info!("FFI: Input Injector initialized ({}x{})", width, height);
    0
}

#[no_mangle]
pub extern "C" fn wavry_test_input_injection() -> i32 {
    let mut guard = INPUT_INJECTOR.lock().unwrap();
    if let Some(injector) = guard.as_mut() {
        // Test: Move mouse to center
        if let Err(e) = injector.inject(InputEvent::MouseMove { x: 0.5, y: 0.5 }) {
            log::error!("Failed to inject test input: {}", e);
            return -2;
        }
        log::info!("FFI: Injected Mouse Center");
        0
    } else {
        -1
    }
}
