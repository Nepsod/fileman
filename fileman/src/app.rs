use nptk::core::app::info::AppInfo;
use nptk::core::config::MayConfig;
use nptk::core::plugin::{Plugin, PluginManager};
use nptk::core::window::{ActiveEventLoop, Window};
use nptk::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use crate::config::FilemanConfig;
use crate::navigation::NavigationState;
use crate::window::build_window;

pub struct FilemanApp {
    may_config: MayConfig,
    window_config_path: Option<PathBuf>,
    persist_window_size: bool,
}

impl Application for FilemanApp {
    type State = AppState;

    fn build(context: AppContext, state: Self::State) -> impl Widget {
        build_window(context, state)
    }

    fn config(&self) -> MayConfig {
        self.may_config.clone()
    }

    fn plugins(&self) -> PluginManager {
        let mut plugins = PluginManager::new();
        plugins.register(WindowGeometryPersistPlugin {
            config_path: self.window_config_path.clone(),
            persist_when_remember: self.persist_window_size,
        });
        plugins
    }
}

impl FilemanApp {
    pub fn run(initial_path: PathBuf, fileman: FilemanConfig) {
        let window_config_path = FilemanConfig::config_file_path();
        let persist_window_size = fileman.window.remember_window_size.unwrap_or(true);
        let may_config = fileman.may_config();
        let navigation = Arc::new(Mutex::new(NavigationState::new(initial_path)));
        let state = AppState {
            navigation: navigation.clone(),
            fileman,
        };
        FilemanApp {
            may_config,
            window_config_path,
            persist_window_size,
        }
        .run(state);
    }
}

/// Saves `[Window]` size (and maximized flag) on shutdown when `RememberWindowSize` is true.
/// Uses [nptk::core::plugin::Plugin::on_shutting_down] so native Wayland (no main winit window,
/// `Update::EXIT` from the surface) is covered in addition to X11/winit close.
struct WindowGeometryPersistPlugin {
    config_path: Option<PathBuf>,
    persist_when_remember: bool,
}

impl Plugin for WindowGeometryPersistPlugin {
    fn name(&self) -> &'static str {
        "fileman-window-geometry-persist"
    }

    fn on_shutting_down(
        &mut self,
        config: &mut MayConfig,
        window: Option<&Arc<Window>>,
        info: &AppInfo,
        _event_loop: &ActiveEventLoop,
    ) {
        let remember = if let Some(ref path) = self.config_path {
            FilemanConfig::load_from_path(path)
                .window
                .remember_window_size
                .unwrap_or(true)
        } else {
            self.persist_when_remember
        };
        if !remember {
            return;
        }
        let Some(ref path) = self.config_path else {
            return;
        };
        let width = info.size.x.round() as i64;
        let height = info.size.y.round() as i64;
        let maximized = window
            .map(|w| w.is_maximized())
            .unwrap_or(config.window.maximized);
        if let Err(e) = crate::config::persist_window_geometry(path, width, height, maximized) {
            log::warn!("fileman: could not persist window geometry: {}", e);
        }
    }
}

pub struct AppState {
    pub navigation: Arc<Mutex<NavigationState>>,
    pub fileman: FilemanConfig,
}
