// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "macos")]
    import_login_shell_env();
    sandbox_watcher_lib::run()
}

/// Started from Finder or the Dock, the app only has launchd's minimal
/// environment; take `PATH` and the runtime host variables from the login
/// shell so runtime detection sees what the terminal sees.
#[cfg(target_os = "macos")]
fn import_login_shell_env() {
    use sandbox_core::shell_env;

    if !shell_env::started_outside_terminal(std::env::var("TERM").ok().as_deref()) {
        return;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let Some(vars) = shell_env::login_shell_env(&shell) else {
        return;
    };
    for (key, value) in shell_env::updates(&vars, |key| std::env::var(key).ok()) {
        // SAFETY: runs first in main, before Tauri or any other thread starts.
        unsafe { std::env::set_var(key, value) };
    }
}
