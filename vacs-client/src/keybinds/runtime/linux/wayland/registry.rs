//! App id registration with the XDG desktop portal.
//!
//! xdg-desktop-portal identifies an unsandboxed process by the systemd unit its launcher
//! started it in, and the Global Shortcuts portal refuses to create a session for a process
//! without an app id. A packaged vacs started from its desktop entry gets `vacs`, a terminal
//! launch inherits the terminal's app id, and an AppImage double-clicked in a file manager
//! gets none at all: its unit is named after the file path, which matches no desktop entry.
//!
//! The `org.freedesktop.host.portal.Registry` interface (xdg-desktop-portal 1.22) lets a
//! host process name its own app id. The registration is bound to one D-Bus connection and
//! must be the first portal call on it, so the connection returned here has to be the one
//! the shortcuts proxy uses. The app id must match a desktop entry the portal can find.

use ashpd::zbus;
use std::collections::HashMap;

const PORTAL_DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REGISTRY_INTERFACE: &str = "org.freedesktop.host.portal.Registry";

/// Basename of the desktop entry the bundler installs, named after `productName`.
const DESKTOP_APP_ID: &str = "vacs";

/// Opens a portal connection registered under the first app id the portal accepts.
///
/// Falls back to an unregistered connection when the portal has no registry or rejects
/// every candidate. The launcher-derived app id then applies, as it did before.
pub async fn connect() -> zbus::Result<zbus::Connection> {
    let candidates = candidate_app_ids();

    for app_id in &candidates {
        // A rejected registration may still count as this connection's one attempt,
        // so every candidate gets a fresh connection.
        let connection = zbus::Connection::session().await?;

        match register(&connection, app_id).await {
            Ok(()) => {
                log::debug!("Registered with the desktop portal as app id {app_id}");
                return Ok(connection);
            }
            Err(zbus::Error::MethodError(name, message, _)) if is_missing_interface(&name) => {
                log::info!(
                    "Desktop portal has no app registry, relying on the launcher's app id: {}",
                    message.unwrap_or_default()
                );
                break;
            }
            Err(err) => {
                log::debug!("Desktop portal rejected app id {app_id}: {err}");
            }
        }
    }

    log::warn!(
        "Desktop portal accepted none of the app ids {candidates:?}, global shortcuts depend on the launcher's app id"
    );
    zbus::Connection::session().await
}

async fn register(connection: &zbus::Connection, app_id: &str) -> zbus::Result<()> {
    let options = HashMap::<&str, zbus::zvariant::Value<'_>>::new();
    connection
        .call_method(
            Some(PORTAL_DESTINATION),
            PORTAL_PATH,
            Some(REGISTRY_INTERFACE),
            "Register",
            &(app_id, options),
        )
        .await?;
    Ok(())
}

fn is_missing_interface(error_name: &zbus::names::ErrorName<'_>) -> bool {
    matches!(
        error_name.as_str(),
        "org.freedesktop.DBus.Error.UnknownMethod" | "org.freedesktop.DBus.Error.UnknownInterface"
    )
}

/// The packaged desktop entry first, then the URL handler entry that the deep-link plugin
/// writes to the user's applications directory on every launch. The latter is the only entry
/// describing a bare AppImage.
fn candidate_app_ids() -> Vec<String> {
    let mut candidates = vec![DESKTOP_APP_ID.to_string()];
    if let Some(handler) = url_handler_app_id() {
        candidates.push(handler);
    }
    candidates
}

/// Mirrors the file name tauri-plugin-deep-link derives for its desktop entry.
fn url_handler_app_id() -> Option<String> {
    let exe = tauri::utils::platform::current_exe().ok()?;
    let name = exe.file_name()?.to_string_lossy();
    Some(format!("{name}-handler"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_app_id_matches_the_bundled_desktop_entry() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../tauri.conf.json")).unwrap();
        assert_eq!(config["productName"], DESKTOP_APP_ID);
    }

    #[test]
    fn candidates_prefer_the_packaged_entry_over_the_url_handler() {
        let candidates = candidate_app_ids();
        assert_eq!(candidates[0], DESKTOP_APP_ID);
        assert_eq!(candidates.len(), 2);
        assert!(candidates[1].ends_with("-handler"));
        assert_ne!(candidates[1], "-handler");
    }

    #[test]
    fn only_dbus_lookup_failures_count_as_a_missing_registry() {
        let missing =
            zbus::names::ErrorName::try_from("org.freedesktop.DBus.Error.UnknownMethod").unwrap();
        let rejected =
            zbus::names::ErrorName::try_from("org.freedesktop.portal.Error.Failed").unwrap();
        assert!(is_missing_interface(&missing));
        assert!(!is_missing_interface(&rejected));
    }
}
