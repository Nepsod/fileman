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
use nptk::services::bookmarks::{BookmarksService, Bookmark};
use nptk::services::thumbnail::npio_adapter::uri_to_path;
use nptk::core::app::info::AppInfo;
use nptk::core::vgi::Graphics;
use std::path::PathBuf;
use tokio::sync::mpsc;
use std::sync::Arc;
use npio::service::volumemonitor::VolumeMonitor;
// use npio::mount::Mount; // Unused
// use npio::volume::Volume; // Unused

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
    bookmarks: Vec<Bookmark>,
    devices: Vec<SidebarItem>,
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
            bookmarks: Vec::new(),
            devices: Vec::new(),
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
    bookmarks_rx: Option<mpsc::UnboundedReceiver<Vec<Bookmark>>>, // Channel for loaded bookmarks
    devices_rx: Option<mpsc::UnboundedReceiver<Vec<SidebarItem>>>, // Channel for loaded devices
    layout_style: MaybeSignal<LayoutStyle>,
    current_path: StateSignal<PathBuf>,
    last_path: Option<PathBuf>,
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
            bookmarks_rx: None,
            devices_rx: None,
            layout_style: LayoutStyle {
                size: Vector2::new(Dimension::length(200.0), Dimension::percent(1.0)),
                flex_shrink: 0.0, // Prevent sidebar from shrinking below its width
                ..Default::default()
            }
            .into(),
            current_path: StateSignal::new(PathBuf::new()),
            last_path: None,
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
        if enabled {
            // Initialize service if needed
            if self.bookmarks_service.is_none() {
                self.bookmarks_service = Some(BookmarksService::new());
            }

            // Create channel for loading results
            let (tx, rx) = mpsc::unbounded_channel();
            self.bookmarks_rx = Some(rx);

            // Spawn async loading task
            // Clone service to load in background (creates new instance with same path)
            // Note: BookmarksService::new() uses standard path. If custom path was set in original service,
            // we should ideally clone that path. But BookmarksService fields are private.
            // However, standard usage uses default path.
            let tx_clone = tx.clone();
            
            // We use a fresh service instance for loading to avoid sharing mutable state across threads
            // (Service isn't Clone/Send/Sync easily if it holds state, but BookmarksService is simple)
            // Actually BookmarksService struct fields are private.
            // Let's just create a new one with default path.
            // If custom path is needed, we'd need to expose it or make service cloneable.
            // Assuming default path for now.
            tokio::spawn(async move {
                let mut service = BookmarksService::new();
                if let Ok(_) = service.load().await {
                    let bookmarks = service.get_bookmarks();
                    let _ = tx_clone.send(bookmarks);
                }
            });
        }
        self.rebuild_sidebar();
        self
    }

    /// Enable or disable the Devices section.
    pub fn with_devices(mut self, enabled: bool) -> Self {
        self.config.show_devices = enabled;
        if enabled {
            self.start_device_monitoring();
        }
        self.rebuild_sidebar();
        self
    }
    
    fn start_device_monitoring(&mut self) {
        if self.devices_rx.is_some() {
            return;
        }
        
        // Create channel for loading results
        let (tx, rx) = mpsc::unbounded_channel();
        self.devices_rx = Some(rx);
        
        let use_symbolic = self.config.use_symbolic_icons;
        
        tokio::spawn(async move {
            let volume_monitor = Arc::new(VolumeMonitor::new());
             // Start monitoring (ignore errors for now)
            let _ = volume_monitor.start(None).await;
            
            // Helper to update list
            async fn refresh_devices(monitor: &VolumeMonitor, tx: &mpsc::UnboundedSender<Vec<SidebarItem>>, use_symbolic: bool) {
                 let mut items = Vec::new();
                 let mut added_uris = std::collections::HashSet::new();
                 
                 // Add volumes
                 let volumes = monitor.get_volumes().await;
                 
                 for volume in volumes {
                     let name = volume.get_name();
                     let mut icon = volume.get_icon();
                     if icon.is_empty() {
                         icon = if use_symbolic { "drive-harddisk-symbolic".to_string() } else { "drive-harddisk".to_string() };
                     }
                     
                     // Only show mounted volumes for now, or use mount point if available
                     if let Some(mount) = volume.get_mount() {
                         let root = mount.get_root();
                         let uri = root.uri();
                         added_uris.insert(uri.clone());
                         items.push(SidebarItem::new(uri.clone(), name).with_icon(icon).with_uri(uri));
                     }
                 }
                 
                 // Add standalone mounts that might not be volumes (e.g. network shares, custom mounts)
                 let mounts = monitor.get_mounts().await;
                 for mount in mounts {
                      let root = mount.get_root();
                      let uri = root.uri();
                      
                      // Filter out system mounts and already added volumes
                      if added_uris.contains(&uri) {
                          continue;
                      }
                      
                      let path = uri.strip_prefix("file://").unwrap_or(&uri);
                      
                      // Deny list - Ignore system paths
                      if path == "/" 
                          || path.starts_with("/dev")
                          || path.starts_with("/sys")
                          || path.starts_with("/proc")
                          || path.starts_with("/tmp")
                          || path.starts_with("/boot")
                          || path.starts_with("/var")
                          || path.starts_with("/usr")
                          || path.starts_with("/etc")
                          || path.starts_with("/bin")
                          || path.starts_with("/sbin")
                          || path.starts_with("/lib")
                          || path.starts_with("/opt")
                          || path.starts_with("/snap")
                          || path.starts_with("/run/credentials")
                          || path == "/run" 
                          || path == "/home" 
                          || path == "/cache"
                          || path == "/root"
                      {
                          continue;
                      }
                      
                      // Special handling for /run/user
                      if path.starts_with("/run/user/") {
                          // Ignore the user root itself /run/user/1000
                          // Path is like /run/user/1000 or /run/user/1000/
                          let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
                          if parts.len() <= 3 { // run, user, 1000
                               continue;
                          }
                          // Allow subdirs
                      }
                      
                      let name = mount.get_name();
                      let mut icon = mount.get_icon();
                      if icon.is_empty() {
                          icon = if use_symbolic { "folder-remote-symbolic".to_string() } else { "folder-remote".to_string() };
                      }
                      
                      added_uris.insert(uri.clone());
                      items.push(SidebarItem::new(uri.clone(), name).with_icon(icon).with_uri(uri));
                 }
                 
                 let _ = tx.send(items);
            }
            
            // Initial update
            refresh_devices(&volume_monitor, &tx, use_symbolic).await;
            
            let mut event_rx = volume_monitor.subscribe();
            
            while let Ok(_event) = event_rx.recv().await {
                // On any event, refresh the list
                refresh_devices(&volume_monitor, &tx, use_symbolic).await;
            }
        });
    }

    
    /// Get the current path signal.
    pub fn current_path_signal(&self) -> StateSignal<PathBuf> {
        self.current_path.clone()
    }

    /// Set the current path signal (for reactive updates).
    pub fn with_current_path_signal(mut self, signal: StateSignal<PathBuf>) -> Self {
        self.current_path = signal;
        self
    }

    /// Set which user directories to show in Places section.
    pub fn with_user_directories(mut self, dirs: Vec<UserDirectory>) -> Self {
        self.config.user_directories = dirs;
        self.rebuild_sidebar();
        self
    }
    
    /// Set the current path to highlight in the sidebar.
    pub fn set_current_path(&mut self, path: PathBuf) {
        let uri = format!("file://{}", path.display());
        self.inner.set_selected(Some(uri));
    }

    /// Add a custom section to the sidebar.
    pub fn with_custom_section(mut self, section: SidebarSection) -> Self {
        self.config.custom_sections.push(section);
        self.rebuild_sidebar();
        self
    }

    /// Set the width of the sidebar.
    pub fn with_width(self, width: f32) -> Self {
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
    pub fn with_symbolic_icons(self, symbolic: bool) -> Self {
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

        self.rebuild_sidebar();
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

        // Devices section
        if config.show_devices {
            if !config.devices.is_empty() {
                sections.push(SidebarSection::new("Devices").with_items(config.devices.clone()));
            } else {
                // Keep empty section or hide?
                // Hide if empty to avoid clutter
            }
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
            SidebarItem::new(format!("file://{}", home_path.display()), "Home")
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
                    SidebarItem::new(uri.clone(), label)
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
        if config.bookmarks.is_empty() {
             return None;
        }

        let mut items = Vec::new();
        for bookmark in &config.bookmarks {
            let uri = &bookmark.uri;
            let label = bookmark.name.clone().unwrap_or_else(|| {
                // If no name, use last component of path
                if let Some(path) = uri_to_path(uri) {
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Unknown".to_string())
                } else {
                    "Unknown".to_string()
                }
            });
            
            // Determine icon
            // Use bookmark icon if available, else default folder icon
            let _icon = bookmark.icon.clone().unwrap_or_else(|| {
                 get_directory_icon_name(UserDirectory::Documents, config.use_symbolic_icons).to_string()
            });
            
            // Use folder-symbolic/folder as fallback if icon invalid?
            // Actually get_directory_icon_name returns "folder-documents" etc.
            // We'll just use "user-bookmarks-symbolic" or "folder" generic.
            let display_icon = if config.use_symbolic_icons {
                "user-bookmarks-symbolic"
            } else {
                "user-bookmarks"
            };

            items.push(
                SidebarItem::new(uri.clone(), label)
                    .with_icon(display_icon)
                    .with_uri(uri.clone())
            );
        }

        Some(SidebarSection::new("Bookmarks").with_items(items))
    }
}

impl Default for FilemanSidebar {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl Widget for FilemanSidebar {
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
        // Hook signals
        context.hook_signal(&mut self.current_path);

        // Handle navigation events from channel
        // Note: The receiver should be taken and polled externally, but we can check here too
        // For now, just delegate to inner sidebar
        
        let mut needs_rebuild = false;

        // Poll for loaded bookmarks
        if let Some(ref mut rx) = self.bookmarks_rx {
             while let Ok(bookmarks) = rx.try_recv() {
                self.config.bookmarks = bookmarks;
                needs_rebuild = true;
             }
        }

        // Poll for loaded devices
        if let Some(ref mut rx) = self.devices_rx {
             while let Ok(devices) = rx.try_recv() {
                self.config.devices = devices;
                needs_rebuild = true;
             }
        }

        // Sync current path
        let current_path = (*self.current_path.get()).clone();
        if self.last_path.as_ref() != Some(&current_path) {
            self.set_current_path(current_path.clone());
            self.last_path = Some(current_path);
            needs_rebuild = true; 
        }

        if needs_rebuild {
            self.rebuild_sidebar();
            return Update::LAYOUT | Update::DRAW;
        }

        if !layout.children.is_empty() {
            self.inner.update(&layout.children[0], context, info).await
        } else {
            Update::empty()
        }
    }

    fn render(
        &mut self,
        graphics: &mut dyn Graphics,
        layout: &LayoutNode,
        info: &mut AppInfo,
        context: AppContext,
    ) {
        if !layout.children.is_empty() {
            self.inner.render(graphics, &layout.children[0], info, context);
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
