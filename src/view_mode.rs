#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Icon,
    Compact,
    Table,
}

impl ViewMode {
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "icon" | "thumbnail" | "thumbnails" => Self::Icon,
            "compact" => Self::Compact,
            "table" | "detailed" | "detail" => Self::Table,
            _ => Self::List,
        }
    }

    pub fn config_value(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Icon => "icon",
            Self::Compact => "compact",
            Self::Table => "table",
        }
    }

    pub fn menu_label(self) -> &'static str {
        match self {
            Self::List => "List",
            Self::Icon => "Icons",
            Self::Compact => "Compact",
            Self::Table => "Table",
        }
    }

    pub fn icon_size(self) -> u32 {
        match self {
            Self::Icon => 48,
            Self::Compact => 16,
            Self::List | Self::Table => 20,
        }
    }

    pub fn default_icon_size(self) -> u32 {
        self.icon_size()
    }
}

pub const MIN_ICON_SIZE: u32 = 16;
pub const MAX_ICON_SIZE: u32 = 256;
pub const ICON_ZOOM_STEP: u32 = 8;

pub fn clamp_icon_size(size: u32) -> u32 {
    size.clamp(MIN_ICON_SIZE, MAX_ICON_SIZE)
}

/// Matches legacy `nptk-fileman-widgets` icon view padding.
pub const ICON_VIEW_PADDING_PX: f32 = 2.0;
/// Horizontal padding on each side of an icon cell (legacy `icon_view_spacing`).
pub const ICON_VIEW_HORIZONTAL_SPACING_PX: f32 = 22.0;
/// Vertical gap between icon-view rows (legacy reused horizontal spacing here).
pub const ICON_VIEW_ROW_GAP_PX: f32 = 6.0;
/// Gap between icon tiles in the grid (marquee and background clicks use this space).
pub const ICON_VIEW_TILE_GAP_PX: f32 = 10.0;
pub const ICON_LABEL_AREA_HEIGHT_PX: f32 = 12.0 * 1.25 * 2.0;
pub const ICON_ICON_LABEL_GAP_PX: f32 = 1.0;
pub const ICON_TILE_LABEL_SHELL_PADDING_PX: f32 = 1.0;
pub const ICON_LABEL_SHELL_HORIZONTAL_PADDING_PX: f32 = 4.0;
pub const COMPACT_TILE_HORIZONTAL_PADDING_PX: f32 = 8.0;
pub const COMPACT_TILE_ICON_LABEL_GAP_PX: f32 = 10.0;
pub const COMPACT_TILE_PART_SHELL_PADDING_PX: f32 = 2.0;

#[derive(Debug, Clone, Copy)]
pub struct IconViewLayout {
    pub columns: usize,
    pub cell_width: f32,
    pub cell_height: f32,
}

pub fn icon_view_tile_column_stride(cell_width: f32) -> f32 {
    cell_width + ICON_VIEW_TILE_GAP_PX
}

pub fn icon_view_tile_row_stride(cell_height: f32) -> f32 {
    cell_height + ICON_VIEW_TILE_GAP_PX
}

pub fn icon_view_layout(icon_size: u32, panel_width: f32) -> IconViewLayout {
    let icon_size_pixels = icon_size as f32;
    let cell_width = (icon_size_pixels + ICON_VIEW_HORIZONTAL_SPACING_PX * 2.0).max(1.0);
    let available_width = (panel_width - ICON_VIEW_PADDING_PX * 2.0).max(1.0);
    let column_stride = icon_view_tile_column_stride(cell_width);
    let columns = (available_width / column_stride).floor().max(1.0) as usize;
    let cell_height = icon_size_pixels
        + ICON_ICON_LABEL_GAP_PX
        + ICON_LABEL_AREA_HEIGHT_PX
        + ICON_VIEW_ROW_GAP_PX;
    IconViewLayout {
        columns,
        cell_width,
        cell_height,
    }
}

/// Legacy compact tile dimensions (`view_compact.rs`).
pub const COMPACT_TILE_WIDTH_PX: f32 = 250.0;
pub const COMPACT_TILE_HEIGHT_PX: f32 = 68.0;
pub const COMPACT_TILE_SPACING_PX: f32 = 6.0;
pub const COMPACT_TILE_ICON_PX: u32 = 48;

/// Details (Table) view column widths — aligned with XP Details columns.
pub const TABLE_COLUMN_SIZE_PX: f32 = 72.0;
pub const TABLE_COLUMN_TYPE_PX: f32 = 100.0;
pub const TABLE_COLUMN_MODIFIED_PX: f32 = 120.0;
pub const TABLE_HEADER_HEIGHT_PX: f32 = 30.0;

/// List view row height (legacy ItemView / XP List density).
pub const LIST_ROW_HEIGHT_PX: f32 = 24.0;
pub const TABLE_ROW_HEIGHT_PX: f32 = 28.0;

#[derive(Debug, Clone, Copy)]
pub struct CompactViewLayout {
    pub columns: usize,
    pub cell_width: f32,
    pub cell_height: f32,
    pub spacing: f32,
    pub row_stride: f32,
}

pub fn compact_view_layout(panel_width: f32) -> CompactViewLayout {
    let available_width = (panel_width - ICON_VIEW_PADDING_PX * 2.0).max(1.0);
    let cell_width_plus_spacing = COMPACT_TILE_WIDTH_PX + COMPACT_TILE_SPACING_PX;
    let columns = ((available_width + COMPACT_TILE_SPACING_PX) / cell_width_plus_spacing)
        .floor()
        .max(1.0) as usize;
    let row_stride = COMPACT_TILE_HEIGHT_PX + COMPACT_TILE_SPACING_PX;
    CompactViewLayout {
        columns,
        cell_width: COMPACT_TILE_WIDTH_PX,
        cell_height: COMPACT_TILE_HEIGHT_PX,
        spacing: COMPACT_TILE_SPACING_PX,
        row_stride,
    }
}
