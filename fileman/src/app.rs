use nptk::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use crate::navigation::NavigationState;
use crate::window::build_window;

pub struct FilemanApp;

impl Application for FilemanApp {
    type State = AppState;

    fn build(context: AppContext, state: Self::State) -> impl Widget {
        build_window(context, state)
    }
}

impl FilemanApp {
    pub fn run(initial_path: PathBuf) {
        let navigation = Arc::new(Mutex::new(NavigationState::new(initial_path)));
        let state = AppState {
            navigation: navigation.clone(),
        };
        FilemanApp.run(state);
    }
}

pub struct AppState {
    pub navigation: Arc<Mutex<NavigationState>>,
}
