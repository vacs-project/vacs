//! Handing control to programs that live on the host rather than in our bundle.
//!
//! An AppImage's `AppRun` prepends the AppDir to `PATH` and points `LD_LIBRARY_PATH`, the GTK/GIO
//! module caches and the GSettings schema dir at the bundled copies. That is correct for our own
//! process, but every child we spawn inherits it, which breaks host programs two ways: `xdg-open`
//! resolves to the bundle's copy (built against whatever distribution CI ran on, so on a Plasma 6
//! session an xdg-utils 1.1.3 copy silently exits 0 without opening anything), and the opener it
//! finally execs fails to load against the bundle's older libraries. Strip the bundle back out of
//! the environment before handing over.
//!
//! Outside an AppImage every function here is a straight passthrough.
//!
//! tauri-apps/tauri#15804 stops the bundler shipping its own xdg-utils from 2.12 on, which removes
//! the first half. Do not drop this module when we bump to it: the inherited `LD_LIBRARY_PATH` kills
//! the host opener on its own, verified with a Fedora 44 host and an ubuntu-24.04 build.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use std::{
    ffi::OsString,
    process::{Command, Stdio},
};

/// Colon-separated search paths `AppRun` and the linuxdeploy GTK hook prepend AppDir entries to.
/// The set matches what our bundle's `AppRun.wrapped` and `apprun-hooks/linuxdeploy-plugin-gtk.sh`
/// actually export, plus `GI_TYPELIB_PATH`, which upstream versions of the GTK hook set and ours
/// may pick up on a bundler update; stripping an unset variable is a no-op.
#[cfg(target_os = "linux")]
const BUNDLE_SEARCH_PATHS: &[&str] = &[
    "PATH",
    "LD_LIBRARY_PATH",
    "XDG_DATA_DIRS",
    "GI_TYPELIB_PATH",
    "GTK_PATH",
    "QT_PLUGIN_PATH",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "PYTHONPATH",
    "PERLLIB",
];

/// Variables `AppRun` and the linuxdeploy GTK hook point at bundled files outright. A host program
/// has its own correct values for all of these, so the child is better off with them unset.
#[cfg(target_os = "linux")]
const BUNDLE_OVERRIDES: &[&str] = &[
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "GDK_BACKEND",
    "GDK_PIXBUF_MODULE_FILE",
    "GIO_EXTRA_MODULES",
    "GSETTINGS_SCHEMA_DIR",
    "GTK_DATA_PREFIX",
    "GTK_EXE_PREFIX",
    "GTK_IM_MODULE_FILE",
    "GTK_THEME",
    "LD_PRELOAD",
    "PIPEWIRE_MODULE_DIR",
    "PYTHONHOME",
    "SPA_PLUGIN_DIR",
];

/// AppDir-relative directories the CI overlay bundles the PipeWire plugins into. These are the
/// destination keys of `bundle.linux.appimage.files` in tauri.appimage.conf.json; the overlay test
/// below keeps the two in sync.
#[cfg(target_os = "linux")]
const BUNDLED_SPA_PLUGIN_DIR: &str = "usr/lib/spa-0.2";
#[cfg(target_os = "linux")]
const BUNDLED_PIPEWIRE_MODULE_DIR: &str = "usr/lib/pipewire-0.3";

/// The AppDir we are running out of, if this process was launched from an AppImage.
#[cfg(target_os = "linux")]
fn app_dir() -> Option<PathBuf> {
    std::env::var_os("APPDIR").map(PathBuf::from)
}

/// Drops every AppDir entry from a colon-separated search path, returning `None` if nothing is
/// left, so the caller can unset the variable instead of passing an empty one. Entries pass
/// through byte for byte: a non-UTF-8 path, legal on Linux, must survive the round trip.
#[cfg(target_os = "linux")]
fn strip_app_dir(value: &OsString, app_dir: &Path) -> Option<OsString> {
    let kept = std::env::split_paths(value)
        .filter(|entry| !entry.as_os_str().is_empty() && !entry.starts_with(app_dir))
        .collect::<Vec<_>>();

    if kept.is_empty() {
        return None;
    }

    // Cannot fail: every entry came out of split_paths, so none contains a separator.
    std::env::join_paths(kept).ok()
}

/// Builds a [`Command`] for a host program, with the bundle removed from the environment it
/// inherits. Since Rust 1.58 a spawn on Unix searches the `PATH` from the child's environment
/// (rust-lang/rust#37519), so the stripped value set here is also the one the lookup uses and the
/// bundled copy cannot shadow the host one.
#[cfg(target_os = "linux")]
#[allow(clippy::disallowed_methods)] // the escape hatch the rest of the crate is barred from
pub fn host_command(program: &str) -> Command {
    let mut command = Command::new(program);

    let Some(app_dir) = app_dir() else {
        return command;
    };

    for key in BUNDLE_SEARCH_PATHS {
        match std::env::var_os(key).and_then(|value| strip_app_dir(&value, &app_dir)) {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }

    for key in BUNDLE_OVERRIDES {
        command.env_remove(key);
    }

    command
}

#[cfg(not(target_os = "linux"))]
#[allow(clippy::disallowed_methods)] // the escape hatch the rest of the crate is barred from
pub fn host_command(program: &str) -> std::process::Command {
    std::process::Command::new(program)
}

/// Points PipeWire at the SPA plugins and modules we ship, when there are any.
///
/// libpipewire resolves both directories through paths compiled in at build time, so a bundle
/// built on one distribution looks for them in a layout the user's machine does not have. Without
/// them the client cannot construct even a main loop, `check_pipewire` fails, and playback reports
/// itself unsupported. An existing value is left alone, and so is a bundle that ships no plugins,
/// which then falls back to the compiled-in paths exactly as before.
///
/// Locally built AppImages do not carry the CI overlay (its source paths are Debian specific) and
/// fall through the is_dir check below by design.
///
/// # Safety
///
/// Mutates the environment, so this must be called before any other thread exists.
#[cfg(target_os = "linux")]
pub unsafe fn redirect_bundled_pipewire() {
    let Some(app_dir) = app_dir() else {
        return;
    };

    for (key, relative) in [
        ("SPA_PLUGIN_DIR", BUNDLED_SPA_PLUGIN_DIR),
        ("PIPEWIRE_MODULE_DIR", BUNDLED_PIPEWIRE_MODULE_DIR),
    ] {
        // Empty counts as unset: a placeholder export must not suppress the redirect.
        if std::env::var_os(key).is_some_and(|value| !value.is_empty()) {
            continue;
        }

        let dir = app_dir.join(relative);
        if dir.is_dir() {
            unsafe { std::env::set_var(key, &dir) };
        }
    }
}

/// Opens a URL in the user's default browser. Blocks on the fork and the opener's filesystem
/// probing; from async code use [`open_url_detached`].
///
/// Only web and mail URLs are accepted. This is deliberately narrower than the opener plugin's
/// default ACL, which also allows `tel:`: nothing the app opens is a phone number, and this is the
/// shared primitive every caller funnels through, so the boundary lives here rather than being
/// re-checked per call site.
pub fn open_url(url: &str) -> Result<()> {
    let url = url::Url::parse(url).context("Failed to parse URL")?;

    if !matches!(url.scheme(), "http" | "https" | "mailto") {
        anyhow::bail!("Refusing to open URL with scheme {}", url.scheme());
    }

    // Hand over the parser's normalized form, never the caller's raw string: the WHATWG parser
    // strips control characters and tabs while classifying the scheme, so an input like
    // "\thttp:..." must not reach the opener in a shape the check above never saw.
    let url = url.as_str();

    #[cfg(target_os = "linux")]
    if app_dir().is_some() {
        return xdg_open(url);
    }

    tauri_plugin_opener::open_url(url, None::<&str>).context("Failed to open URL")
}

/// Opens a file or directory in the user's default application. Blocks like [`open_url`]; from
/// async code use [`open_path_detached`].
pub fn open_path(path: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    if app_dir().is_some() {
        return xdg_open(path);
    }

    tauri_plugin_opener::open_path(path, None::<&str>).context("Failed to open path")
}

/// Runs [`open_url`] on the blocking pool, keeping the fork and the opener's filesystem probing
/// off the shared tokio workers that also drive signaling.
pub async fn open_url_detached(url: String) -> Result<()> {
    tokio::task::spawn_blocking(move || open_url(&url))
        .await
        .context("URL opener task panicked")?
}

/// Runs [`open_path`] on the blocking pool; see [`open_url_detached`].
pub async fn open_path_detached(path: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || open_path(&path))
        .await
        .context("Path opener task panicked")?
}

/// Hands `target` to the host `xdg-open`. Takes an `OsStr` so non-UTF8 paths, which are legal on
/// Linux filesystems, pass through byte for byte instead of being mangled by a lossy conversion.
#[cfg(target_os = "linux")]
fn xdg_open(target: impl AsRef<std::ffi::OsStr>) -> Result<()> {
    let child = host_command("xdg-open")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to run xdg-open")?;

    reap_detached(child);

    Ok(())
}

/// Waits for a detached child on a background thread. The child returns as soon as it has handed
/// off its work, but leaving it unwaited would keep a zombie around for the app's lifetime.
#[cfg(target_os = "linux")]
pub fn reap_detached(mut child: std::process::Child) {
    std::thread::spawn(move || {
        if let Err(err) = child.wait() {
            log::warn!("Failed to reap detached child process: {err}");
        }
    });
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn strips_app_dir_entries() {
        let app_dir = Path::new("/tmp/.mount_vacs");
        let path = OsString::from("/tmp/.mount_vacs/usr/bin:/usr/bin:/tmp/.mount_vacs/usr/sbin");

        assert_eq!(
            strip_app_dir(&path, app_dir),
            Some(OsString::from("/usr/bin"))
        );
    }

    #[test]
    fn reports_nothing_left_to_keep() {
        let app_dir = Path::new("/tmp/.mount_vacs");
        let path = OsString::from("/tmp/.mount_vacs/usr/lib:/tmp/.mount_vacs/usr/lib64");

        assert_eq!(strip_app_dir(&path, app_dir), None);
    }

    #[test]
    fn keeps_paths_that_only_share_a_prefix() {
        let app_dir = Path::new("/tmp/.mount_vacs");
        let path = OsString::from("/tmp/.mount_vacs-other/usr/bin:/usr/bin");

        assert_eq!(
            strip_app_dir(&path, app_dir),
            Some(OsString::from("/tmp/.mount_vacs-other/usr/bin:/usr/bin"))
        );
    }

    #[test]
    fn keeps_non_utf8_entries_intact() {
        use std::os::unix::ffi::OsStringExt;

        let app_dir = Path::new("/tmp/.mount_vacs");
        let path = OsString::from_vec(b"/tmp/.mount_vacs/usr/bin:/opt/\xff/bin".to_vec());

        assert_eq!(
            strip_app_dir(&path, app_dir),
            Some(OsString::from_vec(b"/opt/\xff/bin".to_vec()))
        );
    }

    #[test]
    fn drops_empty_entries() {
        let app_dir = Path::new("/tmp/.mount_vacs");
        let path = OsString::from("/usr/bin::/usr/local/bin");

        assert_eq!(
            strip_app_dir(&path, app_dir),
            Some(OsString::from("/usr/bin:/usr/local/bin"))
        );
    }

    #[test]
    fn bundled_pipewire_dirs_match_the_appimage_overlay() {
        let overlay: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.appimage.conf.json")).unwrap();

        let files = overlay["bundle"]["linux"]["appimage"]["files"]
            .as_object()
            .expect("overlay must map AppDir destinations to source paths");

        for dir in [BUNDLED_SPA_PLUGIN_DIR, BUNDLED_PIPEWIRE_MODULE_DIR] {
            assert!(
                files.contains_key(dir),
                "redirect_bundled_pipewire expects the overlay to bundle {dir}"
            );
        }
    }
}
