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

pub fn is_valid_drop_destination(sources: &[PathBuf], destination: &Path) -> bool {
    if !destination.is_dir() {
        return false;
    }

    sources.iter().all(|source| {
        source.as_path() != destination
            && !(source.is_dir() && destination.starts_with(source))
    })
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
