use super::fuzzy::Query;
use super::icons::Icon;
use super::theme::Theme;
use crate::ipc::{self, Request, Response};
use crate::store::{self, Content, Entry, Kind};
use gpui::{
    AnyElement, BoxShadow, ClickEvent, Context, Decorations, FocusHandle, FontWeight,
    HighlightStyle, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ObjectFit, Pixels, Point, ScrollHandle, ScrollStrategy, SharedString, StyledText, Task,
    TextLayout, Timer, UniformListScrollHandle, Window, div, img, point, prelude::*, px, relative,
    rgba, svg, uniform_list,
};
use std::cell::RefCell;
use std::ops::Range;
use std::time::{Duration, Instant};

const ROW_H: f32 = 54.;
const SHADOW: f32 = 14.;
const RADIUS: f32 = 14.;
const UI_FONT: &str = "Cantarell";
const MONO_FONT: &str = "Noto Sans Mono";
const PREVIEW_LIMIT: usize = 20_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Pinned,
    Text,
    Links,
    Code,
    Images,
}

impl Filter {
    const ALL: [Filter; 6] = [
        Filter::All,
        Filter::Pinned,
        Filter::Text,
        Filter::Links,
        Filter::Code,
        Filter::Images,
    ];

    fn label(self) -> &'static str {
        match self {
            Filter::All => "Все",
            Filter::Pinned => "Закреплённые",
            Filter::Text => "Текст",
            Filter::Links => "Ссылки",
            Filter::Code => "Код",
            Filter::Images => "Картинки",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Filter::All => Icon::Layers,
            Filter::Pinned => Icon::Pin,
            Filter::Text => Icon::Text,
            Filter::Links => Icon::Link,
            Filter::Code => Icon::Code,
            Filter::Images => Icon::Image,
        }
    }

    fn accepts(self, item: &Item) -> bool {
        match self {
            Filter::All => true,
            Filter::Pinned => item.entry.pinned,
            Filter::Text => matches!(item.kind, Kind::Text | Kind::Color | Kind::Path),
            Filter::Links => item.kind == Kind::Link,
            Filter::Code => item.kind == Kind::Code,
            Filter::Images => item.kind == Kind::Image,
        }
    }
}

struct Item {
    entry: Entry,
    kind: Kind,
    /// Single-line preview shown in the list.
    title: SharedString,
    /// Lowercased text used for searching.
    search: String,
    lines: usize,
    chars: usize,
    color: Option<u32>,
}

impl Item {
    fn new(entry: Entry) -> Self {
        let kind = entry.kind();
        match &entry.content {
            Content::Text { text } => {
                let first = text
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .unwrap_or("");
                let title: String = first.replace('\t', "  ").chars().take(300).collect();
                Item {
                    kind,
                    title: title.into(),
                    search: text
                        .chars()
                        .take(64 * 1024)
                        .flat_map(char::to_lowercase)
                        .collect(),
                    lines: text.lines().count().max(1),
                    chars: text.chars().count(),
                    color: (kind == Kind::Color)
                        .then(|| store::parse_color(text))
                        .flatten(),
                    entry,
                }
            }
            Content::Image { width, height, .. } => Item {
                kind,
                title: format!("Изображение {width}×{height}").into(),
                search: format!("изображение картинка image png {width}x{height}"),
                lines: 0,
                chars: 0,
                color: None,
                entry,
            },
        }
    }
}

/// Mouse selection inside the preview text (byte offsets into the shown text).
struct TextSelection {
    entry_id: u64,
    anchor: usize,
    head: usize,
}

/// The preview text as laid out in the last frame, for hit-testing the mouse.
struct PreviewText {
    entry_id: u64,
    text: SharedString,
    layout: TextLayout,
}

struct Toast {
    text: SharedString,
    icon: Icon,
    error: bool,
    until: Instant,
}

pub struct ClipView {
    focus: FocusHandle,
    theme: Theme,
    items: Vec<Item>,
    /// Indices into `items`, in display order.
    visible: Vec<usize>,
    query: String,
    filter: Filter,
    selected: usize,
    list_scroll: UniformListScrollHandle,
    caret_on: bool,
    toast: Option<Toast>,
    confirm_clear_until: Option<Instant>,
    show_help: bool,
    error: Option<String>,
    was_active: bool,
    text_selection: Option<TextSelection>,
    dragging: bool,
    preview_text: RefCell<Option<PreviewText>>,
    preview_scroll: ScrollHandle,
    _ticker: Task<()>,
}

impl ClipView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe_window_appearance(window, |this, window, cx| {
            this.theme = Theme::for_appearance(window.appearance());
            cx.notify();
        })
        .detach();

        // Behave like a popup: disappear as soon as focus goes elsewhere.
        if std::env::var_os("CLIPVIBE_KEEP_OPEN").is_none() {
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.was_active = true;
                    return;
                }
                if !this.was_active {
                    return;
                }
                // Ignore a momentary blur (e.g. compositor re-configuring the surface).
                cx.spawn_in(window, async move |_, cx| {
                    Timer::after(Duration::from_millis(150)).await;
                    let _ = cx.update(|window, cx| {
                        if !window.is_window_active() {
                            cx.quit();
                        }
                    });
                })
                .detach();
            })
            .detach();
        }

        let ticker = cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(530)).await;
                let alive = this.update(cx, |this, cx| {
                    this.caret_on = !this.caret_on;
                    let now = Instant::now();
                    if this.toast.as_ref().is_some_and(|t| t.until <= now) {
                        this.toast = None;
                    }
                    if this.confirm_clear_until.is_some_and(|u| u <= now) {
                        this.confirm_clear_until = None;
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        let mut this = Self {
            focus: cx.focus_handle(),
            theme: Theme::for_appearance(window.appearance()),
            items: Vec::new(),
            visible: Vec::new(),
            query: String::new(),
            filter: Filter::All,
            selected: 0,
            list_scroll: UniformListScrollHandle::new(),
            caret_on: true,
            toast: None,
            confirm_clear_until: None,
            show_help: false,
            error: None,
            was_active: false,
            text_selection: None,
            dragging: false,
            preview_text: RefCell::new(None),
            preview_scroll: ScrollHandle::new(),
            _ticker: ticker,
        };
        this.reload();
        this
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    // ---- data ---------------------------------------------------------------

    fn reload(&mut self) {
        let keep = self.selected_item().map(|i| i.entry.id);
        match ipc::ensure_daemon().and_then(|_| ipc::send(&Request::List)) {
            Ok(Response::Entries { entries }) => {
                self.items = entries.into_iter().map(Item::new).collect();
                self.error = None;
            }
            Ok(other) => self.error = Some(format!("Неожиданный ответ демона: {other:?}")),
            Err(err) => self.error = Some(format!("Демон недоступен: {err:#}")),
        }
        self.refilter();
        if let Some(id) = keep
            && let Some(pos) = self
                .visible
                .iter()
                .position(|&i| self.items[i].entry.id == id)
        {
            self.selected = pos;
        }
    }

    fn refilter(&mut self) {
        let query = Query::new(&self.query);
        let mut scored: Vec<(i64, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| self.filter.accepts(it))
            .filter_map(|(i, it)| {
                if query.is_empty() {
                    Some((0, i))
                } else {
                    query.score(&it.search, &it.title).map(|s| (s, i))
                }
            })
            .collect();
        // Stable sort keeps recency order among equal scores.
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        self.visible = scored.into_iter().map(|(_, i)| i).collect();
        self.selected = 0;
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn selected_item(&self) -> Option<&Item> {
        self.visible.get(self.selected).map(|&i| &self.items[i])
    }

    fn count_for(&self, filter: Filter) -> usize {
        self.items.iter().filter(|it| filter.accepts(it)).count()
    }

    // ---- actions ------------------------------------------------------------

    fn select(&mut self, ix: usize) {
        if self.visible.is_empty() {
            return;
        }
        let ix = ix.min(self.visible.len() - 1);
        let strategy = if ix >= self.selected {
            ScrollStrategy::Bottom
        } else {
            ScrollStrategy::Top
        };
        self.selected = ix;
        self.list_scroll.scroll_to_item(ix, strategy);
    }

    fn move_by(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() as isize - 1;
        let next = self.selected as isize + delta;
        // Wrap around only on single steps, clamp on page jumps.
        let next = if delta.abs() == 1 {
            if next < 0 {
                last
            } else if next > last {
                0
            } else {
                next
            }
        } else {
            next.clamp(0, last)
        };
        self.select(next as usize);
    }

    fn copy(&mut self, ix: usize, close: bool, cx: &mut Context<Self>) {
        let Some(&item_ix) = self.visible.get(ix) else {
            return;
        };
        let id = self.items[item_ix].entry.id;
        match ipc::send(&Request::Copy { id }) {
            Ok(Response::Ok) if close => cx.quit(),
            Ok(Response::Ok) => {
                self.reload();
                self.flash("Скопировано в буфер обмена", Icon::Check, false);
            }
            Ok(Response::Error { message }) => self.flash(message, Icon::Clipboard, true),
            Ok(_) => {}
            Err(err) => self.flash(format!("{err:#}"), Icon::Clipboard, true),
        }
    }

    fn toggle_pin(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let (id, was) = (item.entry.id, item.entry.pinned);
        if self.request(Request::TogglePin { id }) {
            let pos = self.selected;
            self.reload_keeping_position(pos);
            if was {
                self.flash("Откреплено", Icon::Pin, false);
            } else {
                self.flash("Закреплено — не удалится при очистке", Icon::Pin, false);
            }
        }
    }

    fn delete_selected(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let id = item.entry.id;
        if self.request(Request::Delete { id }) {
            let pos = self.selected;
            self.reload_keeping_position(pos);
            self.flash("Удалено", Icon::Trash, false);
        }
    }

    fn clear_unpinned(&mut self) {
        let armed = self.confirm_clear_until.is_some_and(|u| u > Instant::now());
        if !armed {
            self.confirm_clear_until = Some(Instant::now() + Duration::from_secs(3));
            self.flash(
                "Нажмите ещё раз, чтобы очистить всё незакреплённое",
                Icon::Trash,
                true,
            );
            return;
        }
        self.confirm_clear_until = None;
        if self.request(Request::ClearUnpinned) {
            self.reload();
            self.flash("История очищена", Icon::Trash, false);
        }
    }

    fn open_external(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let target = match (&item.entry.content, item.kind) {
            (Content::Text { text }, Kind::Link | Kind::Path) => {
                let t = text.trim();
                match t.strip_prefix("~/") {
                    Some(rest) => dirs::home_dir()
                        .map(|h| h.join(rest).display().to_string())
                        .unwrap_or(t.to_string()),
                    None => t.to_string(),
                }
            }
            (Content::Image { file, .. }, _) => store::image_path(file).display().to_string(),
            _ => {
                self.flash("Открыть можно ссылку, путь или картинку", Icon::Link, true);
                return;
            }
        };
        match std::process::Command::new("xdg-open").arg(&target).spawn() {
            Ok(_) => self.flash("Открываю…", Icon::Link, false),
            Err(err) => self.flash(format!("xdg-open: {err}"), Icon::Link, true),
        }
    }

    fn reload_keeping_position(&mut self, pos: usize) {
        self.reload();
        self.select(pos);
    }

    fn request(&mut self, req: Request) -> bool {
        match ipc::send(&req) {
            Ok(Response::Ok) => true,
            Ok(Response::Error { message }) => {
                self.flash(message, Icon::Clipboard, true);
                false
            }
            Ok(_) => false,
            Err(err) => {
                self.flash(format!("{err:#}"), Icon::Clipboard, true);
                false
            }
        }
    }

    fn flash(&mut self, text: impl Into<SharedString>, icon: Icon, error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            icon,
            error,
            until: Instant::now() + Duration::from_millis(if error { 2600 } else { 1600 }),
        });
    }

    fn set_filter(&mut self, filter: Filter) {
        if self.filter != filter {
            self.filter = filter;
            self.refilter();
        }
    }

    fn cycle_filter(&mut self, delta: isize) {
        let n = Filter::ALL.len() as isize;
        let cur = Filter::ALL
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0) as isize;
        self.set_filter(Filter::ALL[((cur + delta).rem_euclid(n)) as usize]);
    }

    // ---- text selection in the preview ---------------------------------------

    fn selection_range(&self) -> Option<Range<usize>> {
        let sel = self.text_selection.as_ref()?;
        let current = self.selected_item()?.entry.id;
        (sel.entry_id == current && sel.anchor != sel.head)
            .then(|| sel.anchor.min(sel.head)..sel.anchor.max(sel.head))
    }

    fn selected_fragment(&self) -> Option<String> {
        let range = self.selection_range()?;
        let preview = self.preview_text.borrow();
        let preview = preview.as_ref()?;
        let id = self.text_selection.as_ref()?.entry_id;
        (preview.entry_id == id)
            .then(|| preview.text.get(range).map(str::to_string))
            .flatten()
    }

    /// Byte offset of the character under `position` in the preview text.
    fn text_index_at(&self, position: Point<Pixels>) -> Option<(u64, usize)> {
        let preview = self.preview_text.borrow();
        let preview = preview.as_ref()?;
        // The layout panics if it hasn't been painted yet; treat that as "no hit".
        let hit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            preview.layout.index_for_position(position)
        }));
        let Ok(Ok(ix) | Err(ix)) = hit else {
            return None;
        };
        let mut ix = ix.min(preview.text.len());
        while !preview.text.is_char_boundary(ix) {
            ix -= 1;
        }
        Some((preview.entry_id, ix))
    }

    fn preview_word_or_line(&self, ix: usize, line: bool) -> Range<usize> {
        let preview = self.preview_text.borrow();
        let Some(text) = preview.as_ref().map(|p| p.text.as_str()) else {
            return ix..ix;
        };
        let is_part = |c: char| {
            if line {
                c != '\n'
            } else {
                c.is_alphanumeric() || c == '_'
            }
        };
        let start = text[..ix]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_part(*c))
            .last()
            .map_or(ix, |(i, _)| i);
        let end = text[ix..]
            .char_indices()
            .find(|(_, c)| !is_part(*c))
            .map_or(text.len(), |(i, _)| ix + i);
        start..end
    }

    pub(super) fn on_text_mouse_down(
        &mut self,
        ev: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((entry_id, ix)) = self.text_index_at(ev.position) else {
            return;
        };
        match ev.click_count {
            1 => {
                let extend = ev.modifiers.shift
                    && self
                        .text_selection
                        .as_ref()
                        .is_some_and(|s| s.entry_id == entry_id);
                match (&mut self.text_selection, extend) {
                    (Some(sel), true) => sel.head = ix,
                    _ => {
                        self.text_selection = Some(TextSelection {
                            entry_id,
                            anchor: ix,
                            head: ix,
                        })
                    }
                }
            }
            n => {
                let r = self.preview_word_or_line(ix, n >= 3);
                self.text_selection = Some(TextSelection {
                    entry_id,
                    anchor: r.start,
                    head: r.end,
                });
            }
        }
        self.dragging = true;
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn on_mouse_move(
        &mut self,
        ev: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.dragging {
            return;
        }
        if ev.pressed_button != Some(MouseButton::Left) {
            self.dragging = false;
            return;
        }
        // Dragging past the top/bottom edge scrolls the preview.
        let bounds = self.preview_scroll.bounds();
        let overshoot = if ev.position.y < bounds.top() {
            bounds.top() - ev.position.y
        } else if ev.position.y > bounds.bottom() {
            bounds.bottom() - ev.position.y
        } else {
            px(0.)
        };
        if overshoot != px(0.) {
            let mut offset = self.preview_scroll.offset();
            let max = self.preview_scroll.max_offset().height;
            offset.y = (offset.y + overshoot * 0.5).clamp(-max, px(0.));
            self.preview_scroll.set_offset(offset);
        }
        if let Some((entry_id, ix)) = self.text_index_at(ev.position)
            && let Some(sel) = &mut self.text_selection
            && sel.entry_id == entry_id
        {
            sel.head = ix;
        }
        cx.notify();
    }

    pub(super) fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.dragging = false;
    }

    fn select_all_text(&mut self) {
        let preview = self.preview_text.borrow();
        let Some(p) = preview.as_ref() else { return };
        let sel = TextSelection {
            entry_id: p.entry_id,
            anchor: 0,
            head: p.text.len(),
        };
        drop(preview);
        self.text_selection = Some(sel);
    }

    fn copy_text(&mut self, text: String, close: bool, cx: &mut Context<Self>) {
        let chars = text.chars().count();
        match ipc::send(&Request::CopyText { text }) {
            Ok(Response::Ok) if close => cx.quit(),
            Ok(Response::Ok) => self.flash(
                format!(
                    "Скопирован фрагмент · {}",
                    plural(chars, "символ", "символа", "символов")
                ),
                Icon::Check,
                false,
            ),
            Ok(Response::Error { message }) => self.flash(message, Icon::Clipboard, true),
            Ok(_) => {}
            Err(err) => self.flash(format!("{err:#}"), Icon::Clipboard, true),
        }
    }

    // ---- keyboard -------------------------------------------------------------

    fn on_key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        let (ctrl, alt, shift) = (m.control, m.alt, m.shift);
        let key = ks.key.as_str();
        self.caret_on = true;

        if self.show_help && matches!(key, "escape" | "f1") {
            self.show_help = false;
            cx.notify();
            return;
        }

        match key {
            "escape" if self.selection_range().is_some() => self.text_selection = None,
            "escape" if !self.query.is_empty() => {
                self.query.clear();
                self.refilter();
            }
            "escape" => cx.quit(),
            "w" | "q" if ctrl => cx.quit(),
            "enter" => match self.selected_fragment() {
                Some(fragment) => self.copy_text(fragment, !shift && !ctrl, cx),
                None => self.copy(self.selected, !shift && !ctrl, cx),
            },
            "c" if ctrl => match self.selected_fragment() {
                Some(fragment) => self.copy_text(fragment, false, cx),
                None => self.copy(self.selected, false, cx),
            },
            "a" if ctrl => self.select_all_text(),
            "up" if alt => self.select(0),
            "down" if alt => self.select(usize::MAX),
            "up" => self.move_by(-1),
            "down" => self.move_by(1),
            "k" | "p" if ctrl => self.move_by(-1),
            "j" | "n" if ctrl => self.move_by(1),
            "pageup" => self.move_by(-8),
            "pagedown" => self.move_by(8),
            "home" => self.select(0),
            "end" => self.select(usize::MAX),
            "tab" => self.cycle_filter(if shift { -1 } else { 1 }),
            "s" if ctrl => self.toggle_pin(),
            "delete" | "backspace" if ctrl && shift => self.clear_unpinned(),
            "delete" => self.delete_selected(),
            "d" if ctrl => self.delete_selected(),
            "o" if ctrl => self.open_external(),
            "f1" => self.show_help = !self.show_help,
            "/" if ctrl => self.show_help = !self.show_help,
            "u" if ctrl => {
                self.query.clear();
                self.refilter();
            }
            "backspace" if ctrl || alt => {
                let trimmed = self.query.trim_end();
                let cut = trimmed
                    .char_indices()
                    .rev()
                    .find(|(_, c)| c.is_whitespace())
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                self.query.truncate(cut);
                self.refilter();
            }
            "backspace" => {
                self.query.pop();
                self.refilter();
            }
            k if (ctrl || alt) && k.len() == 1 && k.as_bytes()[0].is_ascii_digit() && k != "0" => {
                let ix = (k.as_bytes()[0] - b'1') as usize;
                if ix < self.visible.len() {
                    self.copy(ix, !shift, cx);
                }
            }
            _ => {
                let typed = (!ctrl && !alt && !m.platform)
                    .then(|| ks.key_char.as_deref())
                    .flatten()
                    .filter(|s| !s.chars().any(char::is_control));
                match typed {
                    Some(s) => {
                        self.query.push_str(s);
                        self.refilter();
                    }
                    None => return,
                }
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
}

// ---- rendering --------------------------------------------------------------------

fn kbd(t: &Theme, label: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(5.))
        .h(px(18.))
        .min_w(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .bg(t.kbd_bg)
        .border_1()
        .border_color(t.border)
        .text_color(t.text_muted)
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .child(label.into())
}

fn icon(i: Icon, size: f32, color: Hsla) -> impl IntoElement {
    svg()
        .path(i.path())
        .flex_none()
        .size(px(size))
        .text_color(color)
}

fn plural(n: usize, one: &str, few: &str, many: &str) -> String {
    let (m10, m100) = (n % 10, n % 100);
    let word = if m10 == 1 && m100 != 11 {
        one
    } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
        few
    } else {
        many
    };
    format!("{n} {word}")
}

fn ago(ts: i64) -> String {
    let d = (store::now() - ts).max(0);
    match d {
        0..45 => "только что".into(),
        45..3600 => format!("{} мин", (d / 60).max(1)),
        3600..86400 => format!("{} ч", d / 3600),
        86400..172800 => "вчера".into(),
        _ if d < 7 * 86400 => format!("{} дн", d / 86400),
        _ if d < 30 * 86400 => format!("{} нед", d / (7 * 86400)),
        _ => format!("{} мес", d / (30 * 86400)),
    }
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Text => "Текст",
        Kind::Link => "Ссылка",
        Kind::Color => "Цвет",
        Kind::Path => "Путь",
        Kind::Code => "Код",
        Kind::Image => "Картинка",
    }
}

fn color_hsla(c: u32) -> Hsla {
    rgba(c).into()
}

impl ClipView {
    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = &self.theme;
        let caret = div()
            .flex_none()
            .w(px(2.))
            .h(px(20.))
            .rounded(px(1.))
            .bg(t.accent)
            .when(!self.caret_on, |d| d.opacity(0.));
        let field = if self.query.is_empty() {
            div().flex().items_center().child(caret).child(
                div()
                    .ml(px(2.))
                    .text_color(t.text_faint)
                    .child("Поиск по истории…"),
            )
        } else {
            div()
                .flex()
                .items_center()
                .min_w_0()
                .overflow_hidden()
                .child(div().whitespace_nowrap().child(self.query.clone()))
                .child(caret)
        };
        let counter = if self.query.is_empty() && self.filter == Filter::All {
            plural(self.items.len(), "запись", "записи", "записей")
        } else {
            format!("{} из {}", self.visible.len(), self.items.len())
        };

        div()
            .id("header")
            .flex_none()
            .h(px(58.))
            .px(px(18.))
            .flex()
            .items_center()
            .gap(px(12.))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .child(icon(Icon::Search, 18., t.text_muted))
            .child(div().flex_1().min_w_0().text_size(px(17.)).child(field))
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(t.text_faint)
                    .child(counter),
            )
            .child(
                div()
                    .id("help-btn")
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.show_help = !this.show_help;
                        cx.notify();
                    }))
                    .child(kbd(t, "F1")),
            )
    }

    fn render_filters(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        div()
            .flex_none()
            .px(px(14.))
            .pb(px(10.))
            .flex()
            .gap(px(6.))
            .border_b_1()
            .border_color(t.border)
            .children(Filter::ALL.into_iter().map(|f| {
                let active = f == self.filter;
                let count = self.count_for(f);
                div()
                    .id(SharedString::from(f.label()))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(28.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_size(px(12.5))
                    .border_1()
                    .map(|d| {
                        if active {
                            d.bg(t.accent_soft)
                                .border_color(t.accent_soft)
                                .text_color(t.accent)
                        } else {
                            d.border_color(gpui::transparent_black())
                                .text_color(t.text_muted)
                                .hover(|s| s.bg(t.hover).text_color(t.text))
                        }
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.set_filter(f);
                        cx.notify();
                    }))
                    .child(icon(
                        f.icon(),
                        13.,
                        if active { t.accent } else { t.text_faint },
                    ))
                    .child(f.label())
                    .when(count > 0 && f != Filter::All, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(if active { t.accent } else { t.text_faint })
                                .child(count.to_string()),
                        )
                    })
            }))
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let item = &self.items[self.visible[ix]];
        let selected = ix == self.selected;
        let tint = t.kind_color(item.kind);

        let badge = match (&item.entry.content, item.color) {
            (Content::Image { file, .. }, _) => div()
                .size(px(34.))
                .flex_none()
                .rounded(px(8.))
                .overflow_hidden()
                .border_1()
                .border_color(t.border)
                .child(
                    img(store::image_path(file))
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                ),
            (_, Some(c)) => div()
                .size(px(34.))
                .flex_none()
                .rounded(px(8.))
                .border_1()
                .border_color(t.border_strong)
                .bg(color_hsla(c)),
            _ => div()
                .size(px(34.))
                .flex_none()
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .bg(tint.opacity(0.13))
                .child(icon(Icon::for_kind(item.kind), 16., tint)),
        };

        let query = Query::new(&self.query);
        let highlights = query.highlights(&item.title).into_iter().map(|r| {
            (
                r,
                HighlightStyle {
                    color: Some(t.match_fg),
                    background_color: Some(t.match_bg),
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
            )
        });
        let title = StyledText::new(item.title.clone()).with_highlights(highlights);

        let mut meta = vec![kind_label(item.kind).to_string()];
        match &item.entry.content {
            Content::Text { .. } if item.lines > 1 => {
                meta.push(plural(item.lines, "строка", "строки", "строк"))
            }
            Content::Text { .. } if item.kind == Kind::Text => {
                meta.push(plural(item.chars, "символ", "символа", "символов"))
            }
            _ => {}
        }
        meta.push(ago(item.entry.last_used));

        div()
            .id(("row", ix))
            .h(px(ROW_H))
            .px(px(8.))
            .child(
                div()
                    .id(("row-inner", ix))
                    .size_full()
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .rounded(px(10.))
                    .cursor_pointer()
                    .map(|d| {
                        if selected {
                            d.bg(t.selected)
                        } else {
                            d.hover(|s| s.bg(t.hover))
                        }
                    })
                    .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                        if ev.click_count() >= 2 {
                            this.copy(ix, true, cx);
                        } else {
                            this.selected = ix;
                        }
                        cx.notify();
                    }))
                    .child(badge)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.5))
                                    .text_color(t.text)
                                    .when(matches!(item.kind, Kind::Code | Kind::Path), |d| {
                                        d.font_family(MONO_FONT).text_size(px(12.5))
                                    })
                                    .when(item.kind == Kind::Link, |d| d.text_color(t.link))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(t.text_faint)
                                    .child(meta.join("  ·  ")),
                            ),
                    )
                    .when(item.entry.pinned, |d| {
                        d.child(icon(Icon::Pin, 14., t.accent))
                    })
                    .when(ix < 9, |d| {
                        d.child(
                            div()
                                .when(!selected, |d| d.opacity(0.55))
                                .child(kbd(&t, format!("{}", ix + 1))),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        if self.visible.is_empty() {
            let (title, hint) = if let Some(err) = &self.error {
                ("Нет связи с демоном".to_string(), err.clone())
            } else if self.items.is_empty() {
                (
                    "История пуста".to_string(),
                    "Скопируйте что-нибудь — оно появится здесь".to_string(),
                )
            } else if !self.query.is_empty() {
                (
                    format!("Ничего не найдено по «{}»", self.query),
                    "Esc — сбросить поиск, Tab — другой фильтр".to_string(),
                )
            } else {
                (
                    format!("В «{}» пока пусто", self.filter.label()),
                    "Tab — следующий фильтр".to_string(),
                )
            };
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .p(px(24.))
                .child(
                    div()
                        .size(px(56.))
                        .rounded(px(16.))
                        .bg(t.accent_soft)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(Icon::Clipboard, 26., t.accent)),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(t.text_muted)
                        .text_center()
                        .child(hint),
                )
                .into_any_element();
        }
        uniform_list(
            "history",
            self.visible.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                range.map(|ix| this.render_row(ix, cx)).collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.list_scroll.clone())
        .size_full()
        .py(px(6.))
        .into_any_element()
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        *self.preview_text.borrow_mut() = None;
        let pane = div()
            .flex_none()
            .w(relative(0.42))
            .h_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(14.))
            .bg(t.panel)
            .border_l_1()
            .border_color(t.border);
        let Some(item) = self.selected_item() else {
            return pane;
        };
        let tint = t.kind_color(item.kind);

        let action = |id: &'static str, i: Icon, color: Hsla, active: bool| {
            div()
                .id(id)
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .cursor_pointer()
                .when(active, |d| d.bg(t.accent_soft))
                .hover(|s| s.bg(t.hover))
                .child(icon(i, 15., color))
        };

        let header = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .h(px(24.))
                    .rounded(px(6.))
                    .bg(tint.opacity(0.13))
                    .text_color(tint)
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(icon(Icon::for_kind(item.kind), 13., tint))
                    .child(kind_label(item.kind)),
            )
            .child(div().flex_1())
            .child(
                action(
                    "pv-pin",
                    Icon::Pin,
                    if item.entry.pinned {
                        t.accent
                    } else {
                        t.text_muted
                    },
                    item.entry.pinned,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.toggle_pin();
                    cx.notify();
                })),
            )
            .child(
                action("pv-copy", Icon::Copy, t.text_muted, false).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.copy(this.selected, false, cx);
                        cx.notify();
                    },
                )),
            )
            .child(
                action("pv-del", Icon::Trash, t.danger, false).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.delete_selected();
                        cx.notify();
                    },
                )),
            );

        let body_frame = div()
            .id(("preview", item.entry.id as usize))
            .flex_1()
            .min_h_0()
            .rounded(px(10.))
            .bg(t.elevated)
            .border_1()
            .border_color(t.border)
            .overflow_hidden();

        let body: AnyElement = match (&item.entry.content, item.kind) {
            // Natural size, shrunk to fit — small images are never upscaled.
            (
                Content::Image {
                    file,
                    width,
                    height,
                },
                _,
            ) => body_frame
                .p(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(store::image_path(file))
                        .w(px(*width as f32))
                        .h(px(*height as f32))
                        .max_w_full()
                        .max_h_full()
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element(),
            (Content::Text { text }, Kind::Color) => {
                let c = item.color.unwrap_or(0);
                body_frame
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(t.border_strong)
                            .bg(color_hsla(c)),
                    )
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_sm()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(text.trim().to_string())
                            .child(
                                div().text_color(t.text_muted).child(
                                    format!("#{:08x}", c).to_uppercase().replace("#", "HEX #"),
                                ),
                            )
                            .child(div().text_color(t.text_muted).child(format!(
                                "rgb({}, {}, {}) · α {:.2}",
                                c >> 24,
                                (c >> 16) & 0xff,
                                (c >> 8) & 0xff,
                                (c & 0xff) as f32 / 255.
                            ))),
                    )
                    .into_any_element()
            }
            (Content::Text { text }, kind) => {
                let mut shown: String = text.chars().take(PREVIEW_LIMIT).collect();
                if item.chars > PREVIEW_LIMIT {
                    shown.push_str("\n…");
                }
                let shown = SharedString::from(shown);
                let mut styled = StyledText::new(shown.clone());
                if let Some(range) = self.selection_range() {
                    styled = styled.with_highlights([(
                        range,
                        HighlightStyle {
                            background_color: Some(t.text_selection),
                            ..Default::default()
                        },
                    )]);
                }
                *self.preview_text.borrow_mut() = Some(PreviewText {
                    entry_id: item.entry.id,
                    text: shown,
                    layout: styled.layout().clone(),
                });
                body_frame
                    .overflow_y_scroll()
                    .track_scroll(&self.preview_scroll)
                    .p(px(12.))
                    .text_size(px(13.))
                    .line_height(relative(1.45))
                    .when(matches!(kind, Kind::Code | Kind::Path), |d| {
                        d.font_family(MONO_FONT).text_size(px(12.))
                    })
                    .when(kind == Kind::Link, |d| d.text_color(t.link))
                    .child(
                        div()
                            .cursor_text()
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_text_mouse_down))
                            .child(styled),
                    )
                    .into_any_element()
            }
        };

        let mut facts: Vec<(&str, String)> = vec![
            ("Скопировано", ago(item.entry.created)),
            ("Использовано", ago(item.entry.last_used)),
        ];
        match &item.entry.content {
            Content::Image { width, height, .. } => {
                facts.push(("Размер", format!("{width} × {height}")));
            }
            Content::Text { text } => {
                facts.push(("Символов", item.chars.to_string()));
                facts.push(("Строк", item.lines.to_string()));
                facts.push(("Слов", text.split_whitespace().count().to_string()));
            }
        }
        facts.push(("Вставок", item.entry.uses.to_string()));

        let meta = div()
            .flex()
            .flex_wrap()
            .gap_y(px(6.))
            .children(facts.into_iter().map(|(k, v)| {
                div()
                    .w(relative(0.5))
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(10.5)).text_color(t.text_faint).child(k))
                    .child(div().text_size(px(12.5)).text_color(t.text_muted).child(v))
            }));

        pane.child(header).child(body).child(meta)
    }

    fn render_footer(&self) -> impl IntoElement {
        let t = &self.theme;
        let hint = |keys: &[&str], label: &str| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .children(keys.iter().map(|k| kbd(t, k.to_string())))
                .child(div().ml(px(2.)).child(label.to_string()))
        };
        div()
            .flex_none()
            .h(px(38.))
            .px(px(14.))
            .flex()
            .items_center()
            .gap(px(14.))
            .border_t_1()
            .border_color(t.border)
            .bg(t.panel)
            .text_size(px(11.5))
            .text_color(t.text_muted)
            .map(|d| {
                if self.selection_range().is_some() {
                    d.child(hint(&["↵"], "Копировать фрагмент"))
                        .child(hint(&["Ctrl", "C"], "Не закрывать"))
                        .child(hint(&["Esc"], "Снять выделение"))
                } else {
                    d.child(hint(&["↵"], "Копировать"))
                        .child(hint(&["⇧", "↵"], "Не закрывать"))
                }
            })
            .child(hint(&["Ctrl", "S"], "Закрепить"))
            .child(hint(&["Del"], "Удалить"))
            .child(hint(&["Tab"], "Фильтр"))
            .child(div().flex_1())
            .when(self.selection_range().is_none(), |d| {
                d.child(hint(&["Alt", "1–9"], "Быстрый выбор"))
            })
    }

    fn render_help(&self) -> impl IntoElement {
        let t = self.theme;
        let groups: [(&str, &[(&str, &str)]); 3] = [
            (
                "Навигация",
                &[
                    ("↑ ↓  ·  Ctrl J/K  ·  Ctrl N/P", "Вверх / вниз"),
                    ("PgUp PgDn", "Прыжок на 8"),
                    ("Home End  ·  Alt ↑/↓", "В начало / конец"),
                    ("Tab  ·  Shift Tab", "Следующий / предыдущий фильтр"),
                    ("Набор текста", "Нечёткий поиск (несколько слов — И)"),
                ],
            ),
            (
                "Действия",
                &[
                    ("Enter  ·  двойной клик", "Скопировать и закрыть"),
                    ("Shift Enter", "Скопировать, окно не закрывать"),
                    ("Alt/Ctrl 1–9", "Мгновенно выбрать строку"),
                    ("Ctrl S", "Закрепить / открепить"),
                    ("Del  ·  Ctrl D", "Удалить запись"),
                    ("Ctrl O", "Открыть ссылку / путь / картинку"),
                    ("Ctrl Shift Del ×2", "Очистить незакреплённое"),
                    ("Мышь в превью", "Выделить фрагмент (2× слово, 3× строка)"),
                    (
                        "Ctrl C  ·  Ctrl A",
                        "Копировать фрагмент / выделить весь текст",
                    ),
                ],
            ),
            (
                "Поиск и окно",
                &[
                    ("Backspace  ·  Ctrl Backspace", "Стереть символ / слово"),
                    ("Ctrl U", "Очистить поиск"),
                    ("Esc", "Сбросить поиск, затем закрыть"),
                    ("F1  ·  Ctrl /", "Эта справка"),
                ],
            ),
        ];
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(t.shadow.opacity(0.45))
            .child(
                div()
                    .w(px(560.))
                    .p(px(20.))
                    .rounded(px(14.))
                    .bg(t.elevated)
                    .border_1()
                    .border_color(t.border_strong)
                    .shadow_2xl()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon(Icon::Keyboard, 18., t.accent))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Горячие клавиши"),
                            )
                            .child(div().flex_1())
                            .child(kbd(&t, "Esc")),
                    )
                    .children(groups.into_iter().map(|(title, rows)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.accent)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title.to_uppercase()),
                            )
                            .children(rows.iter().map(|(keys, what)| {
                                div()
                                    .flex()
                                    .text_size(px(12.5))
                                    .child(
                                        div()
                                            .w(px(220.))
                                            .flex_none()
                                            .font_family(MONO_FONT)
                                            .text_size(px(11.5))
                                            .text_color(t.text)
                                            .child(keys.to_string()),
                                    )
                                    .child(div().text_color(t.text_muted).child(what.to_string()))
                            }))
                    })),
            )
    }

    fn render_toast(&self, toast: &Toast) -> impl IntoElement {
        let t = self.theme;
        let color = if toast.error { t.danger } else { t.accent };
        div()
            .absolute()
            .bottom(px(52.))
            .left_0()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(14.))
                    .h(px(34.))
                    .rounded(px(17.))
                    .bg(t.elevated)
                    .border_1()
                    .border_color(color.opacity(0.5))
                    .shadow_lg()
                    .text_size(px(12.5))
                    .child(icon(toast.icon, 14., color))
                    .child(toast.text.clone()),
            )
    }
}

impl Render for ClipView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let client_side = matches!(window.window_decorations(), Decorations::Client { .. });
        if client_side {
            window.set_client_inset(px(SHADOW));
        }

        let card = div()
            .id("card")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(t.surface)
            .text_color(t.text)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .when(client_side, |d| {
                d.rounded(px(RADIUS))
                    .border_1()
                    .border_color(t.border_strong)
                    .shadow(vec![
                        BoxShadow {
                            color: t.shadow,
                            offset: point(px(0.), px(6.)),
                            blur_radius: px(SHADOW),
                            spread_radius: px(-2.),
                        },
                        BoxShadow {
                            color: t.shadow.opacity(0.3),
                            offset: point(px(0.), px(1.)),
                            blur_radius: px(3.),
                            spread_radius: px(0.),
                        },
                    ])
            })
            .child(self.render_header(cx))
            .child(self.render_filters(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.render_list(cx)),
                    )
                    .child(self.render_preview(cx)),
            )
            .child(self.render_footer())
            .when_some(self.toast.as_ref(), |d, toast| {
                d.child(self.render_toast(toast))
            })
            .when(self.show_help, |d| d.child(self.render_help()));

        div()
            .size_full()
            .when(client_side, |d| d.p(px(SHADOW)))
            .child(card)
    }
}
