use std::path::PathBuf;
use std::sync::Arc;

use crate::config::FilemanConfig;
use crate::window::FilemanWindow;
use npio::backend::local::LocalBackend;
use npio::register_backend;
use nptk::gpui::{App, Bounds, WindowBounds, WindowOptions};
use nptk::gpui::{size, px, TitlebarOptions};
use nptk::gpui::AppContext;
use crate::menus::register_app_menu_handlers;

fn initial_path_from_args(config: &FilemanConfig) -> PathBuf {
    std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| config.folder_view.default_path.clone())
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn window_options(config: &FilemanConfig, cx: &App) -> WindowOptions {
    let size = size(
        px(config.window.last_window_width as f32),
        px(config.window.last_window_height as f32),
    );
    let bounds = Bounds::centered(None, size, cx);

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(config.window.window_title.clone().into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}


pub fn run() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,zbus=warn,ashpd=warn"),
    )
    .init();

    let config = FilemanConfig::load_or_create();
    let initial_path = initial_path_from_args(&config);
    let window_config = config.clone();

    let backend = Arc::new(LocalBackend::new());
    register_backend(backend);

    nptk::gpui_platform::application().run(move |cx: &mut App| {
        nptk::init(cx);
        register_app_menu_handlers(cx);
        cx.activate(true);

        cx.open_window(window_options(&window_config, cx), |_, cx| {
            cx.new(|cx| FilemanWindow::new(initial_path.clone(), cx))
        })
        .expect("failed to open file manager window");
    });
}
