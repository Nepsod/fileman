use nptk::std::path::{Path, PathBuf};

use nptk::gpui::{px, App, IntoElement, Pixels, Point, Render, StyleRefinement, Window};
use nptk::theme::ActiveTheme;
use nptk::ui::prelude::*;

#[derive(Clone)]
pub struct DraggedFilePaths {
    pub paths: Vec<PathBuf>,
}

const MARQUEE_DRAG_THRESHOLD: f32 = 5.0;
pub const MARQUEE_EDGE_THRESHOLD: f32 = 24.0;
pub const MARQUEE_AUTOSCROLL_STEP: f32 = 16.0;

#[derive(Debug, Clone)]
pub struct MarqueeDrag {
    /// Window coordinates for drawing the rubber band on screen.
    pub origin: Point<Pixels>,
    pub pointer: Point<Pixels>,
    /// List content coordinates for selection; stable when the view scrolls.
    pub origin_list: Point<Pixels>,
    pub pointer_list: Point<Pixels>,
    pub extend_selection: bool,
    pub active: bool,
    /// Press started on empty space (gutters, in-cell padding, or list/table background).
    pub background_pointer_down: bool,
    /// Autoscroll while the pointer is near or past a viewport edge (-1, 0, 1).
    pub autoscroll_vertical: i8,
    pub autoscroll_horizontal: i8,
}

impl Render for DraggedFilePaths {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut nptk::gpui::Context<'_, Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (label, icon) = drag_label_and_icon(&self.paths);
        let count = self.paths.len();

        h_flex()
            .gap_2()
            .px_3()
            .py_2()
            .bg(colors.elevated_surface_background)
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .shadow_sm()
            .child(Icon::new(icon).size(IconSize::Small).color(Color::Default))
            .child(
                v_flex()
                    .gap_0p5()
                    .child(Label::new(label).size(LabelSize::Small))
                    .when(count > 1, |column| {
                        column.child(
                            Label::new(format!("{count} items"))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    }),
            )
    }
}

pub fn drop_target_style(mut style: StyleRefinement, cx: &App) -> StyleRefinement {
    let colors = cx.theme().colors();
    style.background = Some(colors.element_selection_background.into());
    style.border_color = Some(colors.border_selected);
    style
}

fn canonical_path_for_validation(path: &Path) -> PathBuf {
    nptk::std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn path_is_directory_for_validation(path: &Path) -> bool {
    nptk::std::fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

pub fn is_valid_drop_destination(sources: &[PathBuf], destination: &Path) -> bool {
    if !destination.is_dir() {
        return false;
    }

    let destination_canonical = canonical_path_for_validation(destination);
    sources.iter().all(|source| {
        let source_canonical = canonical_path_for_validation(source);
        let source_parent_canonical = source
            .parent()
            .map(canonical_path_for_validation);
        source_canonical != destination_canonical
            && source_parent_canonical.as_ref() != Some(&destination_canonical)
            && !(path_is_directory_for_validation(source)
                && destination_canonical.starts_with(&source_canonical))
    })
}

pub fn filter_paste_sources(
    sources: Vec<PathBuf>,
    destination: &Path,
    is_cut: bool,
) -> Vec<PathBuf> {
    filter_sources_for_destination(&sources, destination, is_cut)
}

pub fn filter_sources_for_destination(
    sources: &[PathBuf],
    destination: &Path,
    is_cut: bool,
) -> Vec<PathBuf> {
    sources
        .iter()
        .filter(|source| {
            if !is_valid_drop_destination(std::slice::from_ref(source), destination) {
                return false;
            }
            if is_cut && source.parent() == Some(destination) {
                return false;
            }
            true
        })
        .cloned()
        .collect()
}

pub fn marquee_exceeds_threshold(origin: Point<Pixels>, pointer: Point<Pixels>) -> bool {
    let delta_x = (pointer.x - origin.x).abs();
    let delta_y = (pointer.y - origin.y).abs();
    delta_x > px(MARQUEE_DRAG_THRESHOLD) || delta_y > px(MARQUEE_DRAG_THRESHOLD)
}

fn drag_label_and_icon(paths: &[PathBuf]) -> (SharedString, IconName) {
    let Some(path) = paths.first() else {
        return (SharedString::from("No items"), IconName::File);
    };

    let name = path
        .file_name()
        .and_then(|segment| segment.to_str())
        .unwrap_or("…");

    let icon = if path.is_dir() {
        IconName::Folder
    } else {
        IconName::File
    };

    (SharedString::from(name.to_string()), icon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nptk::std::fs;

    fn test_directory(label: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "fileman_drag_test_{label}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create drag test directory");
        directory
    }

    #[test]
    fn paste_into_descendant_directory_is_blocked() {
        let parent = test_directory("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).unwrap();

        let sources = vec![parent.clone()];
        assert!(!is_valid_drop_destination(&sources, &child));
        assert!(filter_paste_sources(sources, &child, false).is_empty());

        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn paste_into_same_directory_is_blocked() {
        let directory = test_directory("same");
        let sources = vec![directory.join("item.txt")];
        fs::write(&sources[0], b"x").unwrap();

        assert!(!is_valid_drop_destination(&sources, &directory));
        assert!(filter_paste_sources(sources, &directory, false).is_empty());

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn paste_into_sibling_directory_is_allowed() {
        let root = test_directory("siblings");
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();

        let sources = vec![source.clone()];
        assert!(is_valid_drop_destination(&sources, &destination));
        assert_eq!(
            filter_paste_sources(sources.clone(), &destination, false),
            sources
        );

        let _ = fs::remove_dir_all(&root);
    }
}
