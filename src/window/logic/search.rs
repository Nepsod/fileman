use crate::window::imports::*;

impl FilemanWindow {
    pub(crate) fn activate_search(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        self.path_edit_active = false;
        self.search_active = true;
        self.search_line_input.update(cx, |input, cx| {
            input.set_text(self.search_query.clone(), cx);
        });
        self.focus_search_line_input(window, cx);
        self.set_status("Search: type to filter, Enter/Escape to finish", cx);
        cx.notify();
    }

    pub(crate) fn clear_search(&mut self, cx: &mut ViewContext<Self>) {
        self.search_active = false;
        self.search_query.clear();
        self.search_matches.clear();
        self.search_in_progress = false;
        self.search_line_input.update(cx, |input, cx| {
            input.set_text("", cx);
        });
        self.set_status("Ready", cx);
        cx.notify();
    }

    pub(crate) fn focus_search_line_input(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        self.search_line_input.update(cx, |input, cx| {
            input.set_text(self.search_query.clone(), cx);
        });
        let focus_handle = self.search_line_input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    pub(crate) fn handle_search_input_event(
        &mut self,
        event: ToolbarLineInputEvent,
        cx: &mut ViewContext<Self>,
    ) {
        match event {
            ToolbarLineInputEvent::Changed(text) => {
                self.search_query = text;
                self.schedule_subfolder_search(cx);
            }
            ToolbarLineInputEvent::Submit => {
                self.record_search_history();
                cx.notify();
            }
            ToolbarLineInputEvent::Cancel => self.clear_search(cx),
        }
    }

    pub(crate) fn record_search_history(&mut self) {
        const MAX_SEARCH_HISTORY: usize = 10;
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.search_history.retain(|entry| entry != &query);
        self.search_history.insert(0, query);
        self.search_history.truncate(MAX_SEARCH_HISTORY);
    }

    pub(crate) fn schedule_subfolder_search(&mut self, cx: &mut ViewContext<Self>) {
        if !self.using_subfolder_search() {
            self.search_matches.clear();
            self.search_in_progress = false;
            cx.notify();
            return;
        }

        let root = self.current_path.clone();
        let query = self.search_query.clone();
        let show_hidden = self.show_hidden;
        self.search_in_progress = true;
        self.set_status("Searching subfolders…", cx);

        cx.spawn(async move |this, cx| {
            let matches = Tokio::spawn(cx, async move {
                crate::search::find_in_subfolders(&root, &query, show_hidden)
            })
            .await
            .unwrap_or_default();

            let _ = this.update(cx, |this, cx| {
                this.search_in_progress = false;
                this.search_matches = matches;
                let count = this.search_matches.len();
                this.set_status(format!("Found {count} matches in subfolders"), cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn set_search_scope(
        &mut self,
        scope: SearchScope,
        window: &mut Window,
        cx: &mut ViewContext<Self>,
    ) {
        if !self.search_active {
            self.activate_search(window, cx);
        }
        self.search_scope = scope;
        self.search_matches.clear();
        match scope {
            SearchScope::CurrentFolder => {
                self.set_status("Search: current folder only", cx);
                cx.notify();
            }
            SearchScope::Subfolders => {
                if self.using_subfolder_search() {
                    self.schedule_subfolder_search(cx);
                } else {
                    self.set_status("Search: include subfolders", cx);
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn toggle_search_subfolders(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) {
        if !self.search_active {
            self.activate_search(window, cx);
        }
        self.search_scope = match self.search_scope {
            SearchScope::CurrentFolder => SearchScope::Subfolders,
            SearchScope::Subfolders => SearchScope::CurrentFolder,
        };
        self.search_matches.clear();
        if self.using_subfolder_search() {
            self.schedule_subfolder_search(cx);
        } else {
            self.set_status("Search: current folder only", cx);
            cx.notify();
        }
    }

    pub(crate) fn using_subfolder_search(&self) -> bool {
        self.search_active
            && self.search_scope == SearchScope::Subfolders
            && !self.search_query.trim().is_empty()
    }

}
