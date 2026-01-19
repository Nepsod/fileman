//! File manager sidebar widget
//!
//! A reusable sidebar widget for file managers and file choosers.
//! Provides Places (user directories), Bookmarks, Devices, and custom sections.

use async_trait::async_trait;
use nptk::prelude::*;
use nptk::widgets::sidebar::{Sidebar, SidebarSection, SidebarItem};
use nptk::services::{
    get_user_special_dir_path, UserDirectory,
    get_home_icon_name, get_directory_icon_name,
};
use nptk::services::bookmarks::BookmarksService;
use nptk::services::thumbnail::npio_adapter::uri_to_path;
use nptk::core::app::info::AppInfo;
use nptk::core::vgi::Graphics;
use nptk::theme::theme::Theme;
use std::path::PathBuf;
use tokio::sync::mpsc;

/// Configuration for FilemanSidebar
#[derive(Debug, Clone)]
pub struct FilemanSidebarConfig {
    show_places: bool,
    show_bookmarks: bool,
    show_devices: bool,
    user_directories: Vec<UserDirectory>,
    custom_sections: Vec<SidebarSection>,
    width: f32,
    use_symbolic_icons: bool,
}

impl Default for FilemanSidebarConfig {
    fn default() -> Self {
        Self {
            show_places: true,
            show_bookmarks: false,
            show_devices: false,
            user_directories: vec![
                UserDirectory::Desktop,
                UserDirectory::Documents,
                UserDirectory::Download,
                UserDirectory::Music,
                UserDirectory::Pictures,
                UserDirectory::Videos,
            ],
            custom_sections: Vec::new(),
            width: 200.0,
            use_symbolic_icons: false,
        }
    }
}

/// A reusable file manager sidebar widget.
///
/// Provides Places (user directories), Bookmarks, Devices, and custom sections.
/// Uses a channel for navigation events to ensure Send+Sync compatibility.
pub struct FilemanSidebar {
    inner: Sidebar,
    config: FilemanSidebarConfig,
    navigation_tx: mpsc::UnboundedSender<PathBuf>,
    navigation_rx: Option<mpsc::UnboundedReceiver<PathBuf>>,
    bookmarks_service: Option<BookmarksService>,
    layout_style: MaybeSignal<LayoutStyle>,
}

impl FilemanSidebar {
    /// Create a new FilemanSidebar with default configuration.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let config = FilemanSidebarConfig::default();
        
        // Build sections based on config (synchronous - user dirs will be loaded later)
        let sections = Self::build_sections(&config, tx.clone());
        
        // Set up navigation callback
        let nav_tx_clone = tx.clone();
        let mut sidebar = Sidebar::new()
            .with_on_item_selected(move |item| {
                if let Some(ref uri) = item.uri {
                    // Extract path from file:// URI
                    if let Some(path) = uri_to_path(uri) {
                        let _ = nav_tx_clone.send(path);
                        return Update::EVAL | Update::LAYOUT | Update::DRAW;
                    }
                }
                Update::empty()
            });
        
        // Add sections to sidebar
        for section in sections {
            sidebar = sidebar.with_section(section);
        }

        Self {
            inner: sidebar,
            config,
            navigation_tx: tx,
            navigation_rx: Some(rx),
            bookmarks_service: None,
            layout_style: LayoutStyle {
                size: Vector2::new(Dimension::length(200.0), Dimension::percent(1.0)),
                flex_shrink: 0.0, // Prevent sidebar from shrinking below its width
                ..Default::default()
            }
            .into(),
        }
    }

    fn apply_with(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }

    /// Enable or disable the Places section.
    pub fn with_places(mut self, enabled: bool) -> Self {
        self.config.show_places = enabled;
        self.rebuild_sidebar();
        self
    }

    /// Enable or disable the Bookmarks section.
    pub fn with_bookmarks(mut self, enabled: bool) -> Self {
        self.config.show_bookmarks = enabled;
        if enabled && self.bookmarks_service.is_none() {
            self.bookmarks_service = Some(BookmarksService::new());
        }
        self.rebuild_sidebar();
        self
    }

    /// Enable or disable the Devices section.
    pub fn with_devices(mut self, enabled: bool) -> Self {
        self.config.show_devices = enabled;
        self.rebuild_sidebar();
        self
    }

    /// Set which user directories to show in Places section.
    pub fn with_user_directories(mut self, dirs: Vec<UserDirectory>) -> Self {
        self.config.user_directories = dirs;
        self.rebuild_sidebar();
        self
    }

    /// Add a custom section to the sidebar.
    pub fn with_custom_section(mut self, section: SidebarSection) -> Self {
        self.config.custom_sections.push(section);
        self.rebuild_sidebar();
        self
    }

    /// Set the width of the sidebar.
    pub fn with_width(mut self, width: f32) -> Self {
        self.apply_with(|s| {
            s.config.width = width;
            s.layout_style = LayoutStyle {
                size: Vector2::new(Dimension::length(width), Dimension::percent(1.0)),
                flex_shrink: 0.0, // Prevent sidebar from shrinking below its width
                ..Default::default()
            }
            .into();
        })
    }

    /// Use symbolic icons instead of regular icons.
    pub fn with_symbolic_icons(mut self, symbolic: bool) -> Self {
        self.apply_with(|s| s.config.use_symbolic_icons = symbolic)
    }

    /// Get the receiver end of the navigation channel.
    ///
    /// This consumes the receiver. Call this once after building the sidebar.
    /// Poll the receiver in your widget's update loop to handle navigation events.
    pub fn take_navigation_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<PathBuf>> {
        self.navigation_rx.take()
    }

    /// Reload bookmarks from disk asynchronously.
    ///
    /// This will update the Bookmarks section if it's enabled.
    /// Note: This requires rebuilding the sidebar sections.
    pub async fn reload_bookmarks(&mut self) -> Result<(), String> {
        if !self.config.show_bookmarks {
            return Ok(());
        }

        let service = self.bookmarks_service.as_mut()
            .ok_or_else(|| "BookmarksService not initialized".to_string())?;

        service.load()
            .await
            .map_err(|e| format!("Failed to load bookmarks: {}", e))?;

        // TODO: Rebuild sidebar sections to include updated bookmarks
        // This requires a way to update the inner Sidebar's sections
        Ok(())
    }

    /// Rebuild the sidebar with current configuration.
    /// This is called when configuration changes via builder methods.
    fn rebuild_sidebar(&mut self) {
        // Note: Sidebar doesn't support modifying sections after creation easily
        // For now, we rebuild the entire sidebar. This is called when builder methods change config.
        let sections = Self::build_sections(&self.config, self.navigation_tx.clone());
        
        // Clone the sender for the callback
        let nav_tx_for_callback = self.navigation_tx.clone();
        
        // Recreate sidebar with new sections and callback
        let mut new_sidebar = Sidebar::new()
            .with_on_item_selected(move |item| {
                if let Some(ref uri) = item.uri {
                    if let Some(path) = uri_to_path(uri) {
                        let _ = nav_tx_for_callback.send(path);
                        return Update::EVAL | Update::LAYOUT | Update::DRAW;
                    }
                }
                Update::empty()
            });
        
        for section in sections {
            new_sidebar = new_sidebar.with_section(section);
        }
        
        self.inner = new_sidebar;
    }

    /// Build sections based on configuration.
    fn build_sections(
        config: &FilemanSidebarConfig,
        _nav_tx: mpsc::UnboundedSender<PathBuf>,
    ) -> Vec<SidebarSection> {
        let mut sections = Vec::new();

        // Places section
        if config.show_places {
            if let Some(places_section) = Self::build_places_section(config) {
                sections.push(places_section);
            }
        }

        // Bookmarks section
        if config.show_bookmarks {
            if let Some(bookmarks_section) = Self::build_bookmarks_section(config) {
                sections.push(bookmarks_section);
            }
        }

        // Custom sections
        sections.extend(config.custom_sections.clone());

        // Devices section (placeholder for now)
        if config.show_devices {
            sections.push(SidebarSection::new("Devices"));
        }

        sections
    }

    /// Build the Places section with user directories.
    /// Note: User directories are loaded synchronously using blocking approach.
    /// This works because we're in a tokio runtime context from #[tokio::main].
    fn build_places_section(config: &FilemanSidebarConfig) -> Option<SidebarSection> {
        let mut items = Vec::new();

        // Home directory - use env var directly to avoid requiring npio backend
        let home_path = std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));

        let home_icon = get_home_icon_name(config.use_symbolic_icons);
        log::debug!("Home icon name: '{}'", home_icon);
        items.push(
            SidebarItem::new("home", "Home")
                .with_icon(home_icon)
                .with_uri(format!("file://{}", home_path.display())),
        );

        // User directories - load synchronously using tokio runtime handle
        // This works because we're in a tokio runtime context from #[tokio::main].
        // We use block_in_place + block_on to safely convert async call to sync during widget construction.
        // Use get_user_special_dir_path instead of get_user_special_file to avoid requiring npio backend
        for dir_type in &config.user_directories {
            // Use block_in_place to move to a blocking thread, then block_on the async call
            // This prevents blocking the async runtime if we're already on an async thread
            let path_result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::try_current()
                    .map(|handle| {
                        handle.block_on(async {
                            get_user_special_dir_path(*dir_type).await
                        })
                    })
                    .unwrap_or_else(|_| {
                        // If no runtime available (shouldn't happen in normal execution),
                        // return None so we skip this directory
                        log::warn!("No tokio runtime available for loading user directory {:?}", dir_type);
                        None
                    })
            });
            
            if let Some(path) = path_result {
                let uri = format!("file://{}", path.display());
                let label = match dir_type {
                    UserDirectory::Desktop => "Desktop",
                    UserDirectory::Documents => "Documents",
                    UserDirectory::Download => "Downloads",
                    UserDirectory::Music => "Music",
                    UserDirectory::Pictures => "Pictures",
                    UserDirectory::Videos => "Videos",
                    UserDirectory::PublicShare => "Public",
                    UserDirectory::Templates => "Templates",
                };
                let icon = get_directory_icon_name(*dir_type, config.use_symbolic_icons);
                log::debug!("Adding sidebar item: {} with icon '{}' and path {:?}", label, icon, path);

                items.push(
                    SidebarItem::new(format!("{:?}", dir_type).to_lowercase(), label)
                        .with_icon(icon)
                        .with_uri(uri),
                );
            } else {
                log::warn!("User directory {:?} not found or could not be loaded", dir_type);
            }
        }

        if items.is_empty() {
            None
        } else {
            Some(SidebarSection::new("Places").with_items(items))
        }
    }

    /// Build the Bookmarks section.
    /// Returns None if bookmarks cannot be loaded or are empty.
    /// Note: Bookmark loading may be deferred to avoid blocking during widget construction.
    fn build_bookmarks_section(config: &FilemanSidebarConfig) -> Option<SidebarSection> {
        // Skip synchronous bookmark loading during construction to avoid deadlocks.
        // The issue is that when FilemanSidebar::new() is called, it happens during
        // widget tree construction which may be in a tokio runtime context. Using
        // smol::block_on() or tokio::block_on() here can cause deadlocks.
        //
        // Solution: Bookmarks should be loaded asynchronously after widget creation.
        // For now, return None - the bookmarks section will be empty initially.
        // TODO: Implement proper async bookmark loading that:
        //   1. Creates sidebar with empty bookmarks section initially
        //   2. Spawns async task to load bookmarks
        //   3. Updates sidebar sections when bookmarks are loaded
        log::debug!("Bookmarks section loading deferred to avoid blocking during construction");
        None
    }
}

impl Default for FilemanSidebar {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl Widget for FilemanSidebar {
    fn widget_id(&self) -> WidgetId {
        WidgetId::new("nptk-fileman-widgets", "FilemanSidebar")
    }

    fn layout_style(&self, _context: &LayoutContext) -> StyleNode {
        StyleNode {
            style: self.layout_style.get().clone(),
            children: vec![self.inner.layout_style(_context)],
            measure_func: None,
        }
    }

    async fn update(
        &mut self,
        layout: &LayoutNode,
        context: AppContext,
        info: &mut AppInfo,
    ) -> Update {
        // Handle navigation events from channel
        // Note: The receiver should be taken and polled externally, but we can check here too
        // For now, just delegate to inner sidebar
        
        if !layout.children.is_empty() {
            self.inner.update(&layout.children[0], context, info).await
        } else {
            Update::empty()
        }
    }

    fn render(
        &mut self,
        graphics: &mut dyn Graphics,
        theme: &mut dyn Theme,
        layout: &LayoutNode,
        info: &mut AppInfo,
        context: AppContext,
    ) {
        if !layout.children.is_empty() {
            self.inner.render(graphics, theme, &layout.children[0], info, context);
        }
    }
}

impl WidgetLayoutExt for FilemanSidebar {
    fn set_layout_style(&mut self, layout_style: impl Into<MaybeSignal<LayoutStyle>>) {
        self.layout_style = layout_style.into();
        // Update width from layout if specified
        // Note: Dimension doesn't have Length variant directly, it's in LengthPercentageAuto
        // For now, just store the layout style
    }
}
