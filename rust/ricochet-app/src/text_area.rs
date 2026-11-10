//! A multi-line text field for GPUI, for the Request body. Each line is shaped on its own and drawn at a fixed line
//! height. Lines are not wrapped, so a long line runs off to the right. With a scroll handle, the box scrolls to keep
//! the caret in view. Typing, the caret, selection, the clipboard and the input method work as they do in
//! [`crate::text_input`], which this follows.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString, Style, TextRun,
    ScrollHandle, UTF16Selection, Window, actions, div, fill, point, prelude::*, px, relative, rgb, rgba, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::text_edit::{self, line_and_column, lines, move_vertical, offset_of};
use crate::text_input::{Backspace, Changed, Copy, Cut, Delete, End, Home, Left, Paste, Right, SelectAll, SelectLeft, SelectRight};

actions!(text_area, [Up, Down, Newline]);

const KEY_CONTEXT: &str = "TextArea";
const CARET: u32 = 0x4c8dff;
const SELECTION: u32 = 0x4c8dff55;
const PLACEHOLDER: u32 = 0x6b727d;
/// Drawn at the end of a line whose line break is selected, so the selection shows the break.
const BREAK_WIDTH: f32 = 6.0;

/// Binds the keys of the multi-line field. Enter makes a new line here; Ctrl+Enter still sends from anywhere.
pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("secondary-a", SelectAll, context),
        KeyBinding::new("secondary-v", Paste, context),
        KeyBinding::new("secondary-c", Copy, context),
        KeyBinding::new("secondary-x", Cut, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("enter", Newline, context),
    ]);
}

pub struct TextArea {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    /// The shaped lines and the box they were drawn in, from the last paint. Used for the mouse and the input method.
    last_lines: Vec<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    last_line_height: f32,
    is_selecting: bool,
    /// The box that scrolls this field, when there is one. The caret is kept inside it.
    scroll: Option<ScrollHandle>,
}

impl EventEmitter<Changed> for TextArea {}

impl TextArea {
    pub fn new(content: impl Into<SharedString>, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: content.into(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_lines: Vec::new(),
            last_bounds: None,
            last_line_height: 1.0,
            is_selecting: false,
            scroll: None,
        }
    }

    /// Keeps the caret inside the box with this scroll handle, scrolling it when the caret moves out of view.
    pub fn with_scroll(mut self, scroll: ScrollHandle) -> Self {
        self.scroll = Some(scroll);
        self
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed { self.selected_range.start } else { self.selected_range.end }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content.grapheme_indices(true).rev().find_map(|(idx, _)| (idx < offset).then_some(idx)).unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content.grapheme_indices(true).find_map(|(idx, _)| (idx > offset).then_some(idx)).unwrap_or(self.content.len())
    }

    fn edit_range(&self, range_utf16: Option<&Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|range_utf16| text_edit::range_from_utf16(&self.content, range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone())
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(move_vertical(&self.content, self.cursor_offset(), -1), cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(move_vertical(&self.content, self.cursor_offset(), 1), cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    /// Home and End go to the start and end of the current line, not of the whole text.
    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let (line, _) = line_and_column(&self.content, self.cursor_offset());
        self.move_to(offset_of(&self.content, line, 0), cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let (line, _) = line_and_column(&self.content, self.cursor_offset());
        self.move_to(offset_of(&self.content, line, usize::MAX), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx)
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace("\r\n", "\n").replace('\r', "\n"), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    /// The text offset under a point in the window, or None before the first paint.
    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), false) = (self.last_bounds, self.last_lines.is_empty()) else {
            return 0;
        };
        let line_height = self.last_line_height.max(1.0);
        let row = (f32::from(position.y - bounds.top()) / line_height).floor().max(0.0) as usize;
        let row = row.min(self.last_lines.len() - 1);
        let column = self.last_lines[row].closest_index_for_x(position.x - bounds.left());
        offset_of(&self.content, row, column)
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = text_edit::range_from_utf16(&self.content, &range_utf16);
        actual_range.replace(text_edit::range_to_utf16(&self.content, &range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(&mut self, _ignore_disabled_input: bool, _window: &mut Window, _cx: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: text_edit::range_to_utf16(&self.content, &self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|range| text_edit::range_to_utf16(&self.content, range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.edit_range(range_utf16.as_ref());
        self.content = (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.selection_reversed = false;
        self.marked_range.take();
        cx.emit(Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.edit_range(range_utf16.as_ref());
        self.content = (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        self.marked_range = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| text_edit::range_from_utf16(&self.content, range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.emit(Changed);
        cx.notify();
    }

    fn bounds_for_range(&mut self, _range_utf16: Range<usize>, _bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        None
    }

    fn character_index_for_point(&mut self, _point: Point<Pixels>, _window: &mut Window, _cx: &mut Context<Self>) -> Option<usize> {
        None
    }
}

struct TextAreaElement {
    area: Entity<TextArea>,
}

struct PrepaintState {
    lines: Vec<ShapedLine>,
    selections: Vec<PaintQuad>,
    caret: Option<PaintQuad>,
    /// The field's bounds after any scroll of the box, which the lines and the caret are drawn in.
    bounds: Bounds<Pixels>,
    line_height: Pixels,
}

impl IntoElement for TextAreaElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let line_count = lines(&self.area.read(cx).content).len().max(1);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (window.line_height() * line_count as f32).into();
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
        let area = self.area.read(cx);
        let content = area.content.clone();
        let selected = area.selected_range.clone();
        let cursor = area.cursor_offset();
        let placeholder = area.placeholder.clone();
        let scroll = area.scroll.clone();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let bounds = match scroll {
            Some(scroll) => keep_visible(&scroll, bounds, line_and_column(&content, cursor).0, line_height),
            None => bounds,
        };
        let is_placeholder = content.is_empty();
        let color = if is_placeholder { rgb(PLACEHOLDER).into() } else { style.color };

        let texts: Vec<SharedString> = if is_placeholder {
            vec![placeholder]
        } else {
            lines(&content).into_iter().map(|line| SharedString::from(line.to_string())).collect()
        };

        let mut shaped = Vec::with_capacity(texts.len());
        let mut line_start = 0;
        let mut starts = Vec::with_capacity(texts.len());
        for text in &texts {
            let run = TextRun {
                len: text.len(),
                font: style.font(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            shaped.push(window.text_system().shape_line(text.clone(), font_size, &[run], None));
            starts.push(line_start);
            line_start += text.len() + 1;
        }

        let x_at = |row: usize, column: usize| if is_placeholder { px(0.) } else { shaped[row].x_for_index(column) };
        let (cursor_row, cursor_column) = line_and_column(&content, cursor);
        let caret = (!is_placeholder && selected.is_empty()).then(|| {
            let x = bounds.left() + x_at(cursor_row, cursor_column);
            let top = bounds.top() + line_height * cursor_row as f32;
            fill(Bounds::new(point(x, top), size(px(1.5), line_height)), rgb(CARET))
        });
        let caret = caret.or_else(|| {
            (selected.is_empty() && is_placeholder).then(|| {
                fill(Bounds::new(point(bounds.left(), bounds.top()), size(px(1.5), line_height)), rgb(CARET))
            })
        });

        let mut selections = Vec::new();
        if !selected.is_empty() && !is_placeholder {
            for (row, start) in starts.iter().enumerate() {
                let end = start + texts[row].len();
                let from = selected.start.max(*start);
                let to = selected.end.min(end);
                let breaks = selected.end > end && row + 1 < texts.len();
                if from > to || (from == to && !breaks) {
                    continue;
                }
                let left = bounds.left() + x_at(row, from - start);
                let mut right = bounds.left() + x_at(row, to - start);
                if breaks {
                    right += px(BREAK_WIDTH);
                }
                let top = bounds.top() + line_height * row as f32;
                selections.push(fill(Bounds::from_corners(point(left, top), point(right, top + line_height)), rgba(SELECTION)));
            }
        }

        PrepaintState { lines: shaped, selections, caret, bounds, line_height }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let bounds = prepaint.bounds;
        let focus_handle = self.area.read(cx).focus_handle.clone();
        let is_focused = focus_handle.is_focused(window);
        window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.area.clone()), cx);
        if is_focused {
            for selection in prepaint.selections.drain(..) {
                window.paint_quad(selection);
            }
        }
        let lines = std::mem::take(&mut prepaint.lines);
        for (row, line) in lines.iter().enumerate() {
            let origin = point(bounds.left(), bounds.top() + prepaint.line_height * row as f32);
            line.paint(origin, prepaint.line_height, window, cx).ok();
        }
        if is_focused && let Some(caret) = prepaint.caret.take() {
            window.paint_quad(caret);
        }
        let line_height = f32::from(prepaint.line_height);
        self.area.update(cx, |area, _cx| {
            area.last_lines = lines;
            area.last_bounds = Some(bounds);
            area.last_line_height = line_height;
        });
    }
}

impl Render for TextArea {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .w_full()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(TextAreaElement { area: cx.entity() })
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Scrolls the box so the caret's line is fully inside it, and returns the field's bounds moved by that scroll, so
/// this frame is drawn where the box will show it. The box's scroll offset is applied when it is laid out, so the
/// shift is also taken from the bounds here rather than by asking for another frame. Does nothing before the box has
/// been laid out.
fn keep_visible(scroll: &ScrollHandle, mut bounds: Bounds<Pixels>, caret_row: usize, line_height: Pixels) -> Bounds<Pixels> {
    let view = scroll.bounds();
    if view.size.height <= px(0.) {
        return bounds;
    }
    let top = bounds.top() + line_height * caret_row as f32;
    let bottom = top + line_height;
    let shift = if bottom > view.bottom() {
        view.bottom() - bottom
    } else if top < view.top() {
        view.top() - top
    } else {
        return bounds;
    };
    let offset = scroll.offset();
    scroll.set_offset(point(offset.x, offset.y + shift));
    bounds.origin.y += shift;
    bounds
}
