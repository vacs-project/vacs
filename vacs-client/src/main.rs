// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(unix)]
mod platform;

fn main() {
    #[cfg(unix)]
    {
        let platform = platform::Platform::get();
        if matches!(platform, platform::Platform::LinuxWayland) {
            unsafe {
                // Workaround required until Wayland issues have been fixed.
                // See: https://github.com/tauri-apps/tauri/issues/10702
                std::env::set_var("__GL_THREADED_OPTIMIZATIONS", "0");
                std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
            }
        }
    }
    vacs_client_lib::run();
}
