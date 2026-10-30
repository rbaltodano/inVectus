#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Launch Services gives the release app one identity and one WebKit profile. Cargo's
    // loose executable otherwise opens a second app with a separate recent-project list.
    #[cfg(all(target_os = "macos", not(debug_assertions)))]
    if launch_bundled_app() {
        return;
    }
    invectus_app_lib::run();
}

#[cfg(all(target_os = "macos", not(debug_assertions)))]
fn launch_bundled_app() -> bool {
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    if executable
        .ancestors()
        .any(|p| p.extension().is_some_and(|ext| ext == "app"))
    {
        return false;
    }
    let Some(parent) = executable.parent() else {
        return false;
    };
    let bundle = parent.join("bundle/macos/inVectus.app");
    if !bundle.join("Contents/MacOS/invectus-app").is_file() {
        return false;
    }
    let mut open = std::process::Command::new("/usr/bin/open");
    open.arg(bundle);
    for name in ["INVECTUS_MODELS_DIR", "INVECTUS_OFFLINE"] {
        if let Some(value) = std::env::var_os(name) {
            let mut variable = std::ffi::OsString::from(format!("{name}="));
            variable.push(value);
            open.arg("--env").arg(variable);
        }
    }
    open.status().is_ok_and(|status| status.success())
}
