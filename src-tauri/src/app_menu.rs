//! Application menu: Tauri's default layout, with an About entry that shows
//! our icon, version, license and website even in unbundled dev builds.

use sandbox_core::i18n::{Language, text};
use tauri::image::Image;
use tauri::menu::{AboutMetadataBuilder, Menu, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

const WEBSITE: &str = "https://thorsten.github.io/sandbox-watcher/";

pub fn build<R: Runtime>(app: &AppHandle<R>, lang: Language) -> tauri::Result<Menu<R>> {
    let item = |name: &str| text::menu_predefined(lang, name);
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
        text::menu_window(lang),
        true,
        &[
            &PredefinedMenuItem::minimize(app, item("minimize"))?,
            &PredefinedMenuItem::maximize(app, item("maximize"))?,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, item("close_window"))?,
        ],
    )?;

    #[cfg(not(target_os = "macos"))]
    let help_menu = Submenu::with_items(
        app,
        text::menu_help(lang),
        true,
        &[&PredefinedMenuItem::about(
            app,
            Some(&text::menu_about(lang, &product_name)),
            Some(about.clone()),
        )?],
    )?;

    #[cfg(target_os = "macos")]
    let app_menu = Submenu::with_items(
        app,
        product_name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(
                app,
                Some(&text::menu_about(lang, &product_name)),
                Some(about),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, item("services"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some(&text::menu_hide(lang, &product_name)))?,
            &PredefinedMenuItem::hide_others(app, item("hide_others"))?,
            &PredefinedMenuItem::show_all(app, item("show_all"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some(&text::menu_quit(lang, &product_name)))?,
        ],
    )?;

    #[cfg(not(target_os = "macos"))]
    let file_menu = Submenu::with_items(
        app,
        text::menu_file(lang),
        true,
        &[
            &PredefinedMenuItem::close_window(app, item("close_window"))?,
            &PredefinedMenuItem::quit(app, item("quit"))?,
        ],
    )?;

    let edit_menu = Submenu::with_items(
        app,
        text::menu_edit(lang),
        true,
        &[
            &PredefinedMenuItem::undo(app, item("undo"))?,
            &PredefinedMenuItem::redo(app, item("redo"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, item("cut"))?,
            &PredefinedMenuItem::copy(app, item("copy"))?,
            &PredefinedMenuItem::paste(app, item("paste"))?,
            &PredefinedMenuItem::select_all(app, item("select_all"))?,
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
