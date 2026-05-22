use crate::view_mode::{
    compact_view_layout, icon_view_layout, icon_view_tile_column_stride,
    icon_view_tile_row_stride, CompactViewLayout, ICON_VIEW_PADDING_PX, IconViewLayout,
    ViewMode,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileGridMode {
    Icon,
    Compact,
}

pub fn file_entry_is_visible(name: &str, search_query: &str, show_hidden: bool) -> bool {
    if name.is_empty() {
        return false;
    }
    if !show_hidden && name.starts_with('.') {
        return false;
    }
    if search_query.is_empty() {
        return true;
    }
    name.to_ascii_lowercase()
        .contains(&search_query.to_ascii_lowercase())
}

pub fn filter_visible_file_names(
    names: &[&str],
    search_query: &str,
    show_hidden: bool,
) -> Vec<String> {
    names
        .iter()
        .filter(|name| file_entry_is_visible(name, search_query, show_hidden))
        .map(|name| (*name).to_string())
        .collect()
}

pub fn icon_tile_grid(icon_size: u32, panel_width: f32) -> IconViewLayout {
    icon_view_layout(icon_size, panel_width)
}

pub fn compact_tile_grid(panel_width: f32) -> CompactViewLayout {
    compact_view_layout(panel_width)
}

pub fn tile_slot_at_list_point(
    mode: TileGridMode,
    list_x: f32,
    list_y: f32,
    item_count: usize,
    icon_size: u32,
    panel_width: f32,
) -> Option<usize> {
    if item_count == 0 || list_x < ICON_VIEW_PADDING_PX || list_y < ICON_VIEW_PADDING_PX {
        return None;
    }

    let padding = ICON_VIEW_PADDING_PX;
    let (columns, cell_width, cell_height, column_stride, row_stride) = match mode {
        TileGridMode::Icon => {
            let layout = icon_view_layout(icon_size, panel_width);
            let columns = layout.columns.max(1);
            (
                columns,
                layout.cell_width,
                layout.cell_height,
                icon_view_tile_column_stride(layout.cell_width),
                icon_view_tile_row_stride(layout.cell_height),
            )
        }
        TileGridMode::Compact => {
            let layout = compact_view_layout(panel_width);
            let columns = layout.columns.max(1);
            (
                columns,
                layout.cell_width,
                layout.cell_height,
                layout.cell_width + layout.spacing,
                layout.row_stride,
            )
        }
    };

    let column = ((list_x - padding) / column_stride).floor() as usize;
    let row = ((list_y - padding) / row_stride).floor() as usize;
    let index = row * columns + column;
    if index >= item_count {
        return None;
    }

    let left = padding + column as f32 * column_stride;
    let top = padding + row as f32 * row_stride;
    if list_x >= left
        && list_x < left + cell_width
        && list_y >= top
        && list_y < top + cell_height
    {
        Some(index)
    } else {
        None
    }
}

pub fn tile_rectangle_selection_indices(
    anchor_index: usize,
    focus_index: usize,
    columns: usize,
    item_count: usize,
) -> Vec<usize> {
    if item_count == 0 {
        return Vec::new();
    }

    let columns = columns.max(1);
    let anchor_index = anchor_index.min(item_count - 1);
    let focus_index = focus_index.min(item_count - 1);
    let anchor_row = anchor_index / columns;
    let anchor_col = anchor_index % columns;
    let focus_row = focus_index / columns;
    let focus_col = focus_index % columns;
    let row_start = anchor_row.min(focus_row);
    let row_end = anchor_row.max(focus_row);
    let col_start = anchor_col.min(focus_col);
    let col_end = anchor_col.max(focus_col);

    let mut indices = Vec::new();
    for row in row_start..=row_end {
        for col in col_start..=col_end {
            let entry_index = row * columns + col;
            if entry_index < item_count {
                indices.push(entry_index);
            }
        }
    }
    indices
}

pub fn list_row_index_at_list_y(list_y: f32, row_height: f32, item_count: usize) -> Option<usize> {
    if item_count == 0 || list_y < 0.0 || row_height <= 0.0 {
        return None;
    }

    let content_bottom = row_height * item_count as f32;
    if list_y >= content_bottom {
        return None;
    }

    let index = (list_y / row_height).floor().max(0.0) as usize;
    if index >= item_count {
        return None;
    }

    let row_top = row_height * index as f32;
    let row_bottom = row_top + row_height;
    if list_y >= row_top && list_y < row_bottom {
        Some(index)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_entry_is_visible_respects_hidden_and_query() {
        assert!(!file_entry_is_visible("", "", true));
        assert!(!file_entry_is_visible(".hidden", "", false));
        assert!(file_entry_is_visible(".hidden", "", true));
        assert!(file_entry_is_visible("Notes.txt", "note", false));
        assert!(!file_entry_is_visible("Notes.txt", "pdf", false));
    }

    #[test]
    fn filter_visible_file_names_preserves_order() {
        let names = ["b.txt", ".a", "a.txt"];
        let visible = filter_visible_file_names(&names, "", false);
        assert_eq!(visible, vec!["b.txt", "a.txt"]);
    }

    #[test]
    fn icon_tile_slot_rejects_gutter_between_columns() {
        let grid = icon_tile_grid(48, 400.0);
        let stride = icon_view_tile_column_stride(grid.cell_width);
        let gap_x = ICON_VIEW_PADDING_PX + grid.cell_width + (stride - grid.cell_width) / 2.0;
        assert_eq!(
            tile_slot_at_list_point(TileGridMode::Icon, gap_x, ICON_VIEW_PADDING_PX + 1.0, 4, 48, 400.0),
            None
        );
        assert_eq!(
            tile_slot_at_list_point(
                TileGridMode::Icon,
                ICON_VIEW_PADDING_PX + 1.0,
                ICON_VIEW_PADDING_PX + 1.0,
                4,
                48,
                400.0
            ),
            Some(0)
        );
    }

    #[test]
    fn tile_rectangle_selection_is_grid_aligned() {
        let columns = 3;
        let indices = tile_rectangle_selection_indices(0, 4, columns, 6);
        assert_eq!(indices, vec![0, 1, 3, 4]);
    }

    #[test]
    fn list_row_index_skips_area_below_last_row() {
        assert_eq!(list_row_index_at_list_y(48.0, 24.0, 2), None);
        assert_eq!(list_row_index_at_list_y(12.0, 24.0, 2), Some(0));
    }
}
