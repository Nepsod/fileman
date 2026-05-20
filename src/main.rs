mod config;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;
use gpui_tokio::Tokio;

type ViewContext<'a, T> = gpui::Context<'a, T>;
use npio::backend::local::LocalBackend;
use npio::{get_file_for_uri, register_backend, FileInfo, FileType};

use crate::config::FilemanConfig;

// Define Actions for menus and shortcut keys
gpui::actions!(
    fileman,
    [
        CreateFolder,
        CreateFile,
        GoBack,
        GoForward,
        GoUp,
        ToggleHidden,
        DeleteSelected,
        Quit
    ]
);


struct FilemanWindow {
    current_path: PathBuf,
    history_back: Vec<PathBuf>,
    history_forward: Vec<PathBuf>,
    show_hidden: bool,
    selected_files: HashSet<String>,
    files: Vec<FileInfo>,
    search_query: String,
    config: FilemanConfig,
    editing_path: bool,
    path_input_text: String,
    focus_handle: FocusHandle,
}

impl FilemanWindow {
    fn new(cx: &mut ViewContext<Self>) -> Self {
        let config = FilemanConfig::load_or_create();
        let initial_path = config
            .folder_view
            .default_path
            .clone()
            .or_else(|| dirs::home_dir())
            .unwrap_or_else(|| PathBuf::from("/"));

        let mut this = Self {
            current_path: initial_path.clone(),
            history_back: Vec::new(),
            history_forward: Vec::new(),
            show_hidden: config.folder_view.show_hidden,
            selected_files: HashSet::new(),
            files: Vec::new(),
            search_query: String::new(),
            config,
            editing_path: false,
            path_input_text: initial_path.to_string_lossy().to_string(),
            focus_handle: cx.focus_handle(),
        };

        this.navigate_to(initial_path, false, cx);
        this.register_menus(cx);
        this
    }

    fn register_menus(&self, cx: &mut ViewContext<Self>) {
        let menus = vec![
            Menu::new("File").items(vec![
                MenuItem::action("Go Up", GoUp),
                MenuItem::action("Delete Selected", DeleteSelected),
                MenuItem::separator(),
                MenuItem::action("Quit", Quit),
            ]),
            Menu::new("View").items(vec![
                MenuItem::action("Toggle Hidden Files", ToggleHidden),
            ]),
        ];
        cx.set_menus(menus);
    }

    fn navigate_to(&mut self, path: PathBuf, record_history: bool, cx: &mut ViewContext<Self>) {
        if record_history {
            self.history_back.push(self.current_path.clone());
            self.history_forward.clear();
        }
        self.current_path = path.clone();
        self.path_input_text = path.to_string_lossy().to_string();
        self.selected_files.clear();
        self.editing_path = false;

        let path_str = path.to_string_lossy().to_string();
        cx.spawn(async move |this, cx| {
            let files_res = Tokio::spawn(cx, async move {
                let dir_uri = format!("file://{}", path_str);
                if let Ok(dir_file) = get_file_for_uri(&dir_uri) {
                    let mut list = Vec::new();
                    if let Ok(mut enumerator) = dir_file
                        .enumerate_children("standard::*,time::modified", None)
                        .await
                    {
                        while let Ok(Some((info, _child))) = enumerator.next_file(None).await {
                            list.push(info);
                        }
                        let _ = enumerator.close(None).await;
                    }
                    Some(list)
                } else {
                    None
                }
            })
            .await;

            if let Ok(Some(files)) = files_res {
                let _ = this.update(cx, |this, cx| {
                    this.files = files;
                    cx.notify();
                });
            }
        })
        .detach();

        cx.notify();
    }

    fn go_back(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(prev) = self.history_back.pop() {
            self.history_forward.push(self.current_path.clone());
            self.navigate_to(prev, false, cx);
        }
    }

    fn go_forward(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(next) = self.history_forward.pop() {
            self.history_back.push(self.current_path.clone());
            self.navigate_to(next, false, cx);
        }
    }

    fn go_up(&mut self, cx: &mut ViewContext<Self>) {
        if let Some(parent) = self.current_path.parent() {
            self.navigate_to(parent.to_path_buf(), true, cx);
        }
    }

    fn toggle_hidden(&mut self, cx: &mut ViewContext<Self>) {
        self.show_hidden = !self.show_hidden;
        self.config.folder_view.show_hidden = self.show_hidden;
        self.config.save();
        cx.notify();
    }

    fn delete_selected(&mut self, cx: &mut ViewContext<Self>) {
        let paths_to_delete: Vec<PathBuf> = self
            .selected_files
            .iter()
            .map(|name| self.current_path.join(name))
            .collect();

        if paths_to_delete.is_empty() {
            return;
        }

        // Perform deletion asynchronously
        cx.spawn(async move |this, cx| {
            let _ = cx.background_executor().spawn(async move {
                for p in paths_to_delete {
                    let _ = std::fs::remove_file(&p).or_else(|_| std::fs::remove_dir_all(&p));
                }
            }).await;

            // Trigger reload of the current folder
            let _ = this.update(cx, |this, cx| {
                let current = this.current_path.clone();
                this.navigate_to(current, false, cx);
            });
        })
        .detach();
    }
}

// Implement action listeners
impl Render for FilemanWindow {
    fn render(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        // Setup listener shortcuts
        div()
            .id("fileman-root")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _action: &Quit, _, cx| cx.quit()))
            .on_action(cx.listener(|this, _action: &GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _action: &GoForward, _, cx| this.go_forward(cx)))
            .on_action(cx.listener(|this, _action: &GoUp, _, cx| this.go_up(cx)))
            .on_action(cx.listener(|this, _action: &ToggleHidden, _, cx| this.toggle_hidden(cx)))
            .on_action(cx.listener(|this, _action: &DeleteSelected, _, cx| this.delete_selected(cx)))
            .flex()
            .h_full()
            .w_full()
            .bg(rgb(0x121214))
            .text_color(rgb(0xe2e8f0))
            .font_family("Inter")
            .child(self.render_sidebar(window, cx))
            .child(self.render_main_panel(window, cx))
    }
}

impl FilemanWindow {
    fn render_sidebar(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let sidebar_width = self.config.window.splitter_pos;
        let mut places = vec![
            ("Root", PathBuf::from("/")),
        ];
        if let Some(home) = dirs::home_dir() {
            places.push(("Home", home.clone()));
            places.push(("Documents", home.join("Documents")));
            places.push(("Downloads", home.join("Downloads")));
        }

        div()
            .flex()
            .flex_col()
            .w(px(sidebar_width as f32))
            .h_full()
            .bg(rgb(0x18181b))
            .border_r_1()
            .border_color(rgb(0x2d2d30))
            .p_4()
            .gap_4()
            .child(
                div()
                    .text_size(px(14.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xa78bfa))
                    .child("Quick Access")
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(places.into_iter().map(|(label, path)| {
                        let is_active = self.current_path == path;
                        let path_clone = path.clone();
                        let label_str = label.to_string();
                        div()
                            .id(SharedString::from(format!("sidebar-{}", label_str)))
                            .flex()
                            .items_center()
                            .p_2()
                            .rounded_md()
                            .text_size(px(13.0))
                            .cursor_pointer()
                            .bg(if is_active { rgb(0x2d2b38) } else { rgba(0) })
                            .hover(|s| s.bg(if is_active { rgb(0x2d2b38) } else { rgb(0x27272a) }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.navigate_to(path_clone.clone(), true, cx);
                            }))
                            .child(
                                div()
                                    .mr(px(8.0))
                                    .child(if label == "Home" { "🏠" } else if label == "Downloads" { "📥" } else if label == "Documents" { "📂" } else { "📁" })
                            )
                            .child(label_str)
                    }))
            )
    }

    fn render_main_panel(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .child(self.render_toolbar(window, cx))
            .child(self.render_files_area(window, cx))
    }

    fn render_toolbar(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let path_str = self.current_path.to_string_lossy().to_string();
        let show_hidden = self.show_hidden;

        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(52.0))
            .border_b_1()
            .border_color(rgb(0x2d2d30))
            .px_4()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    // Back
                    .child(
                        div()
                            .id("go-back")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(28.0))
                            .h(px(28.0))
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x27272a)))
                            .on_click(cx.listener(|this, _, _, cx| this.go_back(cx)))
                            .child("⬅️")
                    )
                    // Forward
                    .child(
                        div()
                            .id("go-forward")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(28.0))
                            .h(px(28.0))
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x27272a)))
                            .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx)))
                            .child("➡️")
                    )
                    // Parent Up
                    .child(
                        div()
                            .id("go-up")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(28.0))
                            .h(px(28.0))
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x27272a)))
                            .on_click(cx.listener(|this, _, _, cx| this.go_up(cx)))
                            .child("⬆️")
                    )
            )
            // Path Input Bar
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .bg(rgb(0x18181b))
                    .border_1()
                    .border_color(rgb(0x2d2d30))
                    .rounded_md()
                    .px_3()
                    .py_1()
                    .text_size(px(13.0))
                    .child(path_str)
            )
            // Search & Show Hidden Control
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("toggle-hidden")
                            .flex()
                            .items_center()
                            .p_2()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(if show_hidden { rgb(0x2d2b38) } else { rgba(0) })
                            .hover(|s| s.bg(rgb(0x27272a)))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_hidden(cx)))
                            .child("👁️ Hidden")
                    )
            )
    }

    fn render_files_area(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let show_hidden = self.show_hidden;
        let mut visible_files: Vec<&FileInfo> = self
            .files
            .iter()
            .filter(|f| {
                let name = f.get_name().unwrap_or("");
                if name.is_empty() {
                    return false;
                }
                if !show_hidden && name.starts_with('.') {
                    return false;
                }
                true
            })
            .collect();

        // Sort files: folders first, then alphabetically
        visible_files.sort_by(|a, b| {
            let a_is_dir = a.get_file_type() == FileType::Directory;
            let b_is_dir = b.get_file_type() == FileType::Directory;
            if a_is_dir != b_is_dir {
                b_is_dir.cmp(&a_is_dir)
            } else {
                a.get_name().unwrap_or("").cmp(b.get_name().unwrap_or(""))
            }
        });

        if visible_files.is_empty() {
            return div()
                .id("empty-folder")
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(32.0))
                        .child("📁")
                )
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(rgb(0x71717a))
                        .child("This folder is empty")
                );
        }

        div()
            .id("files-scroll-area")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_y_scroll()
            .p_4()
            .gap_1()
            .children(visible_files.into_iter().map(|f| {
                let name = f.get_name().unwrap_or("").to_string();
                let is_dir = f.get_file_type() == FileType::Directory;
                let size = f.get_size();
                let is_selected = self.selected_files.contains(&name);

                let name_clone = name.clone();
                let name_clone2 = name.clone();
                let size_str = if is_dir {
                    "--".to_string()
                } else {
                    format_size(size)
                };

                div()
                    .id(SharedString::from(format!("file-row-{}", name_clone)))
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(if is_selected { rgb(0x2d2b38) } else { rgba(0) })
                    .border_1()
                    .border_color(if is_selected { rgb(0x8b5cf6) } else { rgba(0) })
                    .hover(|s| s.bg(if is_selected { rgb(0x2d2b38) } else { rgb(0x18181b) }))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() == 2 {
                            let full_path = this.current_path.join(&name_clone2);
                            if is_dir {
                                this.navigate_to(full_path, true, cx);
                            } else {
                                cx.open_with_system(&full_path);
                            }
                        } else {
                            this.selected_files.clear();
                            this.selected_files.insert(name_clone.clone());
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .mr(px(10.0))
                                    .child(if is_dir { "📁" } else { "📄" })
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .child(name)
                            )
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(0x71717a))
                            .child(size_str)
                    )
            }))
    }
}

fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Register local filesystem backend
    let backend = Arc::new(LocalBackend::new());
    register_backend(backend);

    gpui_platform::application().run(|cx: &mut App| {
        gpui_tokio::init(cx);

        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| FilemanWindow::new(cx))
        }).expect("Failed to open file manager window");
    });
}
