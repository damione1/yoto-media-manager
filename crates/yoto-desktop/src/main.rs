#![cfg_attr(windows, windows_subsystem = "windows")]
gpui_kit::actions!(yoto, [Quit]);
#[macro_use]
mod i18n;
mod diagnostics;
mod image_http;
mod ui;
use gpui_kit::*;
use std::sync::Arc;
use yoto_infra::{Session, media::Tools};
fn main() {
    if let Ok(profile) = yoto_infra::default_profile()
        && let Err(error) = diagnostics::init(&profile, yoto_updater::CURRENT_VERSION)
    {
        eprintln!("Unable to initialize crash logging: {error}");
    }
    choose_language();
    if let Err(e) = launch() {
        diagnostics::record(format_args!("Application launch failed: {e}"));
        eprintln!("{e}");
        // A double-clicked app has no terminal: a launch failure must be visible on screen.
        let updating = std::env::args_os()
            .nth(1)
            .is_some_and(|arg| arg == "--apply-update");
        let detail = e
            .downcast_ref::<yoto_domain::Failure>()
            .map(ui::failure_text)
            .unwrap_or_else(|| e.to_string());
        rfd::MessageDialog::new()
            .set_title(ui::PRODUCT_NAME)
            .set_description(if updating {
                tr!(
                    "La mise à jour n’a pas pu être installée.\n\n{detail}",
                    detail = detail
                )
            } else if e.downcast_ref::<MissingTools>().is_some() {
                tr!(
                    "L’application n’a pas pu démarrer : certains de ses composants sont manquants ou abîmés.\n\nRéinstallez l’application pour les remettre en place."
                )
                .to_owned()
            } else {
                tr!(
                    "L’application n’a pas pu démarrer.\n\n{detail}\n\nSi le problème persiste, réinstallez l’application depuis GitHub Releases.",
                    detail = detail
                )
            })
            .set_level(rfd::MessageLevel::Error)
            .show();
        std::process::exit(1);
    }
    diagnostics::record("Application stopped normally");
}
/// The audio tools installed with the application are missing or do not match their
/// manifest: only a reinstall puts them back, so the parent is told so plainly.
#[derive(Debug)]
struct MissingTools(yoto_domain::Failure);
impl std::fmt::Display for MissingTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bundled tools unavailable: {}", self.0)
    }
}
impl std::error::Error for MissingTools {}
/// The interface language, before anything is shown: the one saved in the profile's
/// settings, or else the operating system's.
fn choose_language() {
    let saved = yoto_infra::default_profile()
        .ok()
        .and_then(|profile| yoto_infra::settings::Settings::load(&profile).language);
    i18n::set_language(i18n::resolve(
        saved.as_deref(),
        i18n::system_locale().as_deref(),
    ));
}
fn launch() -> Result<(), Box<dyn std::error::Error>> {
    if yoto_updater::apply_pending_update()? {
        return Ok(());
    }
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?,
    );
    let profile = yoto_infra::default_profile()?;
    let tools = Tools::bundled().map_err(MissingTools)?;
    let service = runtime.block_on(Session::connect(
        profile,
        tools,
        cfg!(debug_assertions) && std::env::args().any(|a| a == "--isolated"),
    ))?;
    {
        let _runtime = runtime.enter();
        service.start_tool_updates(!cfg!(debug_assertions));
    }
    let delta = runtime.block_on(service.interface_changes(vec![]))?;
    let mut snapshot = yoto_infra::Snapshot::default();
    let revisions = delta.apply(&mut snapshot);
    let snapshot = ui::PreparedSnapshot::new(snapshot, revisions);
    let shutdown = service.clone();
    gpui_kit::application()
        .with_http_client(Arc::new(image_http::ImageHttpClient::new(runtime.clone())?))
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            ui::install_theme(cx);
            cx.bind_keys([KeyBinding::new(
                if cfg!(target_os = "macos") {
                    "cmd-q"
                } else {
                    "alt-f4"
                },
                Quit,
                None,
            )]);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
            let (window, view) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(980.), px(640.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(ui::PRODUCT_NAME.into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    cx.new(|cx| {
                        ui::AppView::new(service.clone(), runtime.clone(), snapshot, window, cx)
                    })
                },
            )
            .expect("Open native window");
            // Quitting and closing the window first write pending playlist edits to Yoto.
            let quitting = view.clone();
            cx.on_action(move |_: &Quit, cx| {
                if quitting.update(cx, |view, cx| view.request_quit(cx)) {
                    cx.quit();
                }
            });
            let _ = window.update(cx, |_, window, cx| {
                window.on_window_should_close(cx, move |_, cx| {
                    view.update(cx, |view, cx| view.request_quit(cx))
                });
            });
            cx.activate(true);
        });
    shutdown.shutdown();

    Ok(())
}
