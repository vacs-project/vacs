use crate::error::Error;
use anyhow::Context;
use tauri::{App, AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, Window};

pub trait WindowProvider {
    fn window(&self) -> Result<WebviewWindow, Error>;
    fn scale_factor(&self) -> Result<f64, Error>;
    fn size(&self) -> Result<PhysicalSize<u32>, Error>;
    fn position(&self) -> Result<PhysicalPosition<i32>, Error>;
}

fn content_size(window: &WebviewWindow) -> Result<PhysicalSize<u32>, Error> {
    // GTK reports the window size including client-side decorations (titlebar, shadow) where the
    // compositor does not draw them, while set_size sizes the content area. The webview fills
    // exactly that content area.
    #[cfg(target_os = "linux")]
    let size = AsRef::<tauri::Webview>::as_ref(window).size();
    #[cfg(not(target_os = "linux"))]
    let size = window.inner_size();
    size.context("Failed to get window size")
        .map_err(Into::into)
}

impl WindowProvider for Window {
    fn window(&self) -> Result<WebviewWindow, Error> {
        self.get_webview_window("main")
            .context("Failed to get main window")
            .map_err(Into::into)
    }

    fn scale_factor(&self) -> Result<f64, Error> {
        self.scale_factor()
            .context("Failed to get scale factor")
            .map_err(Into::into)
    }

    fn size(&self) -> Result<PhysicalSize<u32>, Error> {
        content_size(&WindowProvider::window(self)?)
    }

    fn position(&self) -> Result<PhysicalPosition<i32>, Error> {
        self.outer_position()
            .context("Failed to get window position")
            .map_err(Into::into)
    }
}

impl WindowProvider for WebviewWindow {
    fn window(&self) -> Result<WebviewWindow, Error> {
        Ok(self.clone())
    }

    fn scale_factor(&self) -> Result<f64, Error> {
        self.scale_factor()
            .context("Failed to get scale factor")
            .map_err(Into::into)
    }

    fn size(&self) -> Result<PhysicalSize<u32>, Error> {
        content_size(self)
    }

    fn position(&self) -> Result<PhysicalPosition<i32>, Error> {
        self.outer_position()
            .context("Failed to get window position")
            .map_err(Into::into)
    }
}

impl WindowProvider for App {
    fn window(&self) -> Result<WebviewWindow, Error> {
        self.get_webview_window("main")
            .context("Failed to get main window")
            .map_err(Into::into)
    }

    fn scale_factor(&self) -> Result<f64, Error> {
        self.window()?
            .scale_factor()
            .context("Failed to get scale factor")
            .map_err(Into::into)
    }

    fn size(&self) -> Result<PhysicalSize<u32>, Error> {
        content_size(&self.window()?)
    }

    fn position(&self) -> Result<PhysicalPosition<i32>, Error> {
        self.window()?
            .outer_position()
            .context("Failed to get window position")
            .map_err(Into::into)
    }
}

impl WindowProvider for AppHandle {
    fn window(&self) -> Result<WebviewWindow, Error> {
        self.get_webview_window("main")
            .context("Failed to get main window")
            .map_err(Into::into)
    }

    fn scale_factor(&self) -> Result<f64, Error> {
        self.window()?
            .scale_factor()
            .context("Failed to get scale factor")
            .map_err(Into::into)
    }

    fn size(&self) -> Result<PhysicalSize<u32>, Error> {
        content_size(&self.window()?)
    }

    fn position(&self) -> Result<PhysicalPosition<i32>, Error> {
        self.window()?
            .outer_position()
            .context("Failed to get window position")
            .map_err(Into::into)
    }
}

impl<T: WindowProvider + ?Sized> WindowProvider for &T {
    fn window(&self) -> Result<WebviewWindow, Error> {
        <T as WindowProvider>::window(self)
    }

    fn scale_factor(&self) -> Result<f64, Error> {
        <T as WindowProvider>::scale_factor(self)
    }

    fn size(&self) -> Result<PhysicalSize<u32>, Error> {
        <T as WindowProvider>::size(self)
    }

    fn position(&self) -> Result<PhysicalPosition<i32>, Error> {
        <T as WindowProvider>::position(self)
    }
}
