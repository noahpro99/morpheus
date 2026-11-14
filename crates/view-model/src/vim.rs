//! Vim's modes for a text box, with no toolkit in it: a small modal editing
//! engine that works on a [`TextBuffer`]. Insert mode leaves typing to the
//! box (the engine only watches for Esc); normal mode has the motions
//! `h l j k w b e W B E 0 ^ $ gg G f F t T ; ,`, the operators `d c y` (with
//! motions, counts, `dd cc yy` and the text objects `iw aw`), `x X D C s S Y
//! r ~ J p P`, `i a I A o O` to enter insert, `u` and Ctrl+R, `.` and visual
//! mode (`v`) with `d y c o`. A count goes before the command or the motion.
//! Lines are the text's lines (`\n`), not wrapped rows.

use std::ops::Range;

use crate::{history::History, text_buffer::TextBuffer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Insert,
    Normal,
    Visual,
}

/// A key as the engine takes it: the character typed (shift already in it),
/// Ctrl with a letter, or one of the few named keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Esc,
    Enter,
    Backspace,
}

/// What a key did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The engine used the key; when false (insert mode, Ctrl combos) the box
    /// deals with it.
    pub handled: bool,
    /// The text changed.
    pub changed: bool,
    /// Enter in normal mode: send.
    pub submit: bool,
    /// Esc with nothing to leave: for the owner (close a reply, leave the box).
    pub escape: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Delete,
    Change,
    Yank,
}

impl Op {
    fn letter(self) -> char {
        match self {
            Op::Delete => 'd',
            Op::Change => 'c',
            Op::Yank => 'y',
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
enum Pending {
    #[default]
    None,
    G,
    Find(char),
    Replace,
    Object(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Up to, not including, the far end.
    Excl,
    /// Including the character at the far end.
    Incl,
    /// Whole lines, from the one of `from` to the one of `to`.
    Lines,
}

#[derive(Clone, Copy, Debug)]
struct Mv {
    from: usize,
    to: usize,
    kind: Kind,
}

#[derive(Clone, Debug, Default)]
struct Register {
    text: String,
    linewise: bool,
}

#[derive(Debug, Default)]
pub struct Vim {
    mode: Mode,
    count: Option<usize>,
    op: Option<Op>,
    op_count: Option<usize>,
    pending: Pending,
    register: Register,
    /// The register was written since `take_yank`.
    yanked: bool,
    /// What the clipboard held when `p` was pressed.
    clip: Option<String>,
    /// The column kept while moving by lines.
    goal_col: Option<usize>,
    /// Where visual mode started.
    anchor: usize,
    last_find: Option<(char, char)>,
    /// The keys of the command being typed, and of the last one that changed
    /// the text (what `.` does again).
    cmd: Vec<Key>,
    last: Vec<Key>,
    changed_cmd: bool,
    cmd_visual: bool,
    /// Insert mode was entered by a command: what is typed belongs to it.
    recording: bool,
    replaying: bool,
    pub history: History,
}


fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// 0 for blanks (and line ends), 1 for a word (or any non-blank, with `big`), 2 for symbols.
fn cls(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        0
    } else if big || is_word(c) {
        1
    } else {
        2
    }
}

fn ls(t: &str, i: usize) -> usize {
    t[..i].rfind('\n').map_or(0, |p| p + 1)
}

fn le(t: &str, i: usize) -> usize {
    t[i..].find('\n').map_or(t.len(), |p| i + p)
}

fn next(t: &str, i: usize) -> usize {
    t[i..].chars().next().map_or(i, |c| i + c.len_utf8())
}

fn prev(t: &str, i: usize) -> usize {
    t[..i].char_indices().next_back().map_or(0, |(p, _)| p)
}

fn at(t: &str, i: usize) -> Option<char> {
    t[i..].chars().next()
}

fn before(t: &str, i: usize) -> Option<char> {
    t[..i].chars().next_back()
}

/// The last character of the line `i` is on (its start when empty).
fn last_in_line(t: &str, i: usize) -> usize {
    let (s, e) = (ls(t, i), le(t, i));
    if e > s { prev(t, e) } else { s }
}

/// The first character of the line that is not a blank.
fn first_nb(t: &str, i: usize) -> usize {
    let (s, e) = (ls(t, i), le(t, i));
    let mut p = s;
    for c in t[s..e].chars() {
        if c == ' ' || c == '\t' {
            p += c.len_utf8();
        } else {
            break;
        }
    }
    if p >= e && e > s { prev(t, e) } else { p }
}

fn line_of(t: &str, i: usize) -> usize {
    t[..i].matches('\n').count()
}

fn lines_in(t: &str) -> usize {
    t.matches('\n').count() + 1
}

fn start_of_line(t: &str, line: usize) -> usize {
    let mut at = 0;
    for _ in 0..line {
        match t[at..].find('\n') {
            Some(p) => at += p + 1,
            None => break,
        }
    }
    at
}

fn col_of(t: &str, i: usize) -> usize {
    t[ls(t, i)..i].chars().count()
}

/// `col` characters into the line starting at `start`, on a character.
fn at_col(t: &str, start: usize, col: usize) -> usize {
    let last = last_in_line(t, start);
    let mut p = start;
    for _ in 0..col {
        if p >= last {
            break;
        }
        p = next(t, p);
    }
    p
}

/// The start of the next word (or the empty line, which counts as one).
fn next_word_start(t: &str, mut i: usize, big: bool) -> usize {
    if let Some(c) = at(t, i) {
        let k = cls(c, big);
        if k != 0 {
            while let Some(c) = at(t, i) {
                if cls(c, big) != k {
                    break;
                }
                i += c.len_utf8();
            }
        }
    }
    loop {
        match at(t, i) {
            Some('\n') => {
                i += 1;
                if at(t, i) == Some('\n') {
                    return i;
                }
            }
            Some(c) if c.is_whitespace() => i += c.len_utf8(),
            _ => return i,
        }
    }
}

/// The last character of the word run `i` is in.
fn run_end(t: &str, mut i: usize, big: bool) -> usize {
    let Some(c) = at(t, i) else { return i };
    let k = cls(c, big);
    loop {
        let n = next(t, i);
        match at(t, n) {
            Some(c) if cls(c, big) == k && n > i => i = n,
            _ => return i,
        }
    }
}

/// `e`: the end of the word after the character at `i`.
fn word_end(t: &str, i: usize, big: bool) -> usize {
    let mut p = next(t, i);
    while let Some(c) = at(t, p) {
        if cls(c, big) != 0 {
            break;
        }
        p += c.len_utf8();
    }
    if p >= t.len() {
        return prev(t, t.len());
    }
    run_end(t, p, big)
}

/// `b`: the start of the word before `i`.
fn prev_word_start(t: &str, i: usize, big: bool) -> usize {
    if i == 0 {
        return 0;
    }
    let mut p = prev(t, i);
    while let Some(c) = at(t, p) {
        if cls(c, big) != 0 {
            break;
        }
        if c == '\n' && (p == 0 || before(t, p) == Some('\n')) {
            return p;
        }
        if p == 0 {
            return 0;
        }
        p = prev(t, p);
    }
    let k = at(t, p).map_or(0, |c| cls(c, big));
    while p > 0 {
        let q = prev(t, p);
        if at(t, q).is_some_and(|c| cls(c, big) == k) {
            p = q;
        } else {
            break;
        }
    }
    p
}

/// `iw` and `aw`: the run of the same sort of character around `i` (and the
/// blanks after a word, or before it when none follow).
fn word_object(t: &str, i: usize, inner: bool, big: bool) -> Range<usize> {
    if t.is_empty() {
        return 0..0;
    }
    let i = if i >= t.len() { prev(t, t.len()) } else { i };
    let kind = |c: char| if c == '\n' { 3 } else { cls(c, big) };
    let Some(first) = at(t, i) else { return i..i };
    let k = kind(first);
    let mut start = i;
    while let Some(c) = before(t, start) {
        if kind(c) != k {
            break;
        }
        start -= c.len_utf8();
    }
    let mut end = i + first.len_utf8();
    while let Some(c) = at(t, end) {
        if kind(c) != k {
            break;
        }
        end += c.len_utf8();
    }
    if inner {
        return start..end;
    }
    let blank = |c: char| c.is_whitespace() && c != '\n';
    if k == 0 {
        // Blanks: with the word after them.
        if let Some(c) = at(t, end)
            && kind(c) != 3
        {
            let k2 = kind(c);
            while let Some(c) = at(t, end) {
                if kind(c) != k2 {
                    break;
                }
                end += c.len_utf8();
            }
        }
        return start..end;
    }
    let mut after = end;
    while let Some(c) = at(t, after) {
        if !blank(c) {
            break;
        }
        after += c.len_utf8();
    }
    if after > end {
        return start..after;
    }
    while let Some(c) = before(t, start) {
        if !blank(c) {
            break;
        }
        start -= c.len_utf8();
    }
    start..end
}

impl Vim {
    pub fn new() -> Self {
        Self::default()
    }

    /// An engine in `mode` (for tests).
    pub fn in_mode(mode: Mode) -> Self {
        Self { mode, ..Self::default() }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Back to insert mode with nothing pending (the box gained focus).
    pub fn enter_insert(&mut self) {
        self.mode = Mode::Insert;
        self.cancel();
        self.recording = false;
        self.cmd.clear();
        self.changed_cmd = false;
        self.cmd_visual = false;
        self.history.break_run();
    }

    /// Typed text in insert mode, noted for `.`.
    pub fn typed(&mut self, text: &str) {
        if self.mode == Mode::Insert && self.recording && !self.replaying {
            self.cmd.extend(text.chars().map(Key::Char));
        }
    }

    /// Backspace in insert mode, noted for `.`.
    pub fn backspaced(&mut self) {
        if self.mode == Mode::Insert && self.recording && !self.replaying && matches!(self.cmd.last(), Some(Key::Char(_))) {
            self.cmd.pop();
        }
    }

    /// What the clipboard holds, for `p` (the register is used when it is the same).
    pub fn set_clipboard(&mut self, text: Option<&str>) {
        self.clip = text.map(str::to_owned);
    }

    /// Text yanked or deleted since the last call: for the clipboard.
    pub fn take_yank(&mut self) -> Option<String> {
        std::mem::take(&mut self.yanked).then(|| self.register.text.clone())
    }

    /// The characters visual mode has selected.
    pub fn visual_range(&self, buf: &TextBuffer) -> Option<Range<usize>> {
        if self.mode != Mode::Visual {
            return None;
        }
        let t = buf.text();
        let (a, b) = (self.anchor.min(buf.head()).min(t.len()), self.anchor.max(buf.head()).min(t.len()));
        let end = match at(t, b) {
            Some(c) if c != '\n' => b + c.len_utf8(),
            _ => b,
        };
        Some(a..end)
    }

    /// What is typed so far of a command (`2d`), for the indicator.
    pub fn pending(&self) -> String {
        let mut s = String::new();
        if let Some(n) = self.op_count {
            s.push_str(&n.to_string());
        }
        if let Some(op) = self.op {
            s.push(op.letter());
        }
        if let Some(n) = self.count {
            s.push_str(&n.to_string());
        }
        match self.pending {
            Pending::None => {}
            Pending::G => s.push('g'),
            Pending::Find(k) => s.push(k),
            Pending::Replace => s.push('r'),
            Pending::Object(inner) => s.push(if inner { 'i' } else { 'a' }),
        }
        s
    }

    /// Puts the caret where normal mode can have it: on a character.
    pub fn settle(&mut self, buf: &mut TextBuffer) {
        if self.mode == Mode::Insert {
            return;
        }
        let t = buf.text();
        let h = buf.head().min(t.len());
        if h >= le(t, h) && h > ls(t, h) {
            let p = prev(t, h);
            buf.move_to(p, false);
        }
    }

    pub fn key(&mut self, buf: &mut TextBuffer, key: Key) -> Outcome {
        let was = buf.text().to_owned();
        let mut out = match self.mode {
            Mode::Insert => self.insert_key(buf, key),
            Mode::Normal | Mode::Visual => self.command_key(buf, key),
        };
        if self.mode != Mode::Insert {
            self.settle(buf);
        }
        out.changed = buf.text() != was;
        out
    }

    fn insert_key(&mut self, buf: &mut TextBuffer, key: Key) -> Outcome {
        match key {
            Key::Esc => {
                if self.recording && !self.replaying {
                    self.cmd.push(Key::Esc);
                }
                self.mode = Mode::Normal;
                self.recording = false;
                let t = buf.text();
                let h = buf.head().min(t.len());
                if h > ls(t, h) {
                    let p = prev(t, h);
                    buf.move_to(p, false);
                }
                self.history.break_run();
                self.finish_command();
                Outcome { handled: true, ..Outcome::default() }
            }
            Key::Char(c) if self.replaying => {
                buf.insert(&c.to_string());
                Outcome { handled: true, ..Outcome::default() }
            }
            Key::Backspace if self.replaying => {
                buf.backspace(false);
                Outcome { handled: true, ..Outcome::default() }
            }
            _ => Outcome::default(),
        }
    }

    fn finish_command(&mut self) {
        if !self.replaying && self.changed_cmd && !self.cmd_visual && !self.cmd.is_empty() {
            self.last = std::mem::take(&mut self.cmd);
        }
        self.cmd.clear();
        self.changed_cmd = false;
        self.cmd_visual = false;
    }

    fn idle(&self) -> bool {
        self.pending == Pending::None && self.count.is_none() && self.op.is_none() && self.op_count.is_none()
    }

    fn cancel(&mut self) {
        self.count = None;
        self.op = None;
        self.op_count = None;
        self.pending = Pending::None;
    }

    /// The count typed (before an operator times the one after it), if any.
    fn take_count(&mut self) -> Option<usize> {
        match (self.op_count.take(), self.count.take()) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(1) * b.unwrap_or(1)),
        }
    }

    fn begin_change(&mut self, buf: &TextBuffer) {
        self.history.record(buf);
        self.changed_cmd = true;
    }

    fn set_register(&mut self, text: String, linewise: bool) {
        self.register = Register { text, linewise };
        self.yanked = true;
    }

    fn enter_insert_cmd(&mut self) {
        self.mode = Mode::Insert;
        self.recording = true;
        self.history.typing = true;
    }

    fn command_key(&mut self, buf: &mut TextBuffer, key: Key) -> Outcome {
        // Ctrl combos other than redo are the box's (copy, paste, undo, ...).
        if matches!(key, Key::Ctrl(c) if c != 'r') {
            return Outcome::default();
        }
        if !self.replaying {
            self.cmd.push(key);
        }
        if self.mode == Mode::Visual {
            self.cmd_visual = true;
        }
        let mut out = Outcome { handled: true, ..Outcome::default() };
        self.command(buf, key, &mut out);
        if self.mode == Mode::Normal && self.idle() {
            self.finish_command();
        }
        out
    }

    fn command(&mut self, buf: &mut TextBuffer, key: Key, out: &mut Outcome) {
        if key == Key::Esc {
            if self.mode == Mode::Visual {
                self.mode = Mode::Normal;
                self.cancel();
            } else if !self.idle() {
                self.cancel();
            } else {
                out.escape = true;
            }
            return;
        }
        match (self.pending, key) {
            (Pending::None, _) => self.top(buf, key, out),
            (Pending::Find(kind), Key::Char(c)) => {
                self.pending = Pending::None;
                self.last_find = Some((kind, c));
                let n = self.take_count();
                let t = buf.text().to_owned();
                match find(&t, buf.head().min(t.len()), kind, c, n.unwrap_or(1)) {
                    Some(mv) => self.apply(buf, mv),
                    None => self.cancel(),
                }
            }
            (Pending::G, Key::Char('g')) => {
                self.pending = Pending::None;
                let n = self.take_count();
                self.go_to_line(buf, n.map(|n| n.max(1) - 1).or(Some(0)));
            }
            (Pending::Replace, Key::Char(c)) => {
                self.pending = Pending::None;
                let n = self.take_count().unwrap_or(1);
                self.replace(buf, c, n);
            }
            (Pending::Object(inner), Key::Char(c @ ('w' | 'W'))) => {
                self.pending = Pending::None;
                let op = self.op.take();
                self.cancel();
                if let Some(op) = op {
                    let t = buf.text().to_owned();
                    let range = word_object(&t, buf.head().min(t.len()), inner, c == 'W');
                    self.op_chars(buf, op, range);
                }
            }
            _ => self.cancel(),
        }
    }

    fn go_to_line(&mut self, buf: &mut TextBuffer, line: Option<usize>) {
        let t = buf.text().to_owned();
        let h = buf.head().min(t.len());
        let total = lines_in(&t);
        let line = line.map_or(total - 1, |l| l.min(total - 1));
        let to = first_nb(&t, start_of_line(&t, line));
        self.apply(buf, Mv { from: h, to, kind: Kind::Lines });
    }

    fn run_motion(&mut self, buf: &mut TextBuffer, c: char) {
        let n = self.take_count();
        let t = buf.text().to_owned();
        match self.motion(&t, buf.head().min(t.len()), c, n) {
            Some(mv) => self.apply(buf, mv),
            None => self.cancel(),
        }
    }

    /// Does what a motion asks: moves, or (after an operator) operates.
    fn apply(&mut self, buf: &mut TextBuffer, mv: Mv) {
        let Some(op) = self.op.take() else {
            buf.move_to(mv.to, false);
            return;
        };
        self.cancel();
        let t = buf.text().to_owned();
        let (lo, hi) = (mv.from.min(mv.to), mv.from.max(mv.to));
        match mv.kind {
            Kind::Lines => self.op_lines(buf, op, ls(&t, lo), le(&t, hi), mv.to < mv.from),
            Kind::Excl | Kind::Incl => {
                let mut end = hi;
                if mv.kind == Kind::Incl && at(&t, hi).is_some_and(|c| c != '\n') {
                    end = next(&t, hi);
                }
                self.op_chars(buf, op, lo..end);
            }
        }
    }

    fn op_chars(&mut self, buf: &mut TextBuffer, op: Op, range: Range<usize>) {
        let text = buf.text()[range.clone()].to_owned();
        if text.is_empty() {
            if op == Op::Change {
                self.begin_change(buf);
                buf.move_to(range.start, false);
                self.enter_insert_cmd();
            }
            return;
        }
        match op {
            Op::Yank => {
                self.set_register(text, false);
                buf.move_to(range.start, false);
            }
            Op::Delete => {
                self.begin_change(buf);
                self.set_register(text, false);
                buf.replace_range(range, "");
            }
            Op::Change => {
                self.begin_change(buf);
                self.set_register(text, false);
                buf.replace_range(range, "");
                self.enter_insert_cmd();
            }
        }
    }

    /// Whole lines, from the start of the first to the end of the last.
    fn op_lines(&mut self, buf: &mut TextBuffer, op: Op, s: usize, e: usize, upwards: bool) {
        let t = buf.text().to_owned();
        let block = format!("{}\n", &t[s..e]);
        match op {
            Op::Yank => {
                self.set_register(block, true);
                if upwards {
                    buf.move_to(s, false);
                }
            }
            Op::Change => {
                self.begin_change(buf);
                self.set_register(block, true);
                buf.replace_range(s..e, "");
                self.enter_insert_cmd();
            }
            Op::Delete => {
                self.begin_change(buf);
                self.set_register(block, true);
                if e < t.len() {
                    buf.replace_range(s..e + 1, "");
                } else if s > 0 {
                    buf.replace_range(s - 1..e, "");
                } else {
                    buf.replace_range(s..e, "");
                }
                let now = buf.text().to_owned();
                let at = s.min(now.len());
                let line = if e >= t.len() && s > 0 { ls(&now, at.saturating_sub(1)) } else { at };
                buf.move_to(first_nb(&now, line), false);
            }
        }
    }

    fn visual_op(&mut self, buf: &mut TextBuffer, op: Op) {
        let Some(range) = self.visual_range(buf) else { return };
        self.mode = Mode::Normal;
        self.cancel();
        self.op_chars(buf, op, range);
    }

    fn replace(&mut self, buf: &mut TextBuffer, c: char, n: usize) {
        let t = buf.text().to_owned();
        let h = buf.head().min(t.len());
        let e = le(&t, h);
        let have = t[h..e].chars().count();
        if n == 0 || have < n {
            return;
        }
        let end = t[h..e].char_indices().nth(n).map_or(e, |(i, _)| h + i);
        self.begin_change(buf);
        buf.replace_range(h..end, &c.to_string().repeat(n));
        buf.move_to(h + c.len_utf8() * (n - 1), false);
    }

    fn put(&mut self, buf: &mut TextBuffer, before_cursor: bool, n: usize) {
        if let Some(clip) = self.clip.take()
            && !clip.is_empty()
            && clip != self.register.text
        {
            self.register = Register { text: clip, linewise: false };
        }
        let reg = self.register.clone();
        if reg.text.is_empty() {
            return;
        }
        self.begin_change(buf);
        let t = buf.text().to_owned();
        let h = buf.head().min(t.len());
        if reg.linewise {
            let body = reg.text.trim_end_matches('\n');
            let block = vec![body; n].join("\n");
            let at_line = if before_cursor {
                let s = ls(&t, h);
                buf.replace_range(s..s, &format!("{block}\n"));
                s
            } else if le(&t, h) < t.len() {
                let s = le(&t, h) + 1;
                buf.replace_range(s..s, &format!("{block}\n"));
                s
            } else {
                let e = t.len();
                buf.replace_range(e..e, &format!("\n{block}"));
                e + 1
            };
            let p = first_nb(buf.text(), at_line);
            buf.move_to(p, false);
        } else {
            let text = reg.text.repeat(n);
            let pos = if before_cursor || at(&t, h).is_none_or(|c| c == '\n') { h } else { next(&t, h) };
            buf.replace_range(pos..pos, &text);
            let cursor = if text.contains('\n') { pos } else { prev(buf.text(), pos + text.len()) };
            buf.move_to(cursor, false);
        }
    }

    fn join(&mut self, buf: &mut TextBuffer, n: usize) {
        let t = buf.text();
        if le(t, buf.head().min(t.len())) >= t.len() {
            return;
        }
        self.begin_change(buf);
        for _ in 0..n.max(2) - 1 {
            let t = buf.text().to_owned();
            let h = buf.head().min(t.len());
            let e = le(&t, h);
            if e >= t.len() {
                break;
            }
            let rest = &t[e + 1..];
            let trimmed = rest.trim_start_matches([' ', '\t']);
            let skipped = rest.len() - trimmed.len();
            let glue = if trimmed.starts_with('\n') || trimmed.is_empty() || before(&t, e).is_none_or(|c| c.is_whitespace()) {
                ""
            } else {
                " "
            };
            buf.replace_range(e..e + 1 + skipped, glue);
        }
    }

    fn toggle_case(&mut self, buf: &mut TextBuffer, n: usize) {
        let t = buf.text().to_owned();
        let h = buf.head().min(t.len());
        let e = le(&t, h);
        if h >= e {
            return;
        }
        let mut end = h;
        for _ in 0..n {
            if end >= e {
                break;
            }
            end = next(&t, end);
        }
        let flipped: String = t[h..end]
            .chars()
            .map(|c| {
                if c.is_uppercase() {
                    c.to_lowercase().to_string()
                } else {
                    c.to_uppercase().to_string()
                }
            })
            .collect();
        self.begin_change(buf);
        buf.replace_range(h..end, &flipped);
        buf.move_to(h + flipped.len(), false);
    }

    fn replay(&mut self, buf: &mut TextBuffer) {
        if self.last.is_empty() {
            return;
        }
        let keys = self.last.clone();
        self.replaying = true;
        for key in keys {
            match self.mode {
                Mode::Insert => {
                    self.insert_key(buf, key);
                }
                _ => {
                    self.command_key(buf, key);
                }
            }
        }
        self.replaying = false;
        if self.mode == Mode::Insert {
            self.mode = Mode::Normal;
            self.recording = false;
        }
        self.cmd.clear();
        self.changed_cmd = false;
    }

    /// Where a motion goes from `h`; `None` when it cannot.
    fn motion(&mut self, t: &str, h: usize, c: char, count: Option<usize>) -> Option<Mv> {
        let n = count.unwrap_or(1).max(1);
        let op = self.op;
        let mv = |to: usize, kind: Kind| Some(Mv { from: h, to, kind });
        match c {
            'h' | '\u{8}' => {
                let s = ls(t, h);
                let mut p = h;
                for _ in 0..n {
                    if p <= s {
                        break;
                    }
                    p = prev(t, p);
                }
                mv(p, Kind::Excl)
            }
            'l' => {
                let limit = if op.is_some() { le(t, h) } else { last_in_line(t, h) };
                let mut p = h;
                for _ in 0..n {
                    if p >= limit {
                        break;
                    }
                    p = next(t, p);
                }
                mv(p, Kind::Excl)
            }
            'j' | 'k' => {
                let cur = line_of(t, h);
                let total = lines_in(t);
                let target = if c == 'j' { (cur + n).min(total - 1) } else { cur.saturating_sub(n) };
                if target == cur {
                    return None;
                }
                let goal = *self.goal_col.get_or_insert(col_of(t, h));
                mv(at_col(t, start_of_line(t, target), goal), Kind::Lines)
            }
            'w' | 'W' => {
                let big = c == 'W';
                if op == Some(Op::Change) && at(t, h).is_some_and(|ch| cls(ch, big) != 0) {
                    let mut p = run_end(t, h, big);
                    for _ in 1..n {
                        p = word_end(t, p, big);
                    }
                    return mv(p, Kind::Incl);
                }
                let mut p = h;
                for k in 0..n {
                    let q = next_word_start(t, p, big);
                    p = if op.is_some() && k + 1 == n && t[p..q].contains('\n') { le(t, p) } else { q };
                    if q >= t.len() {
                        break;
                    }
                }
                mv(p, Kind::Excl)
            }
            'e' | 'E' => {
                let mut p = h;
                for _ in 0..n {
                    p = word_end(t, p, c == 'E');
                }
                mv(p, Kind::Incl)
            }
            'b' | 'B' => {
                let mut p = h;
                for _ in 0..n {
                    p = prev_word_start(t, p, c == 'B');
                }
                mv(p, Kind::Excl)
            }
            '0' => mv(ls(t, h), Kind::Excl),
            '^' => mv(first_nb(t, h), Kind::Excl),
            '$' => {
                let target = (line_of(t, h) + n - 1).min(lines_in(t) - 1);
                mv(last_in_line(t, start_of_line(t, target)), Kind::Incl)
            }
            'G' => {
                let total = lines_in(t);
                let line = count.map_or(total - 1, |c| c.max(1).min(total) - 1);
                mv(first_nb(t, start_of_line(t, line)), Kind::Lines)
            }
            ';' | ',' => {
                let (kind, ch) = self.last_find?;
                let kind = if c == ',' {
                    match kind {
                        'f' => 'F',
                        'F' => 'f',
                        't' => 'T',
                        _ => 't',
                    }
                } else {
                    kind
                };
                find(t, h, kind, ch, n)
            }
            _ => None,
        }
    }

    fn top(&mut self, buf: &mut TextBuffer, key: Key, out: &mut Outcome) {
        let c = match key {
            Key::Ctrl(_) => {
                // Redo.
                self.cancel();
                self.history.redo(buf);
                return;
            }
            Key::Enter => {
                if self.idle() && self.mode == Mode::Normal {
                    out.submit = true;
                } else {
                    self.cancel();
                }
                return;
            }
            Key::Backspace => 'h',
            Key::Char(c) => c,
            Key::Esc => return,
        };
        if c.is_ascii_digit() && (c != '0' || self.count.is_some()) {
            let d = c as usize - '0' as usize;
            self.count = Some((self.count.unwrap_or(0) * 10 + d).min(99_999));
            return;
        }
        if !matches!(c, 'j' | 'k') {
            self.goal_col = None;
        }
        let visual = self.mode == Mode::Visual;
        match c {
            'h' | '\u{8}' | 'l' | 'j' | 'k' | 'w' | 'W' | 'e' | 'E' | 'b' | 'B' | '0' | '^' | '$' | 'G' | ';' | ',' => {
                self.run_motion(buf, c)
            }
            'g' => self.pending = Pending::G,
            'f' | 'F' | 't' | 'T' => self.pending = Pending::Find(c),
            'd' | 'c' | 'y' => {
                let this = match c {
                    'd' => Op::Delete,
                    'c' => Op::Change,
                    _ => Op::Yank,
                };
                if visual {
                    self.visual_op(buf, this);
                } else if self.op == Some(this) {
                    let n = self.take_count().unwrap_or(1);
                    let t = buf.text().to_owned();
                    let h = buf.head().min(t.len());
                    let target = (line_of(&t, h) + n - 1).min(lines_in(&t) - 1);
                    self.apply(buf, Mv { from: h, to: start_of_line(&t, target), kind: Kind::Lines });
                } else if self.op.is_none() {
                    self.op = Some(this);
                    self.op_count = self.count.take();
                } else {
                    self.cancel();
                }
            }
            'x' | 'X' | 'D' | 's' | 'C' if visual => {
                self.visual_op(buf, if matches!(c, 's' | 'C') { Op::Change } else { Op::Delete });
            }
            'o' if visual => {
                let h = buf.head();
                buf.move_to(self.anchor, false);
                self.anchor = h;
            }
            'v' => {
                if visual {
                    self.mode = Mode::Normal;
                } else {
                    self.mode = Mode::Visual;
                    self.anchor = buf.head();
                }
                self.cancel();
            }
            _ if visual => self.cancel(),
            'x' => {
                self.op = Some(Op::Delete);
                self.run_motion(buf, 'l');
            }
            'X' => {
                self.op = Some(Op::Delete);
                self.run_motion(buf, 'h');
            }
            'D' => {
                self.op = Some(Op::Delete);
                self.run_motion(buf, '$');
            }
            'C' => {
                self.op = Some(Op::Change);
                self.run_motion(buf, '$');
            }
            's' => {
                self.op = Some(Op::Change);
                self.run_motion(buf, 'l');
            }
            'S' | 'Y' => {
                self.op = Some(if c == 'S' { Op::Change } else { Op::Yank });
                let n = self.take_count().unwrap_or(1);
                let t = buf.text().to_owned();
                let h = buf.head().min(t.len());
                let target = (line_of(&t, h) + n - 1).min(lines_in(&t) - 1);
                self.apply(buf, Mv { from: h, to: start_of_line(&t, target), kind: Kind::Lines });
            }
            'i' | 'a' if self.op.is_some() => self.pending = Pending::Object(c == 'i'),
            'i' => {
                self.cancel();
                self.begin_change(buf);
                self.enter_insert_cmd();
            }
            'a' => {
                self.cancel();
                self.begin_change(buf);
                let t = buf.text();
                let h = buf.head().min(t.len());
                if at(t, h).is_some_and(|ch| ch != '\n') {
                    let p = next(t, h);
                    buf.move_to(p, false);
                }
                self.enter_insert_cmd();
            }
            'I' => {
                self.cancel();
                self.begin_change(buf);
                let t = buf.text();
                let h = buf.head().min(t.len());
                let s = ls(t, h);
                let p = if first_nb(t, h) == last_in_line(t, h) && le(t, h) > s && t[s..le(t, h)].trim().is_empty() {
                    le(t, h)
                } else {
                    first_nb(t, h)
                };
                buf.move_to(p, false);
                self.enter_insert_cmd();
            }
            'A' => {
                self.cancel();
                self.begin_change(buf);
                let t = buf.text();
                let p = le(t, buf.head().min(t.len()));
                buf.move_to(p, false);
                self.enter_insert_cmd();
            }
            'o' | 'O' => {
                self.cancel();
                self.begin_change(buf);
                let t = buf.text().to_owned();
                let h = buf.head().min(t.len());
                if c == 'o' {
                    let e = le(&t, h);
                    buf.replace_range(e..e, "\n");
                    buf.move_to(e + 1, false);
                } else {
                    let s = ls(&t, h);
                    buf.replace_range(s..s, "\n");
                    buf.move_to(s, false);
                }
                self.enter_insert_cmd();
            }
            'r' => self.pending = Pending::Replace,
            '~' => {
                let n = self.take_count().unwrap_or(1);
                self.toggle_case(buf, n);
            }
            'J' => {
                let n = self.take_count().unwrap_or(2);
                self.join(buf, n);
            }
            'p' | 'P' => {
                let n = self.take_count().unwrap_or(1);
                self.put(buf, c == 'P', n);
            }
            'u' => {
                self.cancel();
                self.history.undo(buf);
            }
            '.' => {
                self.cancel();
                self.replay(buf);
            }
            _ => self.cancel(),
        }
    }
}

/// `f`, `F`, `t` and `T`: the `n`th `ch` along the line from `h`.
fn find(t: &str, h: usize, kind: char, ch: char, n: usize) -> Option<Mv> {
    let (s, e) = (ls(t, h), le(t, h));
    let mut p = h;
    match kind {
        'f' | 't' => {
            for _ in 0..n {
                let from = next(t, p);
                if from >= e {
                    return None;
                }
                p = from + t[from..e].find(ch)?;
            }
            let to = if kind == 'f' { p } else { prev(t, p) };
            Some(Mv { from: h, to, kind: Kind::Incl })
        }
        _ => {
            for _ in 0..n {
                if p <= s {
                    return None;
                }
                p = s + t[s..p].rfind(ch)?;
            }
            let to = if kind == 'F' { p } else { next(t, p) };
            Some(Mv { from: h, to, kind: Kind::Excl })
        }
    }
}

const FILE: &str = "vim-mode.json";

/// Whether vim mode is on, as kept in `dir` (off when missing or unreadable).
pub fn load_enabled(dir: &std::path::Path) -> bool {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("enabled").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

pub fn save_enabled(dir: &std::path::Path, on: bool) -> std::io::Result<()> {
    std::fs::write(dir.join(FILE), serde_json::json!({ "enabled": on }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box and the engine, the caret marked `|` in the text it is given.
    struct Ed {
        buf: TextBuffer,
        vim: Vim,
        last: Outcome,
    }

    fn ed(spec: &str) -> Ed {
        let at = spec.find('|').expect("a caret");
        let text = spec.replacen('|', "", 1);
        let mut buf = TextBuffer::new(&text);
        buf.move_to(at, false);
        Ed { buf, vim: Vim::in_mode(Mode::Normal), last: Outcome::default() }
    }

    impl Ed {
        /// Keys as typed; `<Esc>`, `<CR>`, `<BS>` and `<C-r>` are the named ones.
        fn keys(mut self, keys: &str) -> Self {
            let mut rest = keys;
            while let Some(c) = rest.chars().next() {
                let key = if c == '<'
                    && let Some(end) = rest.find('>')
                    && end > 1
                {
                    let name = &rest[1..end];
                    rest = &rest[end + 1..];
                    match name {
                        "Esc" => Key::Esc,
                        "CR" => Key::Enter,
                        "BS" => Key::Backspace,
                        ctrl if ctrl.starts_with("C-") => Key::Ctrl(ctrl.chars().nth(2).expect("a letter")),
                        other => panic!("unknown key {other}"),
                    }
                } else {
                    rest = &rest[c.len_utf8()..];
                    Key::Char(c)
                };
                self.press(key);
            }
            self
        }

        /// One key, typing what insert mode leaves to the box.
        fn press(&mut self, key: Key) {
            self.last = self.vim.key(&mut self.buf, key);
            if !self.last.handled {
                match key {
                    Key::Char(c) => {
                        self.vim.history.record_typing(&self.buf);
                        self.buf.insert(&c.to_string());
                        self.vim.typed(&c.to_string());
                    }
                    Key::Backspace => {
                        self.buf.backspace(false);
                    }
                    _ => {}
                }
            }
        }

        fn show(&self) -> String {
            let mut text = self.buf.text().to_owned();
            text.insert(self.buf.head(), '|');
            text
        }
    }

    fn is(spec: &str, keys: &str, want: &str) {
        let got = ed(spec).keys(keys).show();
        assert_eq!(got, want, "{spec:?} then {keys:?}");
    }

    #[test]
    fn h_and_l_stop_at_the_ends_of_the_line() {
        is("|abc", "l", "a|bc");
        is("|abc", "lll", "ab|c");
        is("a|bc", "h", "|abc");
        is("|abc", "hh", "|abc");
        is("ab|c\nd", "l", "ab|c\nd");
    }

    #[test]
    fn words_move_by_w_b_and_e() {
        is("|foo bar.baz", "w", "foo |bar.baz");
        is("|foo bar.baz", "ww", "foo bar|.baz");
        is("|foo bar.baz", "www", "foo bar.|baz");
        is("foo bar.|baz", "b", "foo bar|.baz");
        is("foo bar.|baz", "bb", "foo |bar.baz");
        is("|foo bar", "e", "fo|o bar");
        is("|foo bar", "ee", "foo ba|r");
        is("|foo bar.baz", "W", "foo |bar.baz");
        is("|foo.bar baz", "W", "foo.bar |baz");
        is("|foo\nbar", "w", "foo\n|bar");
        is("a|\n\nb", "w", "a\n|\nb");
    }

    #[test]
    fn line_ends_and_first_words() {
        is("  fo|o bar", "0", "|  foo bar");
        is("  fo|o bar", "^", "  |foo bar");
        is("  fo|o bar", "$", "  foo ba|r");
        is("|ab\ncd", "$", "a|b\ncd");
    }

    #[test]
    fn j_and_k_keep_the_column() {
        is("ab|cd\nxy\nabcd", "j", "abcd\nx|y\nabcd");
        is("ab|cd\nxy\nabcd", "jj", "abcd\nxy\nab|cd");
        is("abcd\nxy\nab|cd", "kk", "ab|cd\nxy\nabcd");
        is("a|b", "j", "a|b");
        is("a|b", "k", "a|b");
        is("a\nb\nc|", "2k", "|a\nb\nc");
    }

    #[test]
    fn gg_and_g_go_to_the_ends_or_a_line() {
        is("a\nb\n|c", "gg", "|a\nb\nc");
        is("  a\nb\n|c", "gg", "  |a\nb\nc");
        is("|a\nb\nc", "G", "a\nb\n|c");
        is("|a\nb\nc", "2G", "a\n|b\nc");
        is("a\nb\nc|", "2gg", "a\n|b\nc");
    }

    #[test]
    fn counts_repeat_motions() {
        is("|a b c d", "3w", "a b c |d");
        is("|abcdefghijkl", "10l", "abcdefghij|kl");
        is("|abcdefghijkl", "10x", "|kl");
        is("|abcdef", "2l3l", "abcde|f");
    }

    #[test]
    fn f_and_t_find_a_character_on_the_line() {
        is("|a,b,c", "f,", "a|,b,c");
        is("|a,b,c", "f,;", "a,b|,c");
        is("a,b|,c", "F,", "a|,b,c");
        is("|ab,c", "t,", "a|b,c");
        is("|a,b,c", "2f,", "a,b|,c");
        is("|ab,c", "df,", "|c");
        is("|ab,c", "dt,", "|,c");
        is("|abc", "fz", "|abc");
        is("|a,b,c", "f,,", "a|,b,c");
    }

    #[test]
    fn x_deletes_characters() {
        is("|abc", "x", "|bc");
        is("|abc", "3x", "|");
        is("ab|c", "x", "a|b");
        is("a|bcd", "2x", "a|d");
        is("ab|c", "X", "a|c");
        is("|\nb", "x", "|\nb");
    }

    #[test]
    fn dw_deletes_words_without_joining_lines() {
        is("|foo bar", "dw", "|bar");
        is("|a b c", "d2w", "|c");
        is("|a b c", "2dw", "|c");
        is("foo |bar\nbaz", "dw", "foo| \nbaz");
        is("|foo bar", "de", "| bar");
        is("foo |bar", "db", "|bar");
        is("ab|cd", "d0", "|cd");
        is("ab|cd", "d$", "a|b");
        is("|ab cd", "dW", "|cd");
    }

    #[test]
    fn dd_deletes_lines() {
        is("a\n|b\nc", "dd", "a\n|c");
        is("a\nb|", "dd", "|a");
        is("|abc", "dd", "|");
        is("|a\nb\nc", "2dd", "|c");
        is("|a\nb\nc", "d2d", "|c");
        is("|a\nb\nc", "3dd", "|");
        is("  a\n|b\n  c", "dd", "  a\n  |c");
    }

    #[test]
    fn d_with_j_k_g_works_on_lines() {
        is("|a\nb\nc", "dj", "|c");
        is("a\nb|\nc", "dk", "|c");
        is("|a\nb", "dk", "|a\nb");
        is("a\n|b\nc", "dG", "|a");
        is("a\n|b\nc", "dgg", "|c");
    }

    #[test]
    fn d_and_c_to_the_end_of_the_line() {
        is("ab|cd\nef", "D", "a|b\nef");
        is("ab|cd", "Cxy<Esc>", "abx|y");
        is("ab|cd", "s1<Esc>", "ab|1d");
        is("a\n|bc\nd", "ccX<Esc>", "a\n|X\nd");
        is("a\n|bc\nd", "SX<Esc>", "a\n|X\nd");
    }

    #[test]
    fn change_and_delete_words_and_inner_words() {
        is("|foo bar", "cwX<Esc>", "|X bar");
        is("fo|o bar", "cwX<Esc>", "fo|X bar");
        is("fo|o bar", "ciwX<Esc>", "|X bar");
        is("fo|o bar", "diw", "| bar");
        is("fo|o bar", "daw", "|bar");
        is("foo ba|r", "daw", "fo|o");
        is("|foo bar baz", "c2wX<Esc>", "|X baz");
        is("a  |  b", "diw", "a|b");
        is("|foo.bar", "ciwX<Esc>", "|X.bar");
    }

    #[test]
    fn yank_and_put() {
        let mut e = ed("|a\nb").keys("yy");
        assert_eq!(e.vim.take_yank().as_deref(), Some("a\n"));
        assert_eq!(e.vim.take_yank(), None);
        assert_eq!(e.keys("p").show(), "a\n|a\nb");
        is("|a\nb", "yyP", "|a\na\nb");
        is("a\n|b", "yyP", "a\n|b\nb");
        is("|foo bar", "ywP", "foo| foo bar");
        is("|foo bar", "ywwp", "foo bfoo| ar");
        is("|a\nb", "ddp", "b\n|a");
        is("|a\nb", "yyjp", "a\nb\n|a");
        is("|a\nb", "yy2p", "a\n|a\na\nb");
        is("|ab", "xp", "b|a");
        is("|abc", "xP", "|abc");
    }

    #[test]
    fn put_takes_the_clipboard_over_an_older_register() {
        let mut e = ed("|ab");
        e.vim.set_clipboard(Some("zz"));
        assert_eq!(e.keys("p").show(), "az|zb");
        let mut e = ed("|a\nb").keys("yy");
        e.vim.set_clipboard(Some("a\n"));
        assert_eq!(e.keys("p").show(), "a\n|a\nb");
        let mut e = ed("|a\nb").keys("yy");
        e.vim.set_clipboard(Some("other"));
        assert_eq!(e.keys("P").show(), "othe|ra\nb");
    }

    #[test]
    fn u_and_ctrl_r_undo_and_redo() {
        is("|abc", "xu", "|abc");
        is("|abc", "xx", "|c");
        is("|abc", "xxu", "|bc");
        is("|abc", "xxuu", "|abc");
        is("|abc", "xxuu<C-r>", "|bc");
        is("|abc", "xxuu<C-r><C-r>", "|c");
        is("|abc", "ihi<Esc>u", "|abc");
        is("|abc", "ddu", "|abc");
        is("|abc def", "dwu", "|abc def");
        is("|abc", "u", "|abc");
        is("|abc", "x<C-r>", "|bc");
    }

    #[test]
    fn insert_commands() {
        is("a|b", "iX<Esc>", "a|Xb");
        is("|ab", "aX<Esc>", "a|Xb");
        is("|ab", "AX<Esc>", "ab|X");
        is("  a|b", "IX<Esc>", "  |Xab");
        is("|a\nb", "oX<Esc>", "a\n|X\nb");
        is("a\n|b", "OX<Esc>", "a\n|X\nb");
        is("|ab", "$aX<Esc>", "ab|X");
        is("|a", "oX<Esc>", "a\n|X");
    }

    #[test]
    fn esc_leaves_insert_one_back_and_normal_hands_it_on() {
        let e = ed("ab|").keys("i");
        assert_eq!(e.vim.mode(), Mode::Insert);
        let e = e.keys("<Esc>");
        assert_eq!(e.vim.mode(), Mode::Normal);
        assert_eq!(e.show(), "a|b");
        assert!(e.last.handled && !e.last.escape);
        let e = e.keys("<Esc>");
        assert!(e.last.escape);
        let e = ed("|abc").keys("d<Esc>x");
        assert_eq!(e.show(), "|bc");
        assert!(!e.last.escape);
        let e = ed("|abc").keys("2<Esc>");
        assert!(!e.last.escape);
    }

    #[test]
    fn insert_mode_leaves_typing_to_the_box() {
        let mut e = ed("|ab").keys("i");
        e.press(Key::Char('x'));
        assert!(!e.last.handled);
        assert_eq!(e.show(), "x|ab");
        e.press(Key::Enter);
        assert!(!e.last.handled);
    }

    #[test]
    fn enter_in_normal_mode_sends() {
        let e = ed("|abc").keys("<CR>");
        assert!(e.last.submit && e.last.handled);
        let e = ed("|abc").keys("d<CR>");
        assert!(!e.last.submit);
    }

    #[test]
    fn unknown_keys_do_nothing_in_normal_mode() {
        let e = ed("|abc").keys("zq");
        assert!(e.last.handled && !e.last.changed);
        assert_eq!(e.show(), "|abc");
    }

    #[test]
    fn visual_mode_selects_and_operates() {
        let e = ed("|foo bar").keys("vl");
        assert_eq!(e.vim.mode(), Mode::Visual);
        assert_eq!(e.vim.visual_range(&e.buf), Some(0..2));
        let e = e.keys("<Esc>");
        assert_eq!(e.vim.mode(), Mode::Normal);
        assert_eq!(e.vim.visual_range(&e.buf), None);
        is("|foo bar", "vld", "|o bar");
        is("|foo bar", "veyP", "fo|ofoo bar");
        is("|foo bar", "vecX<Esc>", "|X bar");
        is("a|b\ncd", "vjd", "|a");
        is("foo| bar", "vhd", "fo|bar");
        is("|abc", "vx", "|bc");
        is("|abc", "v$d", "|");
        is("ab|c", "vhd", "|a");
        is("|ab cd", "vwd", "|d");
    }

    #[test]
    fn visual_o_swaps_the_ends() {
        let e = ed("a|bcd").keys("vlo");
        assert_eq!(e.vim.visual_range(&e.buf), Some(1..3));
        assert_eq!(e.show(), "a|bcd");
    }

    #[test]
    fn dot_repeats_the_last_change() {
        is("|a b c d", "dw.", "|c d");
        is("|abcd", "x.", "|cd");
        is("|foo bar baz", "ciwX<Esc>w.", "X |X baz");
        is("|a\nb", "A!<Esc>j.", "a!\nb|!");
        is("|a\nb\nc\nd", "dd.", "|c\nd");
        is("|abc", "x.u", "|bc");
        is("|a b c d", "w.", "a |b c d");
        is("|a b c d", "dw..", "|d");
    }

    #[test]
    fn j_joins_lines_r_replaces_and_tilde_toggles() {
        is("a|b\n  cd", "J", "ab| cd");
        is("|ab", "J", "|ab");
        is("|abc", "rx", "|xbc");
        is("|abc", "2rx", "x|xc");
        is("|ab", "3rx", "|ab");
        is("|ab", "~", "A|b");
        is("|ab", "2~", "A|B");
        is("|a1", "~", "A|1");
    }

    #[test]
    fn a_count_before_an_operator_and_after_multiply() {
        is("|a b c d e f", "2d2w", "|e f");
        is("|a\nb\nc\nd\ne", "2dj", "|d\ne");
    }

    #[test]
    fn settle_puts_the_caret_on_a_character() {
        let mut e = ed("ab|");
        e.vim.settle(&mut e.buf);
        assert_eq!(e.show(), "a|b");
        let mut e = ed("ab\n|");
        e.vim.settle(&mut e.buf);
        assert_eq!(e.show(), "ab\n|");
    }

    #[test]
    fn pending_shows_what_is_typed() {
        let e = ed("|abc").keys("2d");
        assert_eq!(e.vim.pending(), "2d");
        let e = ed("|abc").keys("g");
        assert_eq!(e.vim.pending(), "g");
        let e = ed("|abc").keys("dd");
        assert_eq!(e.vim.pending(), "");
    }
}

#[cfg(test)]
mod persist_tests {
    use super::*;

    #[test]
    fn the_choice_is_kept_and_off_by_default() {
        let dir = std::env::temp_dir().join(format!("morpheus-vim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert!(!load_enabled(&dir));
        save_enabled(&dir, true).expect("save");
        assert!(load_enabled(&dir));
        save_enabled(&dir, false).expect("save");
        assert!(!load_enabled(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }
}
