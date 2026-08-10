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
use std::path::Path;

#[cfg(target_os = "linux")]
use std::{
    ffi::OsString,
    path::PathBuf,
    process::{Command, Stdio},
};

/// Colon-separated search paths `AppRun` prepends AppDir entries to.
#[cfg(target_os = "linux")]
const BUNDLE_SEARCH_PATHS: &[&str] = &[
    "PATH",
    "LD_LIBRARY_PATH",
    "XDG_DATA_DIRS",
    "GTK_PATH",
    "QT_PLUGIN_PATH",
    "GST_PLUGIN_SYSTEM_PATH",
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
    "PYTHONHOME",
];

/// The AppDir we are running out of, if this process was launched from an AppImage.
#[cfg(target_os = "linux")]
fn app_dir() -> Option<PathBuf> {
    let app_dir = PathBuf::from(std::env::var_os("APPDIR")?);
    let exe = std::env::current_exe().ok()?;
    own_app_dir(app_dir, &exe)
}

/// A stale `APPDIR` inherited from another AppImage's environment must not turn a packaged
/// build into one, so the variable only counts when the running executable lives inside it.
#[cfg(target_os = "linux")]
fn own_app_dir(app_dir: PathBuf, exe: &Path) -> Option<PathBuf> {
    exe.starts_with(&app_dir).then_some(app_dir)
}

/// Drops every AppDir entry from a colon-separated search path, returning `None` if nothing is
/// left, so the caller can unset the variable instead of passing an empty one.
#[cfg(target_os = "linux")]
fn strip_app_dir(value: &OsString, app_dir: &Path) -> Option<OsString> {
    let value = value.to_string_lossy();

    let kept = value
        .split(':')
        .filter(|entry| !entry.is_empty() && !Path::new(entry).starts_with(app_dir))
        .collect::<Vec<_>>();

    (!kept.is_empty()).then(|| OsString::from(kept.join(":")))
}

/// Builds a [`Command`] for a host program, with the bundle removed from the environment it
/// inherits. The program is resolved against the cleaned `PATH` by hand: `execvp` searches the
/// parent's `PATH`, so setting it on the child is not enough to stop the bundled copy from winning.
#[cfg(target_os = "linux")]
pub fn host_command(program: &str) -> Command {
    let Some(app_dir) = app_dir() else {
        return Command::new(program);
    };

    let host_path = std::env::var_os("PATH").and_then(|path| strip_app_dir(&path, &app_dir));

    let resolved = host_path
        .as_ref()
        .and_then(|path| resolve_in_path(path, program))
        .unwrap_or_else(|| PathBuf::from(program));

    let mut command = Command::new(resolved);

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
pub fn host_command(program: &str) -> std::process::Command {
    std::process::Command::new(program)
}

/// Finds the first executable named `program` in a colon-separated search path.
#[cfg(target_os = "linux")]
fn resolve_in_path(path: &OsString, program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    std::env::split_paths(path)
        .map(|dir| dir.join(program))
        .find(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

/// Opens a URL in the user's default browser.
pub fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    if app_dir().is_some() {
        return xdg_open(url);
    }

    tauri_plugin_opener::open_url(url, None::<&str>).context("Failed to open URL")
}

/// Opens a file or directory in the user's default application.
pub fn open_path(path: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    if app_dir().is_some() {
        return xdg_open(&path.to_string_lossy());
    }

    tauri_plugin_opener::open_path(path, None::<&str>).context("Failed to open path")
}

/// Hands `target` to the host `xdg-open`. Reaped on a detached thread: `xdg-open` returns as soon
/// as it has handed off, but leaving the child unwaited would keep a zombie for the app's lifetime.
#[cfg(target_os = "linux")]
fn xdg_open(target: &str) -> Result<()> {
    let mut child = host_command("xdg-open")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to run xdg-open")?;

    std::thread::spawn(move || {
        if let Err(err) = child.wait() {
            log::warn!("Failed to reap xdg-open: {err}");
        }
    });

    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn ignores_an_app_dir_the_executable_does_not_live_in() {
        let app_dir = PathBuf::from("/tmp/.mount_other");
        assert_eq!(
            own_app_dir(
                app_dir.clone(),
                Path::new("/tmp/.mount_other/usr/bin/vacs-client")
            ),
            Some(app_dir.clone())
        );
        assert_eq!(
            own_app_dir(app_dir, Path::new("/usr/bin/vacs-client")),
            None
        );
    }

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
    fn resolves_the_first_executable_on_the_path() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join("vacs-external-resolve");
        let bundled = root.join("appdir");
        let host = root.join("host");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&host).unwrap();

        // Only readable in the bundle, executable on the host: the host copy must win even though
        // the bundle comes first on the path.
        std::fs::write(bundled.join("xdg-open"), "").unwrap();
        std::fs::set_permissions(bundled.join("xdg-open"), PermissionsExt::from_mode(0o644))
            .unwrap();
        std::fs::write(host.join("xdg-open"), "").unwrap();
        std::fs::set_permissions(host.join("xdg-open"), PermissionsExt::from_mode(0o755)).unwrap();

        let path = OsString::from(format!("{}:{}", bundled.display(), host.display()));

        assert_eq!(
            resolve_in_path(&path, "xdg-open"),
            Some(host.join("xdg-open"))
        );
        assert_eq!(resolve_in_path(&path, "kde-open"), None);

        std::fs::remove_dir_all(&root).ok();
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
}
