use nptk::std::path::Path;
use nptk::std::sync::mpsc::Sender;

use notify::{
    event::{CreateKind, EventKind, ModifyKind, RemoveKind},
    Config, Event, RecommendedWatcher, RecursiveMode, Watcher,
};

pub fn create_directory_watcher(
    directory: &Path,
    notify_sender: Sender<()>,
) -> Option<RecommendedWatcher> {
    let mut watcher = RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            if let Ok(event) = result
                && directory_event_should_reload(&event)
            {
                let _ = notify_sender.send(());
            }
        },
        Config::default(),
    )
    .ok()?;
    watcher.watch(directory, RecursiveMode::NonRecursive).ok()?;
    Some(watcher)
}

fn directory_event_should_reload(event: &Event) -> bool {
    match event.kind {
        EventKind::Create(CreateKind::Any | CreateKind::File | CreateKind::Folder)
        | EventKind::Remove(RemoveKind::Any | RemoveKind::File | RemoveKind::Folder)
        | EventKind::Modify(ModifyKind::Name(_) | ModifyKind::Data(_) | ModifyKind::Metadata(_)) => {
            true
        }
        _ => false,
    }
}
