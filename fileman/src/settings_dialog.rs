use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nalgebra::Vector2;
use nptk::core::app::info::AppInfo;
use nptk::core::signal::eval::EvalSignal;
use nptk::core::vgi::Graphics;
use nptk::core::widget::BoxedWidget;
use nptk::prelude::*;
use nptk::widgets::scroll_container::{
    ScrollContainer, ScrollDirection,
};
use nptk::widgets::standard_dialog::{
    open_popup_at, DialogButton, StandardModalLayout, StandardModalStyle,
    STANDARD_MODAL_COLUMN_GAP_Y,
};
use nptk::widgets::text_input::TextInput;
use nptk::widgets::toggle::Toggle;

use crate::config::{
    persist_user_settings, DeletePolicy, FilemanConfig, UserSettingsPersist,
};

/// Initial Configure Fileman window size (logical px); content scrolls if the window is smaller.
const CONFIGURE_FILEMAN_POPUP_SIZE: (u32, u32) = (540, 620);

/// Same delegation pattern as [FileList]: one layout child, `ScrollContainer` gets
/// `layout.children[0]` and uses `100% × 100%` of that node so Taffy keeps a bounded viewport.
struct SettingsFormScroll {
    scroll: ScrollContainer,
}

#[async_trait(?Send)]
impl Widget for SettingsFormScroll {
    fn layout_style(&self, context: &LayoutContext) -> StyleNode {
        StyleNode {
            style: LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                min_size: Vector2::new(Dimension::auto(), Dimension::length(0.0)),
                ..Default::default()
            },
            children: vec![self.scroll.layout_style(context)],
            measure_func: None,
        }
    }

    async fn update(
        &mut self,
        layout: &LayoutNode,
        context: AppContext,
        info: &mut AppInfo,
    ) -> Update {
        if layout.children.is_empty() {
            return Update::empty();
        }
        self.scroll
            .update(&layout.children[0], context, info)
            .await
    }

    fn render(
        &mut self,
        graphics: &mut dyn Graphics,
        layout: &LayoutNode,
        info: &mut AppInfo,
        context: AppContext,
    ) {
        if !layout.children.is_empty() {
            self.scroll
                .render(graphics, &layout.children[0], info, context);
        }
    }
}

fn settings_row_label_toggle(label: impl Into<String>, state: StateSignal<bool>) -> Container {
    let label = label.into();
    Container::new(vec![
        Box::new(Toggle::new(MaybeSignal::signal(Box::new(state.clone())))),
        Box::new(Text::new(label).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            ..Default::default()
        })),
    ])
    .with_layout_style(LayoutStyle {
        flex_direction: FlexDirection::Row,
        align_items: Some(AlignItems::Center),
        gap: Vector2::new(LengthPercentage::length(8.0), LengthPercentage::length(0.0)),
        size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
        ..Default::default()
    })
}

fn section_title(text: impl Into<String>) -> Text {
    Text::new(text.into()).with_font_size(13.0)
}

/// Opens the Configure Fileman preferences popup (writes `config.toml` on OK).
pub fn open_configure_fileman_popup(
    context: AppContext,
    config_path: Option<PathBuf>,
    show_hidden_files_signal: StateSignal<bool>,
    delete_policy: Arc<Mutex<DeletePolicy>>,
    terminal_command: Arc<Mutex<Option<String>>>,
) {
    let remember_initial = config_path
        .as_ref()
        .map(|p| {
            FilemanConfig::load_from_path(p)
                .window
                .remember_window_size
                .unwrap_or(true)
        })
        .unwrap_or(true);

    let show_hidden = StateSignal::new(*show_hidden_files_signal.get());
    let pol = delete_policy
        .lock()
        .ok()
        .map(|g| *g)
        .unwrap_or_default();
    let confirm_delete = StateSignal::new(pol.confirm_delete);
    let confirm_trash = StateSignal::new(pol.confirm_trash);
    let use_trash = StateSignal::new(pol.use_trash);
    let remember_size = StateSignal::new(remember_initial);

    let terminal_initial = terminal_command
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default();
    let terminal_input_signal = StateSignal::new(terminal_initial);

    let settings_sections: Vec<BoxedWidget> = vec![
        Box::new(section_title("Display")),
        Box::new(settings_row_label_toggle(
            "Show hidden files",
            show_hidden.clone(),
        )),
        Box::new(section_title("Behavior")),
        Box::new(settings_row_label_toggle(
            "Confirm before permanent delete",
            confirm_delete.clone(),
        )),
        Box::new(settings_row_label_toggle(
            "Confirm before moving to trash",
            confirm_trash.clone(),
        )),
        Box::new(settings_row_label_toggle("Use trash (Recycle bin)", use_trash.clone())),
        Box::new(section_title("System")),
        Box::new(Text::new("Terminal command (optional; empty = $TERMINAL / defaults)".to_string()).with_font_size(11.0)),
        Box::new(
            TextInput::new()
                .with_text_signal(terminal_input_signal.clone())
                .with_placeholder("e.g. kitty".to_string()),
        ),
        Box::new(section_title("Window")),
        Box::new(settings_row_label_toggle(
            "Remember window size on exit",
            remember_size.clone(),
        )),
    ];

    let settings_column = Container::new(settings_sections).with_layout_style(LayoutStyle {
        flex_direction: FlexDirection::Column,
        align_items: Some(AlignItems::Stretch),
        gap: Vector2::new(
            LengthPercentage::length(0.0),
            LengthPercentage::length(STANDARD_MODAL_COLUMN_GAP_Y),
        ),
        padding: Rect {
            left: LengthPercentage::length(STANDARD_MODAL_PADDING),
            right: LengthPercentage::length(STANDARD_MODAL_PADDING),
            top: LengthPercentage::length(STANDARD_MODAL_PADDING),
            bottom: LengthPercentage::length(STANDARD_MODAL_PADDING),
        },
        size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
        flex_shrink: 0.0,
        ..Default::default()
    });

    let mut scroll = ScrollContainer::new()
        .with_child(settings_column)
        .with_scroll_direction(ScrollDirection::Vertical);
    // Match ItemView path in FileList (`file_list.rs`): inner scroll fills its layout slot 100%×100%.
    scroll.set_layout_style(LayoutStyle {
        flex_direction: FlexDirection::Row,
        align_items: Some(AlignItems::FlexStart),
        size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
        ..Default::default()
    });

    let body: Vec<BoxedWidget> = vec![Box::new(SettingsFormScroll { scroll })];

    let path_save = config_path.clone();
    let show_main = show_hidden_files_signal.clone();
    let del_arc = delete_policy.clone();
    let term_arc = terminal_command.clone();
    let ctx_ok = context.clone();
    let ctx_cancel = context.clone();

    let dialog_content = StandardModalLayout::build_with_style(
        body,
        vec![
            DialogButton::new("Cancel", {
                context.callback(move || {
                    ctx_cancel.close_top_popup();
                    Update::DRAW
                })
            }),
            DialogButton::new("OK", {
                MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    let patch = UserSettingsPersist {
                        show_hidden: *show_hidden.get(),
                        confirm_delete: *confirm_delete.get(),
                        confirm_trash: *confirm_trash.get(),
                        use_trash: *use_trash.get(),
                        remember_window_size: *remember_size.get(),
                        terminal: (*terminal_input_signal.get()).clone(),
                    };
                    if let Some(ref p) = path_save {
                        if let Err(e) = persist_user_settings(p, &patch) {
                            log::warn!("fileman: failed to save settings: {}", e);
                        }
                    }
                    show_main.set(patch.show_hidden);
                    if let Ok(mut g) = del_arc.lock() {
                        *g = DeletePolicy {
                            confirm_delete: patch.confirm_delete,
                            confirm_trash: patch.confirm_trash,
                            use_trash: patch.use_trash,
                        };
                    }
                    let term = patch.terminal.trim();
                    if let Ok(mut g) = term_arc.lock() {
                        *g = if term.is_empty() {
                            None
                        } else {
                            Some(term.to_string())
                        };
                    }
                    ctx_ok.close_top_popup();
                    Update::DRAW | Update::LAYOUT
                })))
            }),
        ],
        StandardModalStyle {
            align_items: Some(AlignItems::Stretch),
            fill_viewport_height: true,
            ..StandardModalStyle::default()
        },
    );

    open_popup_at(
        &context,
        "Configure Fileman",
        CONFIGURE_FILEMAN_POPUP_SIZE,
        (280, 200),
        Box::new(dialog_content),
    );
}
