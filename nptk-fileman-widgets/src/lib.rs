// Compatibility for the ported code.
pub use nptk::{core as nptk_core,
	macros as nptk_macros,
	services as nptk_services,
	widgets as nptk_widgets};

/// Contains the [file_list::FileList] widget.
pub mod file_list;

/// Contains the [fileman_sidebar::FilemanSidebar] widget.
pub mod fileman_sidebar;

// Re-export for convenience
pub use fileman_sidebar::FilemanSidebar;
pub mod location_bar;
pub mod status_bar;
