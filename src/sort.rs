use npio::FileInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Size,
    Type,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortColumn {
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "size" | "1" => Self::Size,
            "type" | "2" => Self::Type,
            "date" | "modified" | "3" => Self::Modified,
            _ => Self::Name,
        }
    }
}

impl SortOrder {
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "descending" | "desc" => Self::Descending,
            _ => Self::Ascending,
        }
    }
}

pub fn sort_files(files: &mut [FileInfo], column: SortColumn, order: SortOrder) {
    files.sort_by(|left, right| {
        let comparison = match column {
            SortColumn::Name => left
                .get_name()
                .unwrap_or("")
                .to_ascii_lowercase()
                .cmp(&right.get_name().unwrap_or("").to_ascii_lowercase()),
            SortColumn::Size => left.get_size().cmp(&right.get_size()),
            SortColumn::Type => compare_ascii_ignore_case(
                left.get_content_type().unwrap_or(""),
                right.get_content_type().unwrap_or(""),
            ),
            SortColumn::Modified => modification_time(left).cmp(&modification_time(right)),
        };

        let directory_bias = directory_first(left, right);
        if directory_bias != std::cmp::Ordering::Equal {
            return directory_bias;
        }

        match order {
            SortOrder::Ascending => comparison,
            SortOrder::Descending => comparison.reverse(),
        }
    });
}

fn directory_first(left: &FileInfo, right: &FileInfo) -> std::cmp::Ordering {
    use npio::FileType;

    let left_is_directory = left.get_file_type() == FileType::Directory;
    let right_is_directory = right.get_file_type() == FileType::Directory;
    right_is_directory.cmp(&left_is_directory)
}

fn compare_ascii_ignore_case(left: &str, right: &str) -> std::cmp::Ordering {
    let mut left_chars = left.chars().map(|character| character.to_ascii_lowercase());
    let mut right_chars = right.chars().map(|character| character.to_ascii_lowercase());
    loop {
        match (left_chars.next(), right_chars.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(left_character), Some(right_character)) => {
                match left_character.cmp(&right_character) {
                    std::cmp::Ordering::Equal => {}
                    ordering => return ordering,
                }
            }
        }
    }
}

fn modification_time(file_info: &FileInfo) -> u64 {
    match file_info.get_attribute("time::modified") {
        Some(npio::FileAttributeType::Uint64(value)) => *value,
        Some(npio::FileAttributeType::Int64(value)) => *value as u64,
        _ => 0,
    }
}
