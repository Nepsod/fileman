// Compatibility for the ported code.
pub use nptk::{core as nptk_core,
	macros as nptk_macros,
	services as nptk_services,
	theme as nptk_theme,
	widgets as nptk_widgets};

/// Contains the [file_list::FileList] widget.
pub mod file_list;
