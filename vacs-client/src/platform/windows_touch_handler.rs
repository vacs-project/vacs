//! Windows Touch Input Handler
//!
//! This module implements a transparent "shield" window that sits on top of the WebView to intercept
//! and normalize touch input behaviors on Windows. It is primarily used to:
//!
//! 1.  **Handle touch inputs gracefully**: It intercepts touch events and synthesizes corresponding mouse messages
//!     to the underlying WebView, ensuring the UI remains interactive and responsive to touch.
//! 2.  **Prevent cursor displacement**: It prevents the Windows native touch handling from hiding or teleporting
//!     the mouse cursor, keeping the cursor visible and untampered with during touch interactions.
//! 3.  **Forward input**: It forwards necessary mouse and touch events to the underlying `Chrome_RenderWidgetHostHWND`
//!     child window to maintain correct functionality.
//!
//! It works by creating a layered, transparent, tool window that tracks the position of the main window
//! and acts as an input interception layer.

use std::mem::size_of;
use std::sync::OnceLock;
use std::thread;
use windows::Win32::Foundation::{
    COLORREF, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetStockObject, HBRUSH, ScreenToClient, WHITE_BRUSH,
};
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleHandleExW,
};
use windows::Win32::UI::Input::Pointer::{GetPointerInfo, POINTER_INFO};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumChildWindows,
    GetClassNameW, GetClientRect, GetMessageW, GetWindowRect, IDC_ARROW, KillTimer, LWA_ALPHA,
    LoadCursorW, MA_NOACTIVATE, MSG, PostMessageW, PostQuitMessage, RegisterClassExW,
    SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER, SetLayeredWindowAttributes, SetTimer,
    SetWindowPos, TranslateMessage, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_POINTERENTER, WM_POINTERLEAVE, WM_POINTERUPDATE, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_POPUP, WS_VISIBLE,
};
use windows::Win32::UI::WindowsAndMessaging::{WM_POINTERDOWN, WM_POINTERUP};
use windows::core::{BOOL, PCWSTR, w};

/// Handle to the main WebView window (the parent/container).
static WEBVIEW_HWND: OnceLock<usize> = OnceLock::new();
/// Handle to the actual `Chrome_RenderWidgetHostHWND` child window that receives input.
static WEBVIEW_CHILD_HWND: OnceLock<usize> = OnceLock::new();

const WEBVIEW_CHILD_CLASS_NAME: &str = "Chrome_RenderWidgetHostHWND";

/// Installs the Windows Touch Handler for the given WebView window.
///
/// This spawns a background thread to manage the touch handler window lifecycle.
///
/// # Arguments
///
/// * `webview_hwnd` - The raw `HWND` (as usize) of the main WebView window.
pub fn install(webview_hwnd: usize) {
    if WEBVIEW_HWND.set(webview_hwnd).is_err() {
        log::warn!("Touch Handler already installed for WebView HWND: {webview_hwnd:?}");
        return;
    }

    log::info!("Installing Touch Handler for WebView HWND: {webview_hwnd:?}");

    // Try to find the actual WebView child window (Chrome_RenderWidgetHostHWND)
    // We try immediately, but it might not be created yet. The timer can retry if needed.
    if let Some(child) = find_webview_child(HWND(webview_hwnd as *mut _)) {
        log::info!("Found WebView Child: {child:?}");
        WEBVIEW_CHILD_HWND.set(child.0 as usize).ok();
    } else {
        log::warn!("Could not find Chrome_RenderWidgetHostHWND yet. Will retry in Timer.");
    }

    thread::spawn(move || {
        create_touch_handler_window(HWND(webview_hwnd as *mut _));
    });
}

/// Helper to find the `Chrome_RenderWidgetHostHWND` child window.
fn find_webview_child(parent: HWND) -> Option<HWND> {
    let mut found_hwnd = None;
    // EnumChildWindows enumerates the child windows that belong to the specified parent window
    // by passing the handle to each child window, in turn, to an application-defined callback function.
    // SAFETY: Valid parent handle and callback function provided.
    // The LPARAM is a pointer to our stack-allocated Option<HWND>, which is valid for the duration of the call.
    unsafe {
        let _ = EnumChildWindows(
            Some(parent),
            Some(enum_child_proc),
            LPARAM(&mut found_hwnd as *mut _ as isize),
        );
    }
    found_hwnd
}

/// Callback for `EnumChildWindows`. Checks the class name of each child.
unsafe extern "system" fn enum_child_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: We trust the HWND provided by the OS during enumeration.
    // lparam is cast back to *mut Option<HWND> which we passed in find_webview_child.
    let found_ptr = lparam.0 as *mut Option<HWND>;
    let mut class_name = [0u16; 256];

    // GetClassNameW retrieves the name of the class to which the specified window belongs.
    // SAFETY: The buffer is stack-allocated and valid. 256 chars is standard max class name length.
    let len = unsafe { GetClassNameW(hwnd, &mut class_name) };

    if len > 0 {
        let class_string = String::from_utf16_lossy(&class_name[..len as usize]);
        if class_string == WEBVIEW_CHILD_CLASS_NAME {
            // SAFETY: found_ptr is derived from the LPARAM passed to EnumChildWindows,
            // which we know points to a valid local variable in find_webview_child.
            unsafe {
                *found_ptr = Some(hwnd);
            }
            return BOOL(0); // Stop enumeration
        }
    }
    BOOL(1) // Continue enumeration
}

/// Creates and manages the message loop for the touch handler window.
fn create_touch_handler_window(target_hwnd: HWND) {
    let mut instance = HINSTANCE::default();

    // GetModuleHandleExW retrieves a module handle for the specified module and increments the module's reference count.
    // GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT The module's reference count is not incremented.
    // SAFETY: Valid pointer to receive the handle. Getting the current module (None) is safe.
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            None,
            &mut instance.0 as *mut _ as *mut _,
        )
    }
    .is_err()
    {
        log::warn!(
            "Failed to get module handle, falling back to default: {:?}",
            unsafe { GetLastError() }
        );
    }

    let class_name = w!("vacs_TouchInputHandler");

    let wnd_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(touch_handler_proc),
        hInstance: HINSTANCE(instance.0),
        // LoadCursorW loads the specified cursor resource from the executable (.exe) file associated with an application instance.
        // SAFETY: Loading system default arrow cursor is safe.
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        // GetStockObject retrieves a handle to one of the stock pens, brushes, fonts, or palettes.
        // SAFETY: Retrieving stock white brush is safe.
        hbrBackground: HBRUSH(unsafe { GetStockObject(WHITE_BRUSH).0 } as _),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };

    // RegisterClassExW registers a window class for subsequent use in calls to the CreateWindow or CreateWindowEx function.
    // SAFETY: Valid pointer to a WNDCLASSEXW structure provided.
    if unsafe { RegisterClassExW(&wnd_class) } == 0 {
        log::warn!(
            "Failed to register vacs_TouchInputHandler window class: {:?}",
            unsafe { GetLastError() }
        );
    }

    // CreateWindowExW creates an overlapped, pop-up, or child window with an extended window style.
    // SAFETY: Valid class name and styles provided, standard window creation.
    let hwnd_res = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("vacs Touch Input Handler"),
            WS_POPUP | WS_VISIBLE,
            0,
            0,
            0,
            0,
            Some(target_hwnd),
            None,
            Some(instance),
            None,
        )
    };

    match hwnd_res {
        Ok(hwnd) => {
            // SetLayeredWindowAttributes sets the opacity and transparency color key of a layered window.
            // SAFETY: Valid HWND. We set alpha to 1 (almost invisible but input active).
            if unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA) }.is_err() {
                log::warn!("Failed to set window opacity: {:?}", unsafe {
                    GetLastError()
                });
            }

            // SetTimer creates a timer used to sync the handler position with the WebView.
            // SAFETY: Valid HWND. We start timer ID 1 with 15ms interval.
            let timer_result = unsafe { SetTimer(Some(hwnd), 1, 15, None) };
            if timer_result == 0 {
                log::warn!("Failed to set timer for handler sync: {:?}", unsafe {
                    GetLastError()
                });
            }

            log::info!("Touch input handler window created: {hwnd:?}");

            let mut msg = MSG::default();
            loop {
                // GetMessageW retrieves a message from the calling thread's message queue.
                // SAFETY: Valid pointer to MSG structure.
                let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                if r.0 == -1 {
                    log::warn!("GetMessageW failed: {:?}", unsafe { GetLastError() });
                    break;
                } else if r.0 == 0 {
                    // WM_QUIT
                    log::trace!("Received WM_QUIT, exiting message loop");
                    break;
                } else {
                    // TranslateMessage translates virtual-key messages into character messages.
                    // DispatchMessageW dispatches a message to a window procedure.
                    // SAFETY: Standard message loop processing.
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }

            // Cleanup: Kill the timer when the loop exits, but only if it was successfully created.
            // KillTimer destroys the specified timer.
            // SAFETY: Valid HWND and timer ID.
            if timer_result != 0 && unsafe { KillTimer(Some(hwnd), 1) }.is_err() {
                log::warn!("Failed to kill handler sync timer: {:?}", unsafe {
                    GetLastError()
                });
            }
        }
        Err(_) => {
            log::warn!(
                "Failed to create touch input handler window: {:?}",
                unsafe { GetLastError() }
            );
        }
    }
}

/// Window procedure for the touch handler window. Handles input forwarding and position syncing.
unsafe extern "system" fn touch_handler_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_POINTERDOWN => {
            let pointer_id = (wparam.0 & 0xFFFF) as u32;
            log::trace!("Synthesizing DOWN for pointer {pointer_id}");
            // Use Child HWND for Input
            if let Some(&child_val) = WEBVIEW_CHILD_HWND.get() {
                click_on_touch(pointer_id, HWND(child_val as *mut _), WM_LBUTTONDOWN);
            }
            return LRESULT(0);
        }
        WM_POINTERUP => {
            let pointer_id = (wparam.0 & 0xFFFF) as u32;
            log::trace!("Synthesizing UP for pointer {pointer_id}");
            // Use Child HWND for Input
            if let Some(&child_val) = WEBVIEW_CHILD_HWND.get() {
                click_on_touch(pointer_id, HWND(child_val as *mut _), WM_LBUTTONUP);
            }
            return LRESULT(0);
        }
        WM_POINTERUPDATE | WM_POINTERENTER | WM_POINTERLEAVE => {
            return LRESULT(0);
        }
        WM_MOUSEACTIVATE => {
            // Prevent the window from activating when clicked
            return LRESULT(MA_NOACTIVATE as isize);
        }
        WM_TIMER => {
            if wparam.0 == 1 {
                // Sync Position - Use Parent HWND
                if let Some(&target_val) = WEBVIEW_HWND.get() {
                    let target = HWND(target_val as *mut _);

                    // Retry finding child if missing
                    if WEBVIEW_CHILD_HWND.get().is_none()
                        && let Some(child) = find_webview_child(target)
                    {
                        log::info!("Found WebView Child (late): {child:?}");
                        WEBVIEW_CHILD_HWND.set(child.0 as usize).ok();
                    }

                    let mut rect = RECT::default();
                    // GetClientRect retrieves the coordinates of a window's client area.
                    // SAFETY: Win32 API calls to get window geometry.
                    if unsafe { GetClientRect(target, &mut rect) }.is_ok() {
                        let mut pt = POINT {
                            x: rect.left,
                            y: rect.top,
                        };
                        // ClientToScreen converts the client-area coordinates of a specified point to screen coordinates.
                        // SAFETY: Valid target window handle.
                        unsafe {
                            let _ = ClientToScreen(target, &mut pt);
                        };

                        let width = rect.right - rect.left;
                        let height = rect.bottom - rect.top;

                        let mut current_rect = RECT::default();
                        // GetWindowRect retrieves the dimensions of the bounding rectangle of the specified window.
                        // SAFETY: Valid window handle.
                        unsafe {
                            let _ = GetWindowRect(hwnd, &mut current_rect);
                        };
                        let current_width = current_rect.right - current_rect.left;
                        let current_height = current_rect.bottom - current_rect.top;

                        if current_rect.left != pt.x
                            || current_rect.top != pt.y
                            || current_width != width
                            || current_height != height
                        {
                            // SetWindowPos changes the size, position, and Z order of a child, pop-up, or top-level window.
                            // SAFETY: Valid window handle. We explicitly use null_mut for HWND_TOP/insert_after as we don't change Z-order.
                            let _ = unsafe {
                                SetWindowPos(
                                    hwnd,
                                    Some(HWND(std::ptr::null_mut())),
                                    pt.x,
                                    pt.y,
                                    width,
                                    height,
                                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
                                )
                            };
                        }
                    }
                }
            }
            return LRESULT(0);
        }
        WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN
        | WM_RBUTTONUP | WM_RBUTTONDBLCLK | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MBUTTONDBLCLK
        | WM_MOUSEWHEEL => {
            // Forward Mouse to Child HWND
            if let Some(&child_val) = WEBVIEW_CHILD_HWND.get() {
                let target = HWND(child_val as *mut _);
                // PostMessageW places (posts) a message in the message queue associated with the thread that created the specified window.
                // SAFETY: PostMessageW is generally safe; we're forwarding the message to the webview child.
                unsafe {
                    let _ = PostMessageW(Some(target), msg, wparam, lparam);
                }
                return LRESULT(0);
            }
        }
        WM_DESTROY => {
            log::info!("Touch input handler window destroyed");
            // PostQuitMessage indicates to the system that a thread has made a request to terminate (quit).
            // SAFETY: Standard cleanup.
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        _ => {}
    }
    // DefWindowProcW calls the default window procedure to provide default processing.
    // SAFETY: Default processing for unhandled messages.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Helper to synthesize mouse clicks from touch pointer events.
/// Also forces the cursor to be visible and correctly positioned.
fn click_on_touch(pointer_id: u32, target: HWND, msg: u32) {
    let mut info = POINTER_INFO::default();
    // GetPointerInfo retrieves information about the specified pointer.
    // SAFETY: We provide a valid pointer to POINTER_INFO structure.
    if unsafe { GetPointerInfo(pointer_id, &mut info) }.is_ok() {
        let mut pt = info.ptPixelLocation;

        // ScreenToClient converts screen coordinates to client coordinates.
        // SAFETY: Valid target window handle.
        unsafe {
            let _ = ScreenToClient(target, &mut pt);
        }

        let x = pt.x as i16;
        let y = pt.y as i16;
        let lparam_val = (x as u16 as usize) | ((y as u16 as usize) << 16);

        // PostMessageW posts a message to the message queue.
        // SAFETY: Forwarding message to valid target.
        unsafe {
            let _ = PostMessageW(Some(target), msg, WPARAM(0), LPARAM(lparam_val as isize));
        }
    }
}
