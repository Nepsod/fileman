use nptk::prelude::*;
use nptk_fileman_widgets::file_list::FileList;
use crate::app::AppState;
use nptk::core::signal::Signal;

pub fn build_window(_context: AppContext, state: AppState) -> impl Widget {
    let navigation = state.navigation.lock().unwrap();
    let initial_path = navigation.get_current_path();
    drop(navigation);

    // Create FileList widget
    let file_list = FileList::new(initial_path);

    // Create toolbar
    let toolbar = Toolbar::new()
        .with_child(ToolbarButton::new(Text::new("←".to_string())))
        .with_child(ToolbarButton::new(Text::new("→".to_string())))
        .with_child(ToolbarButton::new(Text::new("↑".to_string())))
        .with_separator()
        .with_child(ToolbarButton::new(Text::new("Home".to_string())));

    // Create location bar (simple TextInput)
    let location_bar = TextInput::new()
        .with_placeholder("Location...".to_string());

    // Create statusbar (simple container with text)
    let status_text = StateSignal::new("Ready".to_string());
    let statusbar = Container::new(vec![
        Box::new(Text::new(status_text.maybe())),
    ]).with_layout_style(LayoutStyle {
        size: Vector2::new(Dimension::percent(1.0), Dimension::length(24.0)),
        flex_direction: FlexDirection::Row,
        align_items: Some(AlignItems::Center),
        padding: nptk::core::layout::Rect {
            left: LengthPercentage::length(8.0),
            right: LengthPercentage::length(8.0),
            top: LengthPercentage::length(4.0),
            bottom: LengthPercentage::length(4.0),
        },
        ..Default::default()
    });

    // Create sidebar (placeholder for now - will implement properly)
    let sidebar = Container::new(vec![]).with_layout_style(LayoutStyle {
        size: Vector2::new(Dimension::length(200.0), Dimension::percent(1.0)),
        ..Default::default()
    });

    // Build main layout
    Container::new(vec![
        // Toolbar area
        Box::new(Container::new(vec![
            Box::new(toolbar),
            Box::new(location_bar),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        })),
        // Content area (sidebar + file list)
        Box::new(Container::new(vec![
            Box::new(sidebar),
            Box::new(file_list),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
            flex_direction: FlexDirection::Row,
            ..Default::default()
        })),
        // Statusbar
        Box::new(statusbar),
    ]).with_layout_style(LayoutStyle {
        size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
        flex_direction: FlexDirection::Column,
        ..Default::default()
    })
}
