use nptk::core::model::{ItemModel, ItemRole, ModelData, Orientation};
use nptk::core::signal::state::StateSignal;
use nptk::core::signal::Signal;
use nptk::services::filesystem::entry::FileEntry;
use humansize::{format_size, BINARY};

/// Adapter to expose a StateSignal<Vec<FileEntry>> as an ItemModel
#[derive(Clone)]
pub struct FileSystemItemModel {
    entries: StateSignal<Vec<FileEntry>>,
}

impl FileSystemItemModel {
    pub fn new(entries: StateSignal<Vec<FileEntry>>) -> Self {
        Self { entries }
    }
}

impl ItemModel for FileSystemItemModel {
    fn row_count(&self) -> usize {
        self.entries.get().len()
    }

    fn column_count(&self) -> usize {
        4 // Name, Size, Type, Date (Modified)
    }

    fn data(&self, row: usize, col: usize, role: ItemRole) -> ModelData {
        let entries = self.entries.get();
        if row >= entries.len() {
            return ModelData::None;
        }
        let entry = &entries[row];

        match role {
            ItemRole::Display => match col {
                0 => ModelData::String(entry.name.clone()),
                1 => {
                     if entry.is_dir() {
                        ModelData::String("Directory".to_string())
                     } else {
                        ModelData::String(format_size(entry.metadata.size, BINARY))
                     }
                },
                2 => ModelData::String(format!("{:?}", entry.file_type)), // Simplify for now
                3 => ModelData::String("Unknown".to_string()), // Date not in FileEntry yet?
                _ => ModelData::None,
            },
            ItemRole::Icon => {
                if col == 0 {
                    // Logic to retrieve/return icon would go here.
                    // For now, we return None, as the View handles async icon loading separately.
                    // In a full implementation, ModelData::Icon could hold a handle.
                    ModelData::None 
                } else {
                    ModelData::None
                }
            },
            ItemRole::Sort => {
                // For sorting
                match col {
                    0 => ModelData::String(entry.name.clone()),
                    1 => ModelData::Int(entry.metadata.size as i64),
                    _ => ModelData::None,
                }
            }
            _ => ModelData::None,
        }
    }

    fn header_data(&self, section: usize, orientation: Orientation, role: ItemRole) -> ModelData {
        if orientation == Orientation::Horizontal && role == ItemRole::Display {
            match section {
                0 => ModelData::String("Name".to_string()),
                1 => ModelData::String("Size".to_string()),
                2 => ModelData::String("Type".to_string()),
                3 => ModelData::String("Date Modified".to_string()),
                _ => ModelData::None,
            }
        } else {
            ModelData::None
        }
    }
}
