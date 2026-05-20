mod config;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use nptk::gpui::{self as gpui, *};
use nptk::gpui_tokio::Tokio;
use nptk::theme::ActiveTheme;
use nptk::ui::{ListItem, prelude::*};
use npio::backend::local::LocalBackend;
use npio::{get_file_for_uri, register_backend, FileInfo, FileType};

use crate::config::FilemanConfig;

type ViewContext<'a, T> = gpui::Context<'a, T>;

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
    config: FilemanConfig,
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
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));

        let mut this = Self {
            current_path: initial_path.clone(),
            history_back: Vec::new(),
            history_forward: Vec::new(),
            show_hidden: config.folder_view.show_hidden,
            selected_files: HashSet::new(),
            files: Vec::new(),
            config,
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
            let _ = Tokio::spawn(cx, async move {
                for path in paths_to_delete {
                    let _ = std::fs::remove_file(&path).or_else(|_| std::fs::remove_dir_all(&path));
                }
            })
            .await;

            // Trigger reload of the current folder
            let _ = this.update(cx, |this, cx| {
                let current = this.current_path.clone();
                this.navigate_to(current, false, cx);
            });
        })
        .detach();
    }
}

impl Render for FilemanWindow {
    fn render(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();

        div()
            .id("fileman-root")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_this, _action: &Quit, _, cx| cx.quit()))
            .on_action(cx.listener(|this, _action: &GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _action: &GoForward, _, cx| this.go_forward(cx)))
            .on_action(cx.listener(|this, _action: &GoUp, _, cx| this.go_up(cx)))
            .on_action(cx.listener(|this, _action: &ToggleHidden, _, cx| {
                this.toggle_hidden(cx)
            }))
            .on_action(cx.listener(|this, _action: &DeleteSelected, _, cx| {
                this.delete_selected(cx)
            }))
            .flex()
            .h_full()
            .w_full()
            .bg(colors.background)
            .text_color(colors.text)
            .child(self.render_sidebar(window, cx))
            .child(self.render_main_panel(window, cx))
    }
}

impl FilemanWindow {
    fn render_sidebar(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let sidebar_width = self.config.window.splitter_pos;
        let colors = cx.theme().colors().clone();

        let mut places = vec![("Root", PathBuf::from("/"))];
        if let Some(home) = dirs::home_dir() {
            places.push(("Home", home.clone()));
            places.push(("Documents", home.join("Documents")));
            places.push(("Downloads", home.join("Downloads")));
        }

        v_flex()
            .id("fileman-sidebar")
            .w(px(sidebar_width as f32))
            .h_full()
            .bg(colors.panel_background)
            .border_r_1()
            .border_color(colors.border)
            .p_3()
            .gap_2()
            .child(Headline::new("Quick Access").size(HeadlineSize::XSmall))
            .child(
                v_flex()
                    .gap_0p5()
                    .children(places.into_iter().map(|(label, path)| {
                        let is_active = self.current_path == path;
                        let path_clone = path.clone();
                        let label_str = label.to_string();
                        let icon = quick_access_icon(label);

                        ListItem::new(SharedString::from(format!("sidebar-{label_str}")))
                            .toggle_state(is_active)
                            .rounded()
                            .start_slot(
                                Icon::new(icon)
                                    .size(IconSize::Small)
                                    .color(if is_active {
                                        Color::Selected
                                    } else {
                                        Color::Muted
                                    }),
                            )
                            .child(Label::new(label_str))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.navigate_to(path_clone.clone(), true, cx);
                            }))
                    })),
            )
    }

    fn render_main_panel(&mut self, window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .h_full()
            .child(self.render_toolbar(window, cx))
            .child(self.render_files_area(window, cx))
    }

    fn render_toolbar(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let path_str = self.path_input_text.clone();
        let show_hidden = self.show_hidden;
        let colors = cx.theme().colors().clone();

        h_flex()
            .id("fileman-toolbar")
            .h(px(52.0))
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(colors.border)
            .px_3()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        IconButton::new("go-back", IconName::ArrowLeft)
                            .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                    )
                    .child(
                        IconButton::new("go-forward", IconName::ArrowRight)
                            .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx))),
                    )
                    .child(
                        IconButton::new("go-up", IconName::ArrowUp)
                            .on_click(cx.listener(|this, _, _, cx| this.go_up(cx))),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .bg(colors.elevated_surface_background)
                    .border_1()
                    .border_color(colors.border_variant)
                    .rounded_md()
                    .px_3()
                    .py_1()
                    .child(Label::new(path_str).truncate()),
            )
            .child(
                Button::new("toggle-hidden", "Hidden")
                    .style(ButtonStyle::Outlined)
                    .toggle_state(show_hidden)
                    .start_icon(Icon::new(if show_hidden {
                        IconName::Eye
                    } else {
                        IconName::EyeOff
                    }))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_hidden(cx))),
            )
    }

    fn render_files_area(&mut self, _window: &mut Window, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let show_hidden = self.show_hidden;
        let mut visible_files: Vec<&FileInfo> = self
            .files
            .iter()
            .filter(|file_info| {
                let name = file_info.get_name().unwrap_or("");
                if name.is_empty() {
                    return false;
                }
                !(!show_hidden && name.starts_with('.'))
            })
            .collect();

        visible_files.sort_by(|left, right| {
            let left_is_dir = left.get_file_type() == FileType::Directory;
            let right_is_dir = right.get_file_type() == FileType::Directory;
            if left_is_dir != right_is_dir {
                right_is_dir.cmp(&left_is_dir)
            } else {
                left.get_name()
                    .unwrap_or("")
                    .cmp(right.get_name().unwrap_or(""))
            }
        });

        v_flex()
            .id("files-scroll-area")
            .flex_1()
            .overflow_y_scroll()
            .p_2()
            .gap_0p5()
            .when(visible_files.is_empty(), |panel| {
                panel.child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(Icon::new(IconName::Folder).size(IconSize::XLarge).color(Color::Muted))
                        .child(
                            Label::new("This folder is empty").color(Color::Muted).size(LabelSize::Small),
                        ),
                )
            })
            .children(visible_files.into_iter().map(|file_info| {
                let name = file_info.get_name().unwrap_or("").to_string();
                let is_directory = file_info.get_file_type() == FileType::Directory;
                let is_selected = self.selected_files.contains(&name);
                let size_str = if is_directory {
                    "--".to_string()
                } else {
                    format_size(file_info.get_size())
                };

                let name_for_click = name.clone();
                let name_for_open = name.clone();
                let file_icon = if is_directory {
                    IconName::Folder
                } else {
                    IconName::File
                };

                ListItem::new(SharedString::from(format!("file-row-{name}")))
                    .toggle_state(is_selected)
                    .rounded()
                    .start_slot(
                        Icon::new(file_icon)
                            .size(IconSize::Small)
                            .color(if is_directory {
                                Color::Accent
                            } else {
                                Color::Default
                            }),
                    )
                    .child(Label::new(name))
                    .end_slot(Label::new(size_str).color(Color::Muted).size(LabelSize::XSmall))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() == 2 {
                            let full_path = this.current_path.join(&name_for_open);
                            if is_directory {
                                this.navigate_to(full_path, true, cx);
                            } else {
                                cx.open_with_system(&full_path);
                            }
                        } else {
                            this.selected_files.clear();
                            this.selected_files.insert(name_for_click.clone());
                            cx.notify();
                        }
                    }))
            }))
    }
}

fn quick_access_icon(label: &str) -> IconName {
    match label {
        "Home" => IconName::OpenFolder,
        "Documents" => IconName::FileDoc,
        "Downloads" => IconName::ArrowDown,
        _ => IconName::Folder,
    }
}

fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
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

    let backend = Arc::new(LocalBackend::new());
    register_backend(backend);

    nptk::gpui_platform::application().run(|cx: &mut App| {
        nptk::init(cx);

        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| FilemanWindow::new(cx))
        })
        .expect("Failed to open file manager window");
    });
}
