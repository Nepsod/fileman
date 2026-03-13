use nptk::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use async_trait::async_trait;
use nptk::core::app::focus::{FocusBounds, FocusId, FocusProperties, FocusState, FocusableWidget};
use nptk::core::app::info::AppInfo;
use nptk::core::signal::state::StateSignal;
use nptk::core::signal::Signal;
use nptk::core::theme::ColorRole;
use nptk::core::vg::kurbo::{Affine, RoundedRect, Stroke};
use nptk::core::vg::peniko::Brush;
use nptk::core::vgi::Graphics;
use nptk::widgets::breadcrumbs::{Breadcrumbs, BreadcrumbItem};
use nptk::widgets::icon::Icon;
use nptk::widgets::text_input::TextInput;

use crate::file_list::SearchScope;

const MAX_SEARCH_HISTORY: usize = 10;

/// A signal that runs its closure on every `get()` call.
#[derive(Clone)]
struct FuncSignal<F, T> {
    f: F,
    _marker: std::marker::PhantomData<T>,
}

impl<F, T> FuncSignal<F, T> {
    fn new(f: F) -> Self {
        Self {
            f,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<F, T> Signal<T> for FuncSignal<F, T>
where
    F: Fn() -> T + Send + Sync + Clone + 'static,
    T: Send + Sync + 'static + Clone,
{
    fn get(&self) -> nptk::core::reference::Ref<'_, T> {
        nptk::core::reference::Ref::Owned((self.f)())
    }

    fn set_value(&self, _value: T) {}

    fn listen(&self, _listener: nptk::core::signal::Listener<T>) {}

    fn notify(&self) {}

    fn dyn_clone(&self) -> nptk::core::signal::BoxedSignal<T> {
        Box::new((*self).clone())
    }
}

struct SearchToggleVisual {
    inner: Container,
    active: StateSignal<bool>,
    layout_style: MaybeSignal<LayoutStyle>,
}

impl SearchToggleVisual {
    fn new(active: StateSignal<bool>) -> Self {
        let content = Container::new(vec![
            Box::new(Icon::new("system-search", 14, None)),
            Box::new(Text::new("Search".to_string()).with_font_size(13.0)),
        ])
        .with_layout_style(LayoutStyle {
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Vector2::new(LengthPercentage::length(4.0), LengthPercentage::length(0.0)),
            ..Default::default()
        });

        Self {
            inner: content,
            active,
            layout_style: LayoutStyle {
                padding: Rect {
                    left: LengthPercentage::length(2.0),
                    right: LengthPercentage::length(2.0),
                    top: LengthPercentage::length(4.0),
                    bottom: LengthPercentage::length(4.0),
                },
                ..Default::default()
            }
            .into(),
        }
    }
}

impl WidgetLayoutExt for SearchToggleVisual {
    fn set_layout_style(&mut self, layout_style: impl Into<MaybeSignal<LayoutStyle>>) {
        self.layout_style = layout_style.into();
    }
}

#[async_trait(?Send)]
impl Widget for SearchToggleVisual {
    fn render(
        &mut self,
        graphics: &mut dyn Graphics,
        layout_node: &LayoutNode,
        info: &mut AppInfo,
        context: AppContext,
    ) {
        let palette = context.palette();
        let button_bounds = RoundedRect::from_rect(
            nptk::core::vg::kurbo::Rect::new(
                layout_node.layout.location.x as f64,
                layout_node.layout.location.y as f64,
                (layout_node.layout.location.x + layout_node.layout.size.width) as f64,
                (layout_node.layout.location.y + layout_node.layout.size.height) as f64,
            ),
            nptk::core::vg::kurbo::RoundedRectRadii::from_single_radius(8.0),
        );

        graphics.fill_rounded_rect(
            Affine::IDENTITY,
            &Brush::Solid(palette.color(ColorRole::Button)),
            None,
            button_bounds,
        );

        let stroke_color = if *self.active.get() {
            palette.color(ColorRole::ThreedShadow1)
        } else {
            palette.color(ColorRole::ThreedHighlight)
        };
        graphics.stroke_rounded_rect(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            &Brush::Solid(stroke_color),
            None,
            button_bounds,
        );

        if let Some(child_layout) = layout_node.children.first() {
            self.inner.render(graphics, child_layout, info, context);
        }
    }

    fn layout_style(&self, context: &nptk::core::layout::LayoutContext) -> nptk::core::layout::StyleNode {
        StyleNode {
            style: self.layout_style.get().clone(),
            children: vec![self.inner.layout_style(context)],
            measure_func: None,
        }
    }

    async fn update(
        &mut self,
        layout: &nptk::core::layout::LayoutNode,
        context: nptk::core::app::context::AppContext,
        info: &mut nptk::core::app::info::AppInfo,
    ) -> nptk::core::app::update::Update {
        if let Some(child_layout) = layout.children.first() {
            self.inner.update(child_layout, context, info).await
        } else {
            Update::empty()
        }
    }
}

/// Helper function to convert PathBuf to breadcrumb items
fn path_to_breadcrumb_items(path: &std::path::Path) -> Vec<BreadcrumbItem> {
    let mut items = Vec::new();
    let mut current_path = PathBuf::new();
    
    // Handle root path
    if path.has_root() {
        items.push(BreadcrumbItem::new("/").with_id("/".to_string()));
        current_path.push("/");
    }
    
    // Add each component
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            current_path.push(name);
            let label = name.to_string_lossy().to_string();
            let id = current_path.to_string_lossy().to_string();
            items.push(BreadcrumbItem::new(label).with_id(id));
        }
    }
    
    // Last item is not clickable (current location)
    if let Some(last) = items.last_mut() {
        last.clickable = false;
    }
    
    items
}

/// A reusable location bar widget combining breadcrumbs and text input.
pub struct FileLocationBar {
    inner: Container,
    current_path: StateSignal<PathBuf>,
    breadcrumb_items: StateSignal<Vec<BreadcrumbItem>>,
    text_value: StateSignal<String>,
    last_synced_path: PathBuf,
    on_navigate: Option<Box<dyn Fn(PathBuf) -> Update + Send + Sync>>,
    signals_hooked: bool,
    internal_rx: Option<mpsc::UnboundedReceiver<PathBuf>>,
    clear_focus_rx: Option<mpsc::UnboundedReceiver<()>>,
    request_focus_rx: Option<mpsc::UnboundedReceiver<()>>,
    focus_rx: Option<mpsc::UnboundedReceiver<()>>,
    activate_search_rx: Option<mpsc::UnboundedReceiver<()>>,
    toggle_search_rx: Option<mpsc::UnboundedReceiver<bool>>,
    text_input_focus_id: FocusId,
    bar_focus_id: FocusId,
    // Search
    search_query: StateSignal<String>,
    search_active: StateSignal<bool>,
    search_scope: StateSignal<SearchScope>,
    search_history: StateSignal<Vec<String>>,
    search_input_focus_id: Option<FocusId>,
    edit_mode: StateSignal<bool>,
}

impl FileLocationBar {
    /// Create a new location bar. `search_query` is shared with the file list so that typing in the
    /// search field filters the list live (Dolphin-style).
    /// `search_deferred_tx`: when set, search text is sent here instead of writing to the signal (avoids freeze when typing).
    pub fn new(
        current_path: StateSignal<PathBuf>,
        search_query: StateSignal<String>,
        search_deferred_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Self {
        let path_val = (*current_path.get()).clone();
        let initial_items = path_to_breadcrumb_items(&path_val);
        let breadcrumb_items = StateSignal::new(initial_items);
        let text_value = StateSignal::new(path_val.to_string_lossy().to_string());
        
        let edit_mode = StateSignal::new(false);
        
        let search_active = StateSignal::new(false);
        let search_scope = StateSignal::new(SearchScope::CurrentFolder);
        let search_history = StateSignal::new(Vec::new());
        
        let (tx, rx) = mpsc::unbounded_channel();
        let tx = Arc::new(tx);
        
        let (clear_focus_tx, clear_focus_rx) = mpsc::unbounded_channel();
        let clear_focus_tx = Arc::new(clear_focus_tx);
        
        // Breadcrumbs
        let tx_crumb = tx.clone();
        let breadcrumbs = Breadcrumbs::new()
            .with_items_signal(breadcrumb_items.clone())
            .with_on_click(move |item| {
                if let Some(id) = &item.id {
                    let path = PathBuf::from(id);
                     let _ = tx_crumb.send(path);
                     return Update::DRAW;
                }
                Update::empty()
            })
            .with_layout_style(MaybeSignal::signal(Box::new(edit_mode.map(|edit| {
                nptk::prelude::Ref::Owned(LayoutStyle {
                    size: Vector2::new(Dimension::auto(), Dimension::auto()),
                    display: if *edit { Display::None } else { Display::Flex },
                    ..Default::default()
                })
            }))));

        // Clickable empty space
        use nptk::widgets::gesture_detector::GestureDetector;
        
        let (request_focus_tx, request_focus_rx) = mpsc::unbounded_channel();
        let request_focus_tx = Arc::new(request_focus_tx);

        let (toggle_search_tx, toggle_search_rx) = mpsc::unbounded_channel();

        let click_edit_mode = edit_mode.clone();
        let current_path_clone = current_path.clone();
        let text_value_clone = text_value.clone();
        let tx_request_focus = request_focus_tx.clone();
        
        let empty_space = Container::new(vec![Box::new(
            GestureDetector::new(
                Container::new(vec![]).with_layout_style(LayoutStyle {
                    size: Vector2::new(Dimension::percent(1.0), Dimension::percent(1.0)),
                    flex_grow: 1.0,
                    ..Default::default()
                })
            )
            .with_on_press(MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                if !*click_edit_mode.get() {
                    click_edit_mode.set(true);
                    
                    // Sync current path to text before editing
                    let current = (*current_path_clone.get()).clone();
                    text_value_clone.set(current.to_string_lossy().to_string());
                    
                    let _ = tx_request_focus.send(());
                }
                
                Update::LAYOUT | Update::DRAW
            }))))
        )])
        .with_layout_style(MaybeSignal::signal(Box::new(edit_mode.map(|edit| {
            nptk::prelude::Ref::Owned(LayoutStyle {
                size: Vector2::new(Dimension::percent(1.0), Dimension::length(30.0)),
                flex_grow: 1.0,
                display: if *edit { Display::None } else { Display::Flex },
                ..Default::default()
            })
        }))));

        // Text Input
        let tx_submit = tx.clone();
        let submit_edit_mode = edit_mode.clone();
        let submit_text_value = text_value.clone();
        
        let cancel_edit_mode = edit_mode.clone();
        let cancel_current_path = current_path.clone();
        let cancel_text_value = text_value.clone();

        let focus_lost_edit_mode = edit_mode.clone();
        let focus_lost_current_path = current_path.clone();
        let focus_lost_text_value = text_value.clone();
        
        let tx_clear_focus1 = clear_focus_tx.clone();
        let tx_clear_focus2 = clear_focus_tx.clone();

        let text_input = TextInput::new()
            .with_text_signal(text_value.clone())
            .with_placeholder("Path...".to_string())
            .with_layout_style(MaybeSignal::signal(Box::new(edit_mode.map(|edit| {
                nptk::prelude::Ref::Owned(LayoutStyle {
                    size: Vector2::new(Dimension::percent(1.0), Dimension::length(30.0)),
                    flex_grow: 1.0,
                    display: if *edit { Display::Flex } else { Display::None },
                    ..Default::default()
                })
            }))))
            .with_on_submit(MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                  let path = PathBuf::from(&*submit_text_value.get());
                 let _ = tx_submit.send(path);
                 let _ = tx_clear_focus1.send(());
                 submit_edit_mode.set(false);
                 Update::LAYOUT | Update::DRAW
            }))))
            .with_on_escape(MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                  cancel_edit_mode.set(false);
                 // Revert to original path
                 let current = (*cancel_current_path.get()).clone();
                 cancel_text_value.set(current.to_string_lossy().to_string());
                 let _ = tx_clear_focus2.send(());
                 Update::LAYOUT | Update::DRAW
            }))))
            .with_on_focus_lost(MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                 focus_lost_edit_mode.set(false);
                 // Revert to original path
                 let current = (*focus_lost_current_path.get()).clone();
                 focus_lost_text_value.set(current.to_string_lossy().to_string());
                 Update::LAYOUT | Update::DRAW
            }))))
            .with_layout_style(MaybeSignal::signal(Box::new(edit_mode.map(|edit| {
                nptk::prelude::Ref::Owned(LayoutStyle {
                    size: Vector2::new(Dimension::auto(), Dimension::length(30.0)),
                    flex_grow: 1.0, 
                    min_size: Vector2::new(Dimension::length(200.0), Dimension::auto()),
                    display: if *edit { Display::Flex } else { Display::None },
                    ..Default::default()
                })
            }))));
        
        let focus_id = text_input.focus_id();
        let bar_focus_id = FocusId::new();

        // Search UI - hidden when in path edit mode (search only when showing breadcrumbs/empty space)
        let search_query_clone = search_query.clone();
        let search_active_for_style = search_active.clone();
        let search_active_for_column = search_active.clone();
        let search_active_for_scope = search_active.clone();

        // Search field visibility depends primarily on search_active; edit_mode hides it
        let edit_mode_for_input_style = edit_mode.clone();
        let search_input_style = search_active_for_style.map(move |active| {
            let edit = *edit_mode_for_input_style.get();
            nptk::prelude::Ref::Owned(LayoutStyle {
                size: Vector2::new(Dimension::auto(), Dimension::length(30.0)),
                flex_direction: FlexDirection::Row,
                gap: Vector2::new(LengthPercentage::length(4.0), LengthPercentage::length(0.0)),
                align_items: Some(AlignItems::Center),
                display: if edit { Display::None } else if *active { Display::Flex } else { Display::None },
                ..Default::default()
            })
        });

        let search_query_escape = search_query.clone();
        let search_history_for_submit = search_history.clone();
        let search_query_for_submit = search_query.clone();
        let mut search_input = TextInput::new()
            .with_text_signal(search_query.clone())
            .with_placeholder("Search in this folder…".to_string())
            .with_on_escape(MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                search_query_escape.set(String::new());
                Update::LAYOUT | Update::DRAW
            }))))
            .with_on_submit(MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                let q = search_query_for_submit.get().trim().to_string();
                if !q.is_empty() {
                    let mut h = search_history_for_submit.get().clone();
                    h.retain(|x| x != &q);
                    h.insert(0, q);
                    if h.len() > MAX_SEARCH_HISTORY {
                        h.truncate(MAX_SEARCH_HISTORY);
                    }
                    search_history_for_submit.set(h);
                }
                Update::LAYOUT | Update::DRAW
            }))))
            .with_layout_style(LayoutStyle {
                size: Vector2::new(Dimension::length(200.0), Dimension::length(30.0)),
                ..Default::default()
            });
        if let Some(tx) = search_deferred_tx {
            search_input = search_input.with_deferred_signal_sender(tx);
        }

        let search_input_focus_id = Some(search_input.focus_id());

        let search_clear_btn = {
            let query = search_query_clone.clone();
            let btn = Button::new(Text::new("×".to_string()))
                .with_layout_style(MaybeSignal::signal(Box::new(search_query.map(move |q| {
                    nptk::prelude::Ref::Owned(LayoutStyle {
                        display: if q.is_empty() { Display::None } else { Display::Flex },
                        size: Vector2::new(Dimension::length(24.0), Dimension::length(24.0)),
                        ..Default::default()
                    })
                }))));
            GestureDetector::new(btn)
                .with_on_press(MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    query.set(String::new());
                    Update::LAYOUT | Update::DRAW
                }))))
        };

        let search_field_container = Container::new(vec![
            Box::new(search_input),
            Box::new(search_clear_btn),
        ])
        .with_layout_style(MaybeSignal::signal(Box::new(search_input_style)));

        let search_active_for_history = search_active.clone();
        let search_history_for_style = search_history.clone();
        let edit_mode_for_history_style = edit_mode.clone();
        let history_dropdown_style = search_active_for_history.map(move |active| {
            let edit = *edit_mode_for_history_style.get();
            let hist = search_history_for_style.get();
            nptk::prelude::Ref::Owned(LayoutStyle {
                size: Vector2::new(Dimension::length(200.0), Dimension::auto()),
                flex_direction: FlexDirection::Column,
                gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(2.0)),
                display: if edit || !*active || hist.is_empty() {
                    Display::None
                } else {
                    Display::Flex
                },
                ..Default::default()
            })
        });
        const HISTORY_BUTTON_COUNT: usize = 5;
        let mut history_buttons: Vec<nptk::core::widget::BoxedWidget> =
            Vec::with_capacity(HISTORY_BUTTON_COUNT);
        for i in 0..HISTORY_BUTTON_COUNT {
            let hist_clone = search_history.clone();
            let idx = i;
            let btn = Button::new(Text::new(MaybeSignal::signal(Box::new(hist_clone.clone().map(
                move |h| nptk::prelude::Ref::Owned(h.get(idx).cloned().unwrap_or_default()),
            )))))
            .with_layout_style(MaybeSignal::signal(Box::new(hist_clone.map(move |h| {
                nptk::prelude::Ref::Owned(LayoutStyle {
                    display: if idx < h.len() { Display::Flex } else { Display::None },
                    size: Vector2::new(Dimension::length(200.0), Dimension::length(22.0)),
                    ..Default::default()
                })
            }))));
            let hist_for_press = search_history.clone();
            let query_for_press = search_query.clone();
            history_buttons.push(Box::new(GestureDetector::new(btn).with_on_press(
                MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                    let h = hist_for_press.get().clone();
                    if let Some(q) = h.get(idx) {
                        query_for_press.set(q.clone());
                    }
                    Update::LAYOUT | Update::DRAW
                }))),
            )));
        }
        let history_dropdown = Container::new(history_buttons)
            .with_layout_style(MaybeSignal::signal(Box::new(history_dropdown_style)));

        let edit_mode_for_column_style = edit_mode.clone();
        let search_column_style = search_active_for_column.map(move |active| {
            let edit = *edit_mode_for_column_style.get();
            nptk::prelude::Ref::Owned(LayoutStyle {
                size: Vector2::new(Dimension::auto(), Dimension::auto()),
                flex_direction: FlexDirection::Column,
                gap: Vector2::new(LengthPercentage::length(0.0), LengthPercentage::length(2.0)),
                align_items: Some(AlignItems::FlexStart),
                display: if edit { Display::None } else if *active { Display::Flex } else { Display::None },
                ..Default::default()
            })
        });
        let search_column = Container::new(vec![
            Box::new(search_field_container),
            Box::new(history_dropdown),
        ])
        .with_layout_style(MaybeSignal::signal(Box::new(search_column_style)));

        let edit_mode_for_scope_style = edit_mode.clone();
        let scope_style = search_active_for_scope.map(move |active| {
            let edit = *edit_mode_for_scope_style.get();
            nptk::prelude::Ref::Owned(LayoutStyle {
                size: Vector2::new(Dimension::auto(), Dimension::length(30.0)),
                flex_direction: FlexDirection::Row,
                gap: Vector2::new(LengthPercentage::length(2.0), LengthPercentage::length(0.0)),
                align_items: Some(AlignItems::Center),
                display: if edit { Display::None } else if *active { Display::Flex } else { Display::None },
                ..Default::default()
            })
        });
        let scope_for_folder = search_scope.clone();
        let scope_for_subfolders = search_scope.clone();
        let btn_folder = Button::new(Text::new("Folder".to_string()))
            .with_layout_style(LayoutStyle {
                size: Vector2::new(Dimension::length(56.0), Dimension::length(24.0)),
                ..Default::default()
            });
        let btn_subfolders = Button::new(Text::new("Subfolders".to_string()))
            .with_layout_style(LayoutStyle {
                size: Vector2::new(Dimension::length(72.0), Dimension::length(24.0)),
                ..Default::default()
            });
        let scope_container = Container::new(vec![
            Box::new(GestureDetector::new(btn_folder).with_on_press(MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                scope_for_folder.set(SearchScope::CurrentFolder);
                Update::LAYOUT | Update::DRAW
            }))))),
            Box::new(GestureDetector::new(btn_subfolders).with_on_press(MaybeSignal::signal(Box::new(EvalSignal::new(move || {
                scope_for_subfolders.set(SearchScope::FolderAndSubfolders);
                Update::LAYOUT | Update::DRAW
            }))))),
        ])
        .with_layout_style(MaybeSignal::signal(Box::new(scope_style)));

        // Search toggle button: smaller, always labeled "Search", with persistent pressed state while active.
        let search_toggle = {
            let active = search_active.clone();
            let toggle_tx = toggle_search_tx.clone();
            let visual_button = SearchToggleVisual::new(search_active.clone()).with_layout_style(
                MaybeSignal::signal(Box::new(edit_mode.map(|edit| {
                    nptk::prelude::Ref::Owned(LayoutStyle {
                        display: if *edit { Display::None } else { Display::Flex },
                        padding: Rect {
                            left: LengthPercentage::length(4.0),
                            right: LengthPercentage::length(4.0),
                            top: LengthPercentage::length(2.0),
                            bottom: LengthPercentage::length(1.0),
                        },
                        margin: nptk::core::layout::Rect {
                            left: LengthPercentageAuto::length(4.0),
                            right: LengthPercentageAuto::length(0.0),
                            top: LengthPercentageAuto::length(0.0),
                            bottom: LengthPercentageAuto::length(0.0),
                        },
                        ..Default::default()
                    })
                }))),
            );

            GestureDetector::new(visual_button).with_on_press(MaybeSignal::signal(Box::new(
                FuncSignal::new(move || {
                    let new_state = !*active.get();
                    let _ = toggle_tx.send(new_state);
                    Update::LAYOUT | Update::DRAW
                }),
            )))
        };
            
        let container = Container::new(vec![
            Box::new(breadcrumbs),
            Box::new(empty_space),
            Box::new(text_input),
            Box::new(search_column),
            Box::new(scope_container),
            Box::new(search_toggle),
        ]).with_layout_style(LayoutStyle {
            size: Vector2::new(Dimension::percent(1.0), Dimension::auto()),
            flex_direction: FlexDirection::Row,
            gap: Vector2::new(LengthPercentage::length(8.0), LengthPercentage::length(0.0)),
            align_items: Some(AlignItems::Center),
            ..Default::default()
        });
        
        Self {
            inner: container,
            current_path,
            breadcrumb_items,
            text_value,
            last_synced_path: path_val,
            on_navigate: None,
            signals_hooked: false,
            internal_rx: Some(rx),
            clear_focus_rx: Some(clear_focus_rx),
            request_focus_rx: Some(request_focus_rx),
            focus_rx: None,
            activate_search_rx: None,
            toggle_search_rx: Some(toggle_search_rx),
            text_input_focus_id: focus_id,
            bar_focus_id,
            search_query,
            search_active,
            search_scope,
            search_history,
            search_input_focus_id,
            edit_mode,
        }
    }

    /// Unregister the text input from the focus manager so it cannot receive focus on the next click.
    /// When the bar exits edit mode via Escape, the TextInput has already registered with full
    /// bounds this frame; the next frame the click is processed before update(), so we must
    /// unregister when we process clear_focus_rx. The TextInput will re-register (with zero bounds)
    /// on the next update when it receives dummy layout.
    fn unregister_text_input_focus(info: &mut nptk::core::app::info::AppInfo, focus_id: FocusId) {
        if let Ok(mut manager) = info.focus_manager.lock() {
            manager.unregister_widget(focus_id);
        }
    }

    fn unregister_focus(info: &mut nptk::core::app::info::AppInfo, focus_id: FocusId) {
        if let Ok(mut manager) = info.focus_manager.lock() {
            manager.unregister_widget(focus_id);
        }
    }

    fn register_bar_focus(
        info: &mut nptk::core::app::info::AppInfo,
        focus_id: FocusId,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        if let Ok(mut manager) = info.focus_manager.lock() {
            manager.register_widget(FocusableWidget {
                id: focus_id,
                properties: FocusProperties {
                    tab_focusable: false,
                    click_focusable: true,
                    tab_index: 0,
                    accepts_keyboard: false,
                },
                bounds: FocusBounds { x, y, width, height },
            });
        }
    }
    
    /// Get the search query signal
    pub fn search_query_signal(&self) -> &StateSignal<String> {
        &self.search_query
    }

    /// Get the search active signal
    pub fn search_active_signal(&self) -> &StateSignal<bool> {
        &self.search_active
    }

    /// Get the search input focus ID
    pub fn search_input_focus_id(&self) -> Option<FocusId> {
        self.search_input_focus_id
    }

    pub fn search_scope_signal(&self) -> &StateSignal<SearchScope> {
        &self.search_scope
    }

    pub fn with_search_scope_signal(mut self, signal: StateSignal<SearchScope>) -> Self {
        self.search_scope = signal;
        self
    }
    
    pub fn with_focus_receiver(mut self, rx: mpsc::UnboundedReceiver<()>) -> Self {
        self.focus_rx = Some(rx);
        self
    }

    pub fn with_activate_search_receiver(mut self, rx: mpsc::UnboundedReceiver<()>) -> Self {
        self.activate_search_rx = Some(rx);
        self
    }
    
    
    pub fn with_on_navigate<F>(mut self, callback: F) -> Self
    where
        F: Fn(PathBuf) -> Update + Send + Sync + 'static,
    {
        self.on_navigate = Some(Box::new(callback));
        self
    }
}

#[async_trait(?Send)]
impl Widget for FileLocationBar {
    fn layout_style(&self, context: &nptk::core::layout::LayoutContext) -> nptk::core::layout::StyleNode {
        self.inner.layout_style(context)
    }

    async fn update(
        &mut self,
        layout: &nptk::core::layout::LayoutNode,
        context: nptk::core::app::context::AppContext,
        info: &mut nptk::core::app::info::AppInfo,
    ) -> nptk::core::app::update::Update {
        let mut update = Update::empty();
        
        if !self.signals_hooked {
            context.hook_signal(&self.current_path);
            context.hook_signal(&self.breadcrumb_items);
            context.hook_signal(&self.text_value);
            context.hook_signal(&self.edit_mode);
            context.hook_signal(&self.search_active);
            self.signals_hooked = true;
        }

        // Exit edit mode directly when Escape or Enter is pressed and we have focus, so we don't
        // rely on the TextInput callback or the channel (which can fail after multiple enter/exit cycles).
        if *self.edit_mode.get() && context.get_focused_widget() == Some(self.text_input_focus_id) {
            let escape_or_enter_pressed = info.keys.iter().any(|(_, e)| {
                e.state == nptk::core::window::ElementState::Pressed
                    && matches!(
                        e.physical_key,
                        nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::Escape)
                            | nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::Enter)
                            | nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::NumpadEnter)
                    )
            });
            if escape_or_enter_pressed {
                self.edit_mode.set(false);
                context.clear_focus();
                Self::unregister_text_input_focus(info, self.text_input_focus_id);
                update |= Update::LAYOUT | Update::DRAW;
            }
        }

        // When search is active, Escape closes the search field (whether or not the search input had focus).
        if *self.search_active.get() {
            let escape_pressed = info.keys.iter().any(|(_, e)| {
                e.state == nptk::core::window::ElementState::Pressed
                    && e.physical_key == nptk::core::window::PhysicalKey::Code(nptk::core::window::KeyCode::Escape)
            });
            if escape_pressed {
                self.search_active.set(false);
                self.search_query.set(String::new());
                if let Some(sid) = self.search_input_focus_id {
                    if context.get_focused_widget() == Some(sid) {
                        context.clear_focus();
                        Self::unregister_focus(info, sid);
                    }
                }
                update |= Update::LAYOUT | Update::DRAW;
            }
        }

        // When not in edit mode, ensure the text input cannot steal focus: clear focus and
        // unregister if focus is still on it (e.g. escape channel not processed yet or click
        // already happened before this frame's update).
        if !*self.edit_mode.get() {
            let focused = context.get_focused_widget();
            if focused == Some(self.text_input_focus_id) {
                context.clear_focus();
                Self::unregister_text_input_focus(info, self.text_input_focus_id);
                update |= Update::LAYOUT | Update::DRAW;
            }
        }

        // When not in edit mode, register the bar as click-focusable so a click on the bar
        // gives us focus; we then enter edit mode. Use only the empty_space child's bounds
        // (index 1) so breadcrumbs and search button do not trigger edit mode.
        // When in edit mode, unregister so we don't steal focus from the text input.
        if *self.edit_mode.get() {
            Self::unregister_focus(info, self.bar_focus_id);
        } else if let Some(empty_layout) = layout.children.get(1) {
            let loc = empty_layout.layout.location;
            let size = empty_layout.layout.size;
            if size.width > 0.0 && size.height > 0.0 {
                Self::register_bar_focus(info, self.bar_focus_id, loc.x, loc.y, size.width, size.height);
                let bar_has_focus = context.get_focused_widget() == Some(self.bar_focus_id);
                let bar_gained_focus = info.focus_manager.lock()
                    .map(|mut m| m.get_focus_state(self.bar_focus_id) == FocusState::Gained)
                    .unwrap_or(false);
                if bar_has_focus && bar_gained_focus {
                    self.edit_mode.set(true);
                    context.set_focus(Some(self.text_input_focus_id));
                    update |= Update::LAYOUT | Update::DRAW;
                }
            } else {
                Self::unregister_focus(info, self.bar_focus_id);
            }
        } else {
            Self::unregister_focus(info, self.bar_focus_id);
        }
        
        // Sync path changes to UI
        let path = (*self.current_path.get()).clone();
        if path != self.last_synced_path {
            self.last_synced_path = path.clone();
            
            // Update breadcrumbs
            let new_items = path_to_breadcrumb_items(&path);
            self.breadcrumb_items.set(new_items);
            
            // Update text
            self.text_value.set(path.to_string_lossy().to_string());
            
            update.insert(Update::LAYOUT | Update::DRAW);
        }
        
        // Handle internal navigation events
        if let Some(ref mut rx) = self.internal_rx {
            while let Ok(path) = rx.try_recv() {
                if let Some(callback) = &self.on_navigate {
                    update |= callback(path);
                }
            }
        }
        
        // Handle explicit clear focus requests (before inner update so we clear focus early)
        if let Some(ref mut rx) = self.clear_focus_rx {
            while let Ok(_) = rx.try_recv() {
                self.edit_mode.set(false);
                context.clear_focus();
                Self::unregister_text_input_focus(info, self.text_input_focus_id);
                update |= Update::LAYOUT | Update::DRAW;
            }
        }

        // Handle focus requests
        // Extract ID to avoid borrowing self mutably during loop if we used a method
        let focus_id = self.text_input_focus_id;
        
        if let Some(ref mut rx) = self.focus_rx {
             while let Ok(_) = rx.try_recv() {
                 self.edit_mode.set(true); // Switch to edit mode when receiving external focus request
                 context.set_focus(Some(focus_id));
                 update |= Update::LAYOUT | Update::DRAW;
             }
        }
        
        // Handle focus requests specifically from within the component
        if let Some(ref mut rx) = self.request_focus_rx {
             while let Ok(_) = rx.try_recv() {
                 context.set_focus(Some(self.text_input_focus_id));
                 update |= Update::DRAW;
             }
        }

        if let Some(ref mut rx) = self.activate_search_rx.as_mut() {
            while rx.try_recv().is_ok() {
                self.edit_mode.set(false);
                self.search_active.set(true);
                if let Some(sid) = self.search_input_focus_id {
                    context.set_focus(Some(sid));
                }
                update |= Update::LAYOUT | Update::DRAW;
            }
        }
        
        update |= self.inner.update(layout, context.clone(), info).await;

        if let Some(ref mut rx) = self.toggle_search_rx {
            while let Ok(new_state) = rx.try_recv() {
                self.edit_mode.set(false);
                self.search_active.set(new_state);
                if new_state {
                    if let Some(sid) = self.search_input_focus_id {
                        context.set_focus(Some(sid));
                    }
                } else {
                    self.search_query.set(String::new());
                    if let Some(sid) = self.search_input_focus_id {
                        if context.get_focused_widget() == Some(sid) {
                            context.clear_focus();
                            Self::unregister_focus(info, sid);
                        }
                    }
                }
                update |= Update::LAYOUT | Update::DRAW;
            }
        }

        if !*self.search_active.get() {
            if let Some(sid) = self.search_input_focus_id {
                if context.get_focused_widget() == Some(sid) {
                    context.clear_focus();
                    Self::unregister_focus(info, sid);
                    update |= Update::LAYOUT | Update::DRAW;
                }
            }
        }

        // Process clear_focus again: escape handler runs inside a child's update(), so the
        // message is sent during inner.update() and would otherwise be handled only next frame.
        if let Some(ref mut rx) = self.clear_focus_rx {
            while let Ok(_) = rx.try_recv() {
                self.edit_mode.set(false);
                context.clear_focus();
                Self::unregister_text_input_focus(info, self.text_input_focus_id);
                update |= Update::LAYOUT | Update::DRAW;
            }
        }

        update
    }

    fn render(
        &mut self,
        graphics: &mut dyn nptk::core::vgi::Graphics,
        layout: &nptk::core::layout::LayoutNode,
        info: &mut nptk::core::app::info::AppInfo,
        context: nptk::core::app::context::AppContext,
    ) {
        self.inner.render(graphics, layout, info, context)
    }
}

impl nptk::core::widget::WidgetLayoutExt for FileLocationBar {
    fn set_layout_style(&mut self, layout_style: impl Into<nptk::core::signal::MaybeSignal<nptk::core::layout::LayoutStyle>>) {
        self.inner.set_layout_style(layout_style)
    }
}
