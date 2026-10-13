#![cfg_attr(windows, windows_subsystem = "windows")]
gpui_kit::actions!(yoto, [Quit]);
mod image_http;
mod ui;
use gpui_kit::*;
use std::sync::Arc;
use yoto_infra::{Session, media::Tools};
fn main() {
    if let Err(e) = launch() {
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
            .set_title("YotoMediaManager")
            .set_description(if updating {
                format!("La mise à jour n’a pas pu être installée.\n\n{detail}")
            } else {
                format!(
                    "L’application n’a pas pu démarrer.\n\n{detail}\n\nSi le problème persiste, réinstallez l’application depuis GitHub Releases."
                )
            })
            .set_level(rfd::MessageLevel::Error)
            .show();
        std::process::exit(1);
    }
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
    let tools = Tools::bundled()?;
    let service = runtime.block_on(Session::connect(
        profile,
        tools,
        cfg!(debug_assertions) && std::env::args().any(|a| a == "--isolated"),
    ))?;
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
            gpui_kit::component::Theme::sync_system_appearance(None, cx);
            cx.on_action(|_: &Quit, cx| cx.quit());
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
            let bounds = Bounds::centered(None, size(px(1220.), px(820.)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(980.), px(640.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Yoto — votre bibliothèque audio".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    gpui_kit::component::Theme::sync_system_appearance(Some(window), cx);
                    cx.new(|cx| {
                        ui::AppView::new(service.clone(), runtime.clone(), snapshot, window, cx)
                    })
                },
            )
            .expect("Open native window");
            cx.activate(true);
        });
    shutdown.shutdown();

    Ok(())
}
