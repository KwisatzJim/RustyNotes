//! Launch the host browser without leaking AppImage library overrides into it.
#[cfg(any(target_os = "linux", test))]
fn host_browser_command(
    url: &str,
    appdir: &std::path::Path,
    path: &std::ffi::OsStr,
) -> std::io::Result<std::process::Command> {
    let host_paths = std::env::split_paths(path)
        .filter(|entry| entry.is_absolute() && !entry.starts_with(appdir));
    let mut command = std::process::Command::new("/usr/bin/xdg-open");
    command.arg(url).env(
        "PATH",
        std::env::join_paths(host_paths).map_err(std::io::Error::other)?,
    );
    for key in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "QT_PLUGIN_PATH",
        "QT_QPA_PLATFORM_PLUGIN_PATH",
    ] {
        command.env_remove(key);
    }
    Ok(command)
}

pub(crate) async fn open(app: &tauri::AppHandle, url: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if let Some(appdir) = std::env::var_os("APPDIR") {
        let path =
            std::env::var_os("PATH").unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin".into());
        let mut command = host_browser_command(&url, std::path::Path::new(&appdir), &path)
            .map_err(|_| "Could not prepare the system browser launcher.".to_string())?;
        command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let status = tokio::task::spawn_blocking(move || command.status())
            .await
            .map_err(|_| "Could not launch your browser.".to_string())?
            .map_err(|_| {
                "Could not run /usr/bin/xdg-open. Install xdg-utils and set a default browser."
                    .to_string()
            })?;
        return if status.success() {
            Ok(())
        } else {
            Err("The system browser launcher failed. Check your default browser in System Settings, then try again.".into())
        };
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "Could not open your browser. Please try again.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsStr, path::Path};

    #[test]
    fn host_launcher_preserves_url_and_removes_appimage_environment() {
        let url = "https://cloud.example/login?a=1&token=private%20value";
        let command = host_browser_command(
            url,
            Path::new("/tmp/app"),
            OsStr::new("/tmp/app/usr/bin:/usr/bin:/tmp/application/bin:relative:"),
        )
        .unwrap();
        assert_eq!(command.get_program(), "/usr/bin/xdg-open");
        assert_eq!(command.get_args().collect::<Vec<_>>(), [OsStr::new(url)]);
        let env = command
            .get_envs()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            env[OsStr::new("PATH")],
            Some(OsStr::new("/usr/bin:/tmp/application/bin"))
        );
        for key in [
            "LD_LIBRARY_PATH",
            "LD_PRELOAD",
            "QT_PLUGIN_PATH",
            "QT_QPA_PLATFORM_PLUGIN_PATH",
        ] {
            assert_eq!(env[OsStr::new(key)], None);
        }
        assert!(!env.contains_key(OsStr::new("DBUS_SESSION_BUS_ADDRESS")));
        assert!(!env.contains_key(OsStr::new("DISPLAY")));
    }
}
