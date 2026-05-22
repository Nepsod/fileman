use nptk::gpui::{px, App, TextRun, Window};
use nptk::theme::{theme_settings, ActiveTheme};
use nptk::ui::TextSize;

use crate::view_mode::ICON_LABEL_SHELL_HORIZONTAL_PADDING_PX;

pub const ICON_LABEL_MAX_LINES_UNSELECTED: usize = 2;
pub const ICON_LABEL_LINE_HEIGHT_FACTOR: f32 = 1.2;

#[derive(Debug, Clone, Copy)]
pub struct IconViewLabelLayout {
    pub width: f32,
    pub height: f32,
    pub fits_on_one_line: bool,
}

impl IconViewLabelLayout {
    pub fn fallback(max_width_px: f32) -> Self {
        Self {
            width: max_width_px + ICON_LABEL_SHELL_HORIZONTAL_PADDING_PX,
            height: crate::view_mode::ICON_LABEL_AREA_HEIGHT_PX,
            fits_on_one_line: false,
        }
    }
}

pub fn icon_view_label_layout(
    label: &str,
    max_width_px: f32,
    max_lines: Option<usize>,
    window: &Window,
    cx: &App,
) -> IconViewLabelLayout {
    let font_size = TextSize::XSmall.rems(cx).to_pixels(window.rem_size());
    let line_height = px(font_size.as_f32() * ICON_LABEL_LINE_HEIGHT_FACTOR);
    let line_height_px = line_height.as_f32();
    let font = theme_settings(cx).ui_font(cx).clone();
    let text_color = cx.theme().colors().text;
    let text_run = TextRun {
        len: label.len(),
        font,
        color: text_color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let text_runs = [text_run];

    let unwrapped = window
        .text_system()
        .layout_line(label, font_size, &text_runs, None);
    let natural_width = unwrapped.width.as_f32();

    if natural_width <= max_width_px {
        return IconViewLabelLayout {
            width: natural_width + ICON_LABEL_SHELL_HORIZONTAL_PADDING_PX,
            height: line_height_px,
            fits_on_one_line: true,
        };
    }

    let shaped_lines = window
        .text_system()
        .shape_text(
            label.into(),
            font_size,
            &text_runs,
            Some(px(max_width_px)),
            max_lines,
        )
        .unwrap_or_default();

    let mut width = 0.0_f32;
    let mut height = 0.0_f32;
    for line in shaped_lines {
        let line_size = line.size(line_height);
        width = width.max(line_size.width.as_f32());
        height += line_size.height.as_f32();
    }

    if width <= 0.0 {
        width = max_width_px;
    }
    if height <= 0.0 {
        height = line_height_px;
    }

    IconViewLabelLayout {
        width: width + ICON_LABEL_SHELL_HORIZONTAL_PADDING_PX,
        height,
        fits_on_one_line: false,
    }
}
