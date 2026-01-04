use nptk::prelude::*;
use nptk_fileman_widgets::file_list::{FileList, FileListViewMode};
use std::path::PathBuf;

struct FileListApp;

impl Application for FileListApp {
    type State = ();

    fn build(_: AppContext, _: Self::State) -> impl Widget {
        let current_dir = std::env::current_dir().unwrap_or(PathBuf::from("."));
        FileList::new(current_dir)
    }
}

struct FileListGridIconsApp;

impl Application for FileListGridIconsApp {
    type State = ();

    fn build(_: AppContext, _: Self::State) -> impl Widget {
        let current_dir = std::env::current_dir().unwrap_or(PathBuf::from("."));
        FileList::new(current_dir).with_view_mode(FileListViewMode::Icon)
    }
}

struct FileListCompactApp;

impl Application for FileListCompactApp {
    type State = ();

    fn build(_: AppContext, _: Self::State) -> impl Widget {
        let current_dir = std::env::current_dir().unwrap_or(PathBuf::from("."));
        FileList::new(current_dir).with_view_mode(FileListViewMode::Compact)
    }
}
#[tokio::main]
async fn main() {
    // Check for environment variable to determine view mode
    let view_mode = std::env::var("NPTK_FILE_VIEW_MODE")
        .unwrap_or_default()
        .to_lowercase();

    if view_mode == "icon" {
        println!("Running File List in Grid Icons mode");
        FileListGridIconsApp.run(());
    } else if view_mode == "list" {
        println!("Running File List in List mode");
        FileListApp.run(());
    } else if view_mode == "compact" {
        println!("Running File List in Compact mode");
        FileListCompactApp.run(());
    } else {
        println!("Running File List in default List mode");
        FileListApp.run(());
    }
}
