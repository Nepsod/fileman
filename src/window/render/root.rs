use crate::actions::*;
use crate::window::FilemanWindow;
use nptk::gpui::{anchored, deferred, Context, MouseButton, MouseDownEvent, Render, Window};
use nptk::theme::ActiveTheme;
use nptk::ui::prelude::*;

type ViewContext<'a, T> = Context<'a, T>;

macro_rules! window_action {
    ($element:expr, $cx:expr, $Action:ty) => {
        $element.on_action($cx.listener(|this, action: &$Action, window, cx| {
            this.dispatch_action(action, window, cx)
        }))
    };
}

impl Render for FilemanWindow {
    fn render(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();

        if self.marquee_drag.is_some() {
            self.register_marquee_window_listeners(window, cx);
        }
        if self.sidebar_resize_drag.is_some() {
            self.register_sidebar_resize_listeners(window, cx);
        }

        let root = div()
            .id("fileman-root")
            .relative()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_this, _action: &Quit, _, cx| cx.quit()));

        let root = window_action!(root, cx, GoBack);
        let root = window_action!(root, cx, GoForward);
        let root = window_action!(root, cx, GoUp);
        let root = window_action!(root, cx, ToggleHidden);
        let root = window_action!(root, cx, DeleteSelected);
        let root = window_action!(root, cx, DeletePermanent);
        let root = window_action!(root, cx, Refresh);
        let root = window_action!(root, cx, SelectAll);
        let root = window_action!(root, cx, CreateFolder);
        let root = window_action!(root, cx, CreateFile);
        let root = window_action!(root, cx, Rename);
        let root = window_action!(root, cx, crate::actions::Copy);
        let root = window_action!(root, cx, Cut);
        let root = window_action!(root, cx, Paste);
        let root = window_action!(root, cx, Duplicate);
        let root = window_action!(root, cx, ClearSelection);
        let root = window_action!(root, cx, InvertSelection);
        let root = window_action!(root, cx, ActivateSearch);
        let root = window_action!(root, cx, ClearSearch);
        let root = window_action!(root, cx, ToggleSearchSubfolders);
        let root = window_action!(root, cx, SetSearchCurrentFolder);
        let root = window_action!(root, cx, SetSearchIncludeSubfolders);
        let root = window_action!(root, cx, FocusPathBar);
        let root = window_action!(root, cx, GoHome);
        let root = window_action!(root, cx, Undo);
        let root = window_action!(root, cx, Redo);
        let root = window_action!(root, cx, OpenTerminal);
        let root = window_action!(root, cx, OpenSelection);
        let root = window_action!(root, cx, OpenWithSystem);
        let root = window_action!(root, cx, ShowProperties);
        let root = window_action!(root, cx, ShowSettings);
        let root = window_action!(root, cx, ShowAbout);
        let root = window_action!(root, cx, GoToParent);
        let root = window_action!(root, cx, ZoomIn);
        let root = window_action!(root, cx, ZoomOut);
        let root = window_action!(root, cx, ZoomReset);
        let root = window_action!(root, cx, NewTab);
        let root = window_action!(root, cx, NewWindow);
        let root = window_action!(root, cx, CloseTab);
        let root = window_action!(root, cx, AddBookmark);
        let root = window_action!(root, cx, RemoveBookmark);
        let root = window_action!(root, cx, SortByName);
        let root = window_action!(root, cx, SortBySize);
        let root = window_action!(root, cx, SortByModified);
        let root = window_action!(root, cx, SortByType);
        let root = window_action!(root, cx, ToggleSortOrder);
        let root = window_action!(root, cx, SortNameAsc);
        let root = window_action!(root, cx, SortNameDesc);
        let root = window_action!(root, cx, SortSizeAsc);
        let root = window_action!(root, cx, SortSizeDesc);
        let root = window_action!(root, cx, SortModifiedAsc);
        let root = window_action!(root, cx, SortModifiedDesc);
        let root = window_action!(root, cx, SortTypeAsc);
        let root = window_action!(root, cx, SortTypeDesc);
        let root = window_action!(root, cx, ViewList);
        let root = window_action!(root, cx, ViewIcon);
        let root = window_action!(root, cx, ViewCompact);
        let root = window_action!(root, cx, ViewTable);

        root
            .on_key_down(cx.listener(|this, event, _, cx| {
                this.handle_toolbar_input_key(event, cx)
            }))
            .flex()
            .flex_col()
            .h_full()
            .w_full()
            .bg(colors.background)
            .text_color(colors.text)
            .child({
                let colors = colors.clone();
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(window, cx))
                    .child(
                        div()
                            .id("fileman-sidebar-splitter")
                            .w(px(4.0))
                            .h_full()
                            .flex_shrink_0()
                            .cursor_col_resize()
                            .bg(colors.border)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    this.begin_sidebar_resize(event.position.x, window, cx);
                                    cx.notify();
                                }),
                            ),
                    )
                    .child(self.render_main_panel(window, cx))
            })
            .child(self.render_status_bar(window, cx))
            .when_some(self.pending_delete.clone(), |root, pending| {
                root.child(Self::render_delete_dialog(pending, cx))
            })
            .when_some(self.pending_rename.clone(), |root, pending| {
                root.child(Self::render_rename_dialog(pending, cx))
            })
            .when_some(self.pending_properties.clone(), |root, dialog| {
                root.child(Self::render_properties_dialog(dialog, cx))
            })
            .when_some(self.pending_settings.clone(), |root, draft| {
                root.child(self.render_settings_dialog(draft, cx))
            })
            .when_some(self.pending_paste_choice.clone(), |root, pending| {
                root.child(Self::render_paste_conflict_dialog(pending, cx))
            })
            .when_some(self.pending_rename_collision.clone(), |root, pending| {
                root.child(Self::render_rename_collision_dialog(pending, cx))
            })
            .when(self.show_about, |root| root.child(Self::render_about_dialog(cx)))
            .children(self.context_menu.as_ref().map(|(menu, position, _)| {
                deferred(
                    anchored()
                        .position(*position)
                        .anchor(nptk::gpui::Anchor::TopLeft)
                        .child(menu.clone()),
                )
                .with_priority(1)
            }))
    }
}
