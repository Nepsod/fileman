use crate::config::FilemanConfig;

#[derive(Debug, Clone)]
pub struct SettingsDraft {
    pub show_hidden: bool,
    pub confirm_delete: bool,
    pub confirm_trash: bool,
    pub use_trash: bool,
    pub remember_window_size: bool,
    pub terminal_command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    ShowHidden,
    ConfirmDelete,
    ConfirmTrash,
    UseTrash,
    RememberWindowSize,
}

impl SettingsDraft {
    pub fn from_config(config: &FilemanConfig) -> Self {
        Self {
            show_hidden: config.folder_view.show_hidden,
            confirm_delete: config.behavior.confirm_delete,
            confirm_trash: config.behavior.confirm_trash,
            use_trash: config.behavior.use_trash,
            remember_window_size: config.window.remember_window_size,
            terminal_command: config.system.terminal.clone().unwrap_or_default(),
        }
    }

    pub fn apply_to(self, config: &mut FilemanConfig) {
        config.folder_view.show_hidden = self.show_hidden;
        config.behavior.confirm_delete = self.confirm_delete;
        config.behavior.confirm_trash = self.confirm_trash;
        config.behavior.use_trash = self.use_trash;
        config.window.remember_window_size = self.remember_window_size;
        let trimmed = self.terminal_command.trim();
        config.system.terminal = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }

    pub fn toggle(&mut self, field: SettingsField) {
        match field {
            SettingsField::ShowHidden => self.show_hidden = !self.show_hidden,
            SettingsField::ConfirmDelete => self.confirm_delete = !self.confirm_delete,
            SettingsField::ConfirmTrash => self.confirm_trash = !self.confirm_trash,
            SettingsField::UseTrash => self.use_trash = !self.use_trash,
            SettingsField::RememberWindowSize => {
                self.remember_window_size = !self.remember_window_size;
            }
        }
    }
}
