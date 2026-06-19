use nptk::std::path::{Path, PathBuf};

use nptk::gpui::{App, IntoElement, Pixels, Point, Render, SharedString, StyleRefinement, Window};
use nptk::theme::ActiveTheme;
use nptk::ui::prelude::*;

#[derive(Clone)]
pub struct DragSourceValidation {
    pub canonical: PathBuf,
    pub parent_canonical: Option<PathBuf>,
    pub is_directory: bool,
}

impl DragSourceValidation {
    fn from_path(path: &Path) -> Self {
        Self {
            canonical: canonical_path_for_validation(path),
            parent_canonical: path
                .parent()
                .map(canonical_path_for_validation),
            is_directory: path_is_directory_for_validation(path),
        }
    }
}

#[derive(Clone)]
pub struct DraggedFilePaths {
    pub paths: Vec<PathBuf>,
    pub sources: Vec<DragSourceValidation>,
}

impl DraggedFilePaths {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let sources = paths
            .iter()
            .map(|path| DragSourceValidation::from_path(path))
            .collect();
        Self { paths, sources }
    }
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

pub fn canonical_path_for_validation(path: &Path) -> PathBuf {
    nptk::std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn canonical_destination_path(destination: &Path) -> PathBuf {
    canonical_path_for_validation(destination)
}

fn path_is_directory_for_validation(path: &Path) -> bool {
    nptk::std::fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

pub fn is_valid_drop_destination_cached(
    sources: &[DragSourceValidation],
    destination_canonical: &Path,
) -> bool {
    sources.iter().all(|source| {
        source.canonical != destination_canonical
            && source.parent_canonical.as_deref() != Some(destination_canonical)
            && !(source.is_directory && destination_canonical.starts_with(&source.canonical))
    })
}

pub fn is_valid_drop_destination(sources: &[PathBuf], destination: &Path) -> bool {
    if !destination.is_dir() {
        return false;
    }

    let destination_canonical = canonical_destination_path(destination);
    let validations: Vec<DragSourceValidation> = sources
        .iter()
        .map(|path| DragSourceValidation::from_path(path))
        .collect();
    is_valid_drop_destination_cached(&validations, &destination_canonical)
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
    let dx = (pointer.x - origin.x).as_f32();
    let dy = (pointer.y - origin.y).as_f32();
    (dx * dx + dy * dy).sqrt() >= MARQUEE_DRAG_THRESHOLD
}

fn drag_label_and_icon(paths: &[PathBuf]) -> (SharedString, IconName) {
    if paths.len() == 1 {
        let path = &paths[0];
        let name = path
            .file_name()
            .and_then(|segment| segment.to_str())
            .unwrap_or("Item");
        let icon = if path_is_directory_for_validation(path) {
            IconName::Folder
        } else {
            IconName::File
        };
        (SharedString::from(name), icon)
    } else {
        (
            SharedString::from(format!("{} items", paths.len())),
            IconName::Folder,
        )
    }
}
