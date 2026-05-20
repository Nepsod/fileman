use nptk::std::ops::Range;

use nptk::gpui::{
    self as gpui, App, Bounds, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId,
    InteractiveElement, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement,
    PaintQuad, Pixels, Point, ShapedLine, SharedString, Style, Styled, TextRun, UTF16Selection,
    Window, actions, div, fill, point, px, relative, size,
};
use nptk::theme::ActiveTheme;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug)]
pub enum ToolbarLineInputEvent {
    Changed(String),
    Submit,
    Cancel,
}

actions!(
    toolbar_line_input,
    [
        ToolbarBackspace,
        ToolbarDelete,
        ToolbarLeft,
        ToolbarRight,
        ToolbarHome,
        ToolbarEnd,
        ToolbarSubmit,
        ToolbarCancel,
    ]
);

pub struct ToolbarLineInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

impl ToolbarLineInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
        }
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.content = text;
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for character in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += character.len_utf16();
            utf8_offset += character.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for character in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += character.len_utf8();
            utf16_offset += character.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    fn replace_text(&mut self, range: Range<usize>, new_text: &str, cx: &mut Context<Self>) {
        self.content = (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
            .into();
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.marked_range = None;
        cx.emit(ToolbarLineInputEvent::Changed(self.content.to_string()));
        cx.notify();
    }

    fn backspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let previous = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == previous {
                window.play_system_bell();
                return;
            }
            self.select_to(previous, cx);
        }
        let range = self.selected_range.clone();
        self.replace_text(range, "", cx);
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                window.play_system_bell();
                return;
            }
            self.select_to(next, cx);
        }
        let range = self.selected_range.clone();
        self.replace_text(range, "", cx);
    }
}

impl EventEmitter<ToolbarLineInputEvent> for ToolbarLineInput {}

impl EntityInputHandler for ToolbarLineInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let sanitized: String = new_text.lines().collect::<Vec<_>>().join(" ");
        self.replace_text(range, &sanitized, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let sanitized: String = new_text.lines().collect::<Vec<_>>().join(" ");
        self.content =
            (self.content[0..range.start].to_owned() + &sanitized + &self.content[range.end..])
                .into();
        self.marked_range = Some(range.start..range.start + sanitized.len());
        if let Some(selection_utf16) = new_selected_range {
            let selection = self.range_from_utf16(&selection_utf16);
            self.selected_range = selection;
        }
        cx.emit(ToolbarLineInputEvent::Changed(self.content.to_string()));
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        None
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Focusable for ToolbarLineInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl gpui::Render for ToolbarLineInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .key_context("ToolbarLineInput")
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(|this, _: &ToolbarBackspace, window, cx| {
                this.backspace(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToolbarDelete, window, cx| {
                this.delete(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToolbarLeft, _, cx| {
                if this.selected_range.is_empty() {
                    this.move_to(this.previous_boundary(this.cursor_offset()), cx);
                } else {
                    this.move_to(this.selected_range.start, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToolbarRight, _, cx| {
                if this.selected_range.is_empty() {
                    this.move_to(this.next_boundary(this.selected_range.end), cx);
                } else {
                    this.move_to(this.selected_range.end, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToolbarHome, _, cx| this.move_to(0, cx)))
            .on_action(cx.listener(|this, _: &ToolbarEnd, _, cx| {
                this.move_to(this.content.len(), cx);
            }))
            .on_action(cx.listener(|this, _: &ToolbarSubmit, _, cx| {
                cx.emit(ToolbarLineInputEvent::Submit);
            }))
            .on_action(cx.listener(|this, _: &ToolbarCancel, _, cx| {
                cx.emit(ToolbarLineInputEvent::Cancel);
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.is_selecting = true;
                if event.modifiers.shift {
                    this.select_to(this.index_for_mouse_position(event.position), cx);
                } else {
                    this.move_to(this.index_for_mouse_position(event.position), cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| {
                this.is_selecting = false;
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| {
                this.is_selecting = false;
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.is_selecting {
                    this.select_to(this.index_for_mouse_position(event.position), cx);
                }
            }))
            .child(ToolbarLineElement {
                input: cx.entity(),
            })
    }
}

struct ToolbarLineElement {
    input: Entity<ToolbarLineInput>,
}

struct ToolbarLinePrepaint {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl gpui::IntoElement for ToolbarLineElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ToolbarLineElement {
    type RequestLayoutState = ();
    type PrepaintState = ToolbarLinePrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();
        let colors = cx.theme().colors();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), colors.text_muted)
        } else {
            (content, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &[run], None);

        let cursor_position = line.x_for_index(cursor);
        let (selection, cursor_quad) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_position, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    colors.text,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    colors.element_selection_background,
                )),
                None,
            )
        };

        ToolbarLinePrepaint {
            line: Some(line),
            cursor: cursor_quad,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );

        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }

        let line = prepaint.line.take().expect("line shaped in prepaint");
        line.paint(
            bounds.origin,
            window.line_height(),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        )
        .expect("line paint");

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

pub fn register_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", ToolbarBackspace, Some("ToolbarLineInput")),
        KeyBinding::new("delete", ToolbarDelete, Some("ToolbarLineInput")),
        KeyBinding::new("left", ToolbarLeft, Some("ToolbarLineInput")),
        KeyBinding::new("right", ToolbarRight, Some("ToolbarLineInput")),
        KeyBinding::new("home", ToolbarHome, Some("ToolbarLineInput")),
        KeyBinding::new("end", ToolbarEnd, Some("ToolbarLineInput")),
        KeyBinding::new("enter", ToolbarSubmit, Some("ToolbarLineInput")),
        KeyBinding::new("escape", ToolbarCancel, Some("ToolbarLineInput")),
    ]);
}
