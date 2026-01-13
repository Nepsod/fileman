mod app;
mod navigation;
mod window;
mod toolbar;
mod menus;
mod operations;

use std::path::PathBuf;

#[tokio::main]
async fn main() {
    //env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Parse command line arguments
    let mut args = std::env::args().skip(1);
    let initial_location = args.next()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("/"));

    app::FilemanApp::run(initial_location);
}
