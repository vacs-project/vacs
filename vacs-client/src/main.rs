// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod platform;

fn main() {
    #[cfg(unix)]
    {
        let platform = platform::Platform::get();
        if matches!(platform, platform::Platform::LinuxWayland) {
            unsafe {
                // Workarounds for WebKitGTK + Wayland rendering issues.
                //
                // WebKitGTK's DMA-BUF accelerated renderer has known bugs that cause
                // UI freezes (app keeps running but the window stops painting) and
                // crashes ("Error 71 Protocol error") on Wayland, especially with
                // NVIDIA drivers but also affecting AMD/Intel.
                //
                // See: https://github.com/tauri-apps/tauri/issues/10702
                // See: https://github.com/tauri-apps/tauri/issues/13498
                // See: https://bugs.webkit.org/show_bug.cgi?id=291332

                // Disable NVIDIA's threaded GL optimization, which can race with
                // WebKitGTK's own threading. No-op on non-NVIDIA systems.
                std::env::set_var("__GL_THREADED_OPTIMIZATIONS", "0");

                // Disable NVIDIA explicit sync to prevent Wayland protocol errors.
                // No-op on non-NVIDIA systems.
                std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");

                // Disable the DMA-BUF renderer, falling back to shared-memory
                // buffer transfers. This is the primary fix for UI freezes.
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");

                // Disable GPU-accelerated compositing for CSS layers, transforms,
                // and animations. Falls back to CPU rendering. Removes GPU-
                // dependent rendering bugs but degrades animation performance
                // and disables effects like backdrop-filter: blur().
                // std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");

                // Suppress Mesa's device selection Vulkan layer, which auto-
                // selects the "best" GPU on multi-GPU systems (e.g. iGPU + dGPU
                // laptops). Prevents hangs from selecting the wrong GPU.
                // std::env::set_var("NODEVICE_SELECT", "1");
            }
        }
    }
    vacs_client_lib::run();
}
