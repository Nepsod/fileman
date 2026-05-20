use nptk::file_icons::FileIconPresentation;
use nptk::gpui::{self as gpui, *};
use nptk::ui::{ButtonLike, ElevationIndex, prelude::*};

pub const TOOLBAR_ICON_PIXELS: u32 = 16;
pub const SIDEBAR_ICON_PIXELS: u32 = 14;

pub const GO_BACK: &str = "go-previous";
pub const GO_FORWARD: &str = "go-next";
pub const GO_UP: &str = "go-up";
pub const REFRESH: &str = "view-refresh";
pub const COPY: &str = "edit-copy";
pub const CUT: &str = "edit-cut";
pub const PASTE: &str = "edit-paste";
pub const SEARCH: &str = "system-search";
pub const SHOW_HIDDEN: &str = "view-reveal";
pub const HIDE_HIDDEN: &str = "view-conceal";
pub const TAB_CLOSE: &str = "window-close";
pub const TAB_NEW: &str = "tab-new";
pub const FOLDER: &str = "folder";
pub const DELETE: &str = "edit-delete";
pub const PROPERTIES: &str = "document-properties";
pub const VIEW_MODE: &str = "view-list-details";

pub const TOOLBAR_THEME_ICONS: &[&str] = &[
    GO_BACK,
    GO_FORWARD,
    GO_UP,
    REFRESH,
    COPY,
    CUT,
    PASTE,
    SEARCH,
    SHOW_HIDDEN,
    HIDE_HIDDEN,
    TAB_CLOSE,
    TAB_NEW,
    FOLDER,
    DELETE,
    PROPERTIES,
    VIEW_MODE,
];

pub fn quick_access_theme_icon(label: &str) -> Option<&'static str> {
    match label {
        "Home" => Some("user-home"),
        "Desktop" => Some("folder-desktop"),
        "Documents" => Some("folder-documents"),
        "Downloads" => Some("folder-download"),
        "Music" => Some("folder-music"),
        "Pictures" => Some("folder-pictures"),
        "Videos" => Some("folder-videos"),
        "Root" => Some(FOLDER),
        _ => Some(FOLDER),
    }
}

pub fn cached_icon_element(
    presentation: Option<FileIconPresentation>,
    icon_size: IconSize,
    icon_color: Color,
    cx: &App,
) -> AnyElement {
    let Some(presentation) = presentation else {
        return Empty.into_any_element();
    };
    presentation_element(presentation, icon_size, icon_color, cx)
}

pub fn presentation_element(
    presentation: FileIconPresentation,
    icon_size: IconSize,
    icon_color: Color,
    cx: &App,
) -> AnyElement {
    match presentation {
        FileIconPresentation::RenderImage(image) => img(ImageSource::Render(image))
            .size(icon_size.rems())
            .into_any_element(),
        FileIconPresentation::SvgPath(path) => svg()
            .external_path(path)
            .size(icon_size.rems())
            .flex_none()
            .text_color(icon_color.color(cx))
            .into_any_element(),
        FileIconPresentation::RasterPath(path) => Icon::from_path(path)
            .size(icon_size)
            .color(icon_color)
            .into_any_element(),
    }
}

fn presentation_to_icon(
    presentation: FileIconPresentation,
    icon_size: IconSize,
    icon_color: Color,
) -> Icon {
    match presentation {
        FileIconPresentation::RenderImage(_) => Icon::from_path(FOLDER),
        FileIconPresentation::SvgPath(path) => Icon::from_external_svg(path.into())
            .size(icon_size)
            .color(icon_color),
        FileIconPresentation::RasterPath(path) => Icon::from_path(path)
            .size(icon_size)
            .color(icon_color),
    }
}

pub fn cached_theme_icon(
    presentation: Option<FileIconPresentation>,
    icon_size: IconSize,
    icon_color: Color,
) -> Option<Icon> {
    presentation.map(|presentation| presentation_to_icon(presentation, icon_size, icon_color))
}

#[derive(IntoElement)]
pub struct ThemeIconButton {
    base: ButtonLike,
    icon_size: IconSize,
    icon_color: Color,
    cached_presentation: Option<FileIconPresentation>,
    selected_icon_color: Option<Color>,
    selected_style: Option<ButtonStyle>,
    disabled: bool,
    selected: bool,
}

impl ThemeIconButton {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: ButtonLike::new(id),
            icon_size: IconSize::default(),
            icon_color: Color::Default,
            cached_presentation: None,
            selected_icon_color: None,
            selected_style: None,
            disabled: false,
            selected: false,
        }
    }

    pub fn cached(mut self, presentation: Option<FileIconPresentation>) -> Self {
        self.cached_presentation = presentation;
        self
    }

    pub fn icon_size(mut self, icon_size: IconSize) -> Self {
        self.icon_size = icon_size;
        self
    }
}

impl Disableable for ThemeIconButton {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self.base = self.base.disabled(disabled);
        self
    }
}

impl Toggleable for ThemeIconButton {
    fn toggle_state(mut self, selected: bool) -> Self {
        self.selected = selected;
        self.base = self.base.toggle_state(selected);
        self
    }
}

impl SelectableButton for ThemeIconButton {
    fn selected_style(mut self, style: ButtonStyle) -> Self {
        self.selected_style = Some(style);
        self.base = self.base.selected_style(style);
        self
    }
}

impl Clickable for ThemeIconButton {
    fn on_click(
        mut self,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.base = self.base.on_click(handler);
        self
    }

    fn cursor_style(mut self, cursor_style: gpui::CursorStyle) -> Self {
        self.base = self.base.cursor_style(cursor_style);
        self
    }
}

impl FixedWidth for ThemeIconButton {
    fn width(mut self, width: impl Into<DefiniteLength>) -> Self {
        self.base = self.base.width(width);
        self
    }

    fn full_width(mut self) -> Self {
        self.base = self.base.full_width();
        self
    }
}

impl ButtonCommon for ThemeIconButton {
    fn id(&self) -> &ElementId {
        self.base.id()
    }

    fn style(mut self, style: ButtonStyle) -> Self {
        self.base = self.base.style(style);
        self
    }

    fn size(mut self, size: ButtonSize) -> Self {
        self.base = self.base.size(size);
        self
    }

    fn tooltip(mut self, tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.base = self.base.tooltip(tooltip);
        self
    }

    fn tab_index(mut self, tab_index: impl Into<isize>) -> Self {
        self.base = self.base.tab_index(tab_index);
        self
    }

    fn layer(mut self, elevation: ElevationIndex) -> Self {
        self.base = self.base.layer(elevation);
        self
    }

    fn track_focus(mut self, focus_handle: &FocusHandle) -> Self {
        self.base = self.base.track_focus(focus_handle);
        self
    }
}

impl RenderOnce for ThemeIconButton {
    #[allow(refining_impl_trait)]
    fn render(self, _window: &mut Window, cx: &mut App) -> ButtonLike {
        let icon_color = if self.disabled {
            Color::Disabled
        } else if self.selected_style.is_some() && self.selected {
            self.selected_style.unwrap().into()
        } else if self.selected {
            self.selected_icon_color.unwrap_or(Color::Selected)
        } else {
            self.icon_color
        };

        self.base.child(cached_icon_element(
            self.cached_presentation,
            self.icon_size,
            icon_color,
            cx,
        ))
    }
}
