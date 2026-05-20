use nptk::std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BreadcrumbSegment {
    pub label: String,
    pub path: PathBuf,
    pub clickable: bool,
}

pub fn breadcrumb_segments(path: &Path) -> Vec<BreadcrumbSegment> {
    let mut segments = Vec::new();
    let mut accumulated = PathBuf::new();

    if path.has_root() {
        let root = PathBuf::from("/");
        segments.push(BreadcrumbSegment {
            label: "/".to_string(),
            path: root.clone(),
            clickable: true,
        });
        accumulated = root;
    }

    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            accumulated.push(name);
            segments.push(BreadcrumbSegment {
                label: name.to_string_lossy().into_owned(),
                path: accumulated.clone(),
                clickable: true,
            });
        }
    }

    if let Some(last) = segments.last_mut() {
        last.clickable = false;
    }

    segments
}
