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
}
