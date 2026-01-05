use nptk::prelude::*;
use crate::app::AppState;

pub fn build_toolbar(_context: &AppContext, _state: &AppState) -> impl Widget {
    Toolbar::new()
        .with_child(ToolbarButton::new(Text::new("←".to_string())))
        .with_child(ToolbarButton::new(Text::new("→".to_string())))
        .with_child(ToolbarButton::new(Text::new("↑".to_string())))
        .with_separator()
        .with_child(ToolbarButton::new(Text::new("Home".to_string())))
        .with_separator()
        .with_child(ToolbarButton::new(Text::new("New Folder".to_string())))
        .with_child(ToolbarButton::new(Text::new("Delete".to_string())))
}
