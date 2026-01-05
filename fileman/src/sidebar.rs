use nptk::prelude::*;
use crate::app::AppState;

pub fn build_sidebar(_context: &AppContext, _state: &AppState) -> impl Widget {
    // Placeholder sidebar - will implement properly with places/bookmarks later
    Container::new(vec![]).with_layout_style(LayoutStyle {
        size: Vector2::new(Dimension::length(200.0), Dimension::percent(1.0)),
        ..Default::default()
    })
}
