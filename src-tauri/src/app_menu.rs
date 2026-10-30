//! Application menu: Tauri's default layout, with an About entry that shows
//! our icon, version, license and website even in unbundled dev builds.

use tauri::image::Image;
use tauri::menu::{AboutMetadataBuilder, Menu, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

const WEBSITE: &str = "https://thorsten.github.io/sandbox-watcher/";

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let pkg = app.package_info();
    let config = app.config();
    // The Cargo package is `sandbox-watcher`; people should see the product name.
    let product_name = config
        .product_name
        .clone()
        .unwrap_or_else(|| "Sandbox Watcher".to_string());
    let icon = Image::from_bytes(include_bytes!("../icons/128x128@2x.png")).ok();
    let about = AboutMetadataBuilder::new()
        .name(Some(product_name.clone()))
        .version(Some(pkg.version.to_string()))
        .copyright(
            config
                .bundle
                .copyright
                .clone()
                .or_else(|| Some("© 2026 Thorsten Rinne".to_string())),
        )
        .authors(config.bundle.publisher.clone().map(|p| vec![p]))
        .license(config.bundle.license.clone())
        .website(Some(WEBSITE))
        .website_label(Some("thorsten.github.io/sandbox-watcher"))
        .comments(config.bundle.short_description.clone())
        .icon(icon)
        .build();

    let window_menu = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    #[cfg(not(target_os = "macos"))]
    let help_menu = Submenu::with_items(
        app,
        "Help",
        true,
        &[&PredefinedMenuItem::about(app, None, Some(about.clone()))?],
    )?;

    #[cfg(target_os = "macos")]
    let app_menu = Submenu::with_items(
        app,
        product_name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(app, Some(&format!("About {product_name}")), Some(about))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;

    #[cfg(not(target_os = "macos"))]
    let file_menu = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &PredefinedMenuItem::close_window(app, None)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;

    let edit_menu = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;

    Menu::with_items(
        app,
        &[
            #[cfg(target_os = "macos")]
            &app_menu,
            #[cfg(not(target_os = "macos"))]
            &file_menu,
            &edit_menu,
            &window_menu,
            #[cfg(not(target_os = "macos"))]
            &help_menu,
        ],
    )
}
