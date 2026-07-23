#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
struct TerminalPoint {
    line: i32,
    column: usize,
}

/// Per-line content hashes, computed once when the content is built (i.e. once
/// per published snapshot, not per frame) so the renderer can key its row cache
/// without re-hashing every visible row on every frame. Derived purely from
/// `cells`, so it is excluded from content identity (see `PartialEq`).
#[derive(Clone, Default)]
struct PrecomputedRowHashes(HashMap<i32, u64>);

impl PartialEq for PrecomputedRowHashes {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl PrecomputedRowHashes {
    fn from_cells(cells: &[TerminalIndexedCell]) -> Self {
        let mut hashers: HashMap<i32, DefaultHasher> = HashMap::new();
        for indexed in cells {
            terminal_cell_hash(
                &indexed.cell,
                hashers.entry(indexed.point.line).or_default(),
            );
        }
        Self(
            hashers
                .into_iter()
                .map(|(line, hasher)| (line, hasher.finish()))
                .collect(),
        )
    }

    fn get(&self, line: i32) -> Option<u64> {
        self.0.get(&line).copied()
    }
}

#[derive(Clone, PartialEq)]
struct TerminalContent {
    cells: Vec<TerminalIndexedCell>,
    row_hashes: PrecomputedRowHashes,
    wrapped_lines: HashSet<i32>,
    cursor: TerminalScreenCursorSnapshot,
    display_offset: usize,
    viewport_start_line: i32,
    columns: usize,
    screen_lines: usize,
    total_lines: usize,
    visible_rows: usize,
    visible_row_shift: usize,
    input_mode: TerminalInputMode,
    title: Option<String>,
    prompt_marks: Vec<usize>,
    images: Vec<TerminalImagePlacement>,
    #[cfg(test)]
    scrolled_to_bottom: bool,
}

/// Inline image anchored to an absolute buffer line, like cells.
#[derive(Clone, PartialEq)]
struct TerminalImagePlacement {
    line: i32,
    image: TerminalScreenImage,
}

impl TerminalContent {
    fn from_screen_snapshot(snapshot: TerminalScreenSnapshot) -> Self {
        let total_lines = snapshot.total_lines.max(snapshot.rows);
        let viewport_start_line = total_lines
            .saturating_sub(snapshot.display_offset)
            .saturating_sub(snapshot.rows) as i32;
        let images = snapshot
            .images
            .iter()
            .map(|image| TerminalImagePlacement {
                line: viewport_start_line + image.row,
                image: image.clone(),
            })
            .collect();
        let cells = snapshot
            .cells
            .into_iter()
            .map(|cell| TerminalIndexedCell {
                point: TerminalPoint {
                    line: viewport_start_line + cell.row,
                    column: cell.col,
                },
                cell,
            })
            .collect::<Vec<_>>();
        let row_hashes = PrecomputedRowHashes::from_cells(&cells);
        let wrapped_lines = snapshot
            .wrapped_rows
            .iter()
            .enumerate()
            .filter_map(|(row, wrapped)| wrapped.then_some(viewport_start_line + row as i32))
            .collect::<HashSet<i32>>();
        Self {
            cells,
            row_hashes,
            wrapped_lines,
            cursor: snapshot.cursor,
            display_offset: snapshot.display_offset,
            viewport_start_line,
            columns: snapshot.cols,
            screen_lines: snapshot.rows,
            total_lines,
            visible_rows: snapshot.rows,
            visible_row_shift: 0,
            input_mode: snapshot.input_mode,
            title: snapshot.title,
            prompt_marks: snapshot.prompt_marks,
            images,
            #[cfg(test)]
            scrolled_to_bottom: snapshot.display_offset == 0,
        }
    }

    fn with_visible_row_shift(mut self, visible_rows: usize) -> Self {
        self.visible_rows = visible_rows.min(self.screen_lines);
        self.visible_row_shift = self.screen_lines.saturating_sub(self.visible_rows);
        self
    }

    fn visible_rows(&self) -> usize {
        self.visible_rows
    }

    fn display_row_for_line(&self, line: i32) -> Option<usize> {
        let row = line - self.viewport_start_line - self.visible_row_shift as i32;
        if row < 0 || row as usize >= self.visible_rows {
            return None;
        }
        Some(row as usize)
    }

    fn line_for_display_row(&self, row: usize) -> i32 {
        self.viewport_start_line + row as i32 + self.visible_row_shift as i32
    }

    fn line_in_snapshot(&self, line: i32) -> bool {
        let start = self.viewport_start_line;
        let end = self.last_snapshot_line().unwrap_or(start);
        start <= line && line <= end
    }

    /// True when this buffer line soft-wrapped into the next (no hard break),
    /// so copy/line-select should treat it as one continuous logical line.
    fn is_wrapped_line(&self, line: i32) -> bool {
        self.wrapped_lines.contains(&line)
    }

    fn last_snapshot_line(&self) -> Option<i32> {
        self.screen_lines
            .checked_sub(1)
            .map(|row| self.viewport_start_line + row as i32)
    }

    fn display_cursor(&self) -> DisplayCursor {
        DisplayCursor {
            row: self.cursor.row as i32,
            col: self.cursor.col,
        }
        .shifted(self.visible_row_shift)
    }
}

#[derive(Clone, PartialEq)]
struct TerminalIndexedCell {
    point: TerminalPoint,
    cell: TerminalScreenCellSnapshot,
}

impl TerminalIndexedCell {
    fn col(&self) -> usize {
        self.point.column
    }

    fn line(&self) -> i32 {
        self.point.line
    }

    fn text(&self) -> &str {
        &self.cell.text
    }

    fn is_spacer_or_empty(&self) -> bool {
        self.cell.hidden || self.cell.text.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TerminalLink {
    url: String,
    line: i32,
    range: Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TerminalPath {
    path: String,
}

fn terminal_link_at_cell(
    content: &TerminalContent,
    point: TerminalCellPoint,
) -> Option<TerminalLink> {
    let line = content.line_for_display_row(point.row);
    let row_cells: Vec<&TerminalIndexedCell> = content
        .cells
        .iter()
        .filter(|indexed| indexed.line() == line)
        .collect();
    if row_cells.is_empty() {
        return None;
    }

    if let Some((url, range)) = terminal_osc8_link_at(&row_cells, point.col) {
        return Some(TerminalLink { url, line, range });
    }
    let logical_text = terminal_logical_text_at(content, line, point.col)?;
    terminal_plain_url_at(&logical_text.text, logical_text.clicked_col).map(
        |(url, logical_range)| TerminalLink {
            url,
            line,
            range: logical_text.local_range(logical_range),
        },
    )
}

/// Resolves only POSIX absolute paths. Filesystem access is intentionally deferred until the
/// menu action runs so a slow or disconnected volume cannot stall terminal pointer handling.
fn terminal_path_at_cell(
    content: &TerminalContent,
    point: TerminalCellPoint,
) -> Option<TerminalPath> {
    let line = content.line_for_display_row(point.row);
    let row_cells: Vec<&TerminalIndexedCell> = content
        .cells
        .iter()
        .filter(|indexed| indexed.line() == line)
        .collect();
    if row_cells.is_empty() {
        return None;
    }

    let logical_text = terminal_logical_text_at(content, line, point.col)?;
    terminal_plain_path_at(&logical_text.text, logical_text.clicked_col)
        .map(|(path, _)| TerminalPath { path })
}

/// OSC 8 hyperlink under the pointer; the range spans every cell on the row
/// carrying the same URI so the whole label underlines together.
fn terminal_osc8_link_at(
    row_cells: &[&TerminalIndexedCell],
    col: usize,
) -> Option<(String, Range<usize>)> {
    let uri = row_cells.iter().find_map(|indexed| {
        let width = indexed.cell.width.max(1);
        (indexed.col() <= col && col < indexed.col() + width)
            .then(|| indexed.cell.link.clone())
            .flatten()
    })?;
    if !is_openable_terminal_url(&uri) {
        return None;
    }
    let mut range: Option<Range<usize>> = None;
    for indexed in row_cells {
        if indexed.cell.link.as_deref() != Some(uri.as_str()) {
            continue;
        }
        let end = indexed.col() + indexed.cell.width.max(1);
        range = Some(match range {
            Some(range) => range.start.min(indexed.col())..range.end.max(end),
            None => indexed.col()..end,
        });
    }
    Some((uri, range?))
}

fn terminal_row_text(row_cells: &[&TerminalIndexedCell]) -> Vec<(usize, char)> {
    let mut text: Vec<(usize, char)> = Vec::new();
    for indexed in row_cells {
        let col = indexed.col();
        if indexed.is_spacer_or_empty() {
            continue;
        }
        let next_col = text
            .last()
            .map(|(last_col, last_ch)| last_col.saturating_add(terminal_char_width(*last_ch)))
            .unwrap_or(0);
        for spacer_col in next_col..col {
            text.push((spacer_col, ' '));
        }
        for (offset, ch) in indexed.text().chars().enumerate() {
            text.push((col + offset, ch));
        }
    }
    text
}

struct TerminalLogicalText {
    text: Vec<(usize, char)>,
    clicked_col: usize,
    current_row: Range<usize>,
}

impl TerminalLogicalText {
    fn local_range(&self, logical_range: Range<usize>) -> Range<usize> {
        logical_range.start.max(self.current_row.start) - self.current_row.start
            ..logical_range.end.min(self.current_row.end) - self.current_row.start
    }
}

/// Joins the visible rows connected by terminal soft-wrap markers. This is only called for
/// modifier-assisted hover/click handling, so normal rendering never pays the reconstruction cost.
fn terminal_logical_text_at(
    content: &TerminalContent,
    line: i32,
    col: usize,
) -> Option<TerminalLogicalText> {
    if !content.line_in_snapshot(line) {
        return None;
    }
    let first_snapshot_line = content.viewport_start_line;
    let last_snapshot_line = content.last_snapshot_line()?;
    let mut start_line = line;
    while start_line > first_snapshot_line && content.is_wrapped_line(start_line - 1) {
        start_line -= 1;
    }
    let mut end_line = line;
    while end_line < last_snapshot_line && content.is_wrapped_line(end_line) {
        end_line += 1;
    }

    let mut text = Vec::new();
    for current_line in start_line..=end_line {
        let row_cells: Vec<&TerminalIndexedCell> = content
            .cells
            .iter()
            .filter(|indexed| indexed.line() == current_line)
            .collect();
        let logical_base = (current_line - start_line) as usize * content.columns;
        text.extend(
            terminal_row_text(&row_cells)
                .into_iter()
                .map(|(col, ch)| (logical_base + col, ch)),
        );
    }

    let current_start = (line - start_line) as usize * content.columns;
    Some(TerminalLogicalText {
        text,
        clicked_col: current_start + col,
        current_row: current_start..current_start + content.columns,
    })
}

fn terminal_plain_url_at(row_text: &[(usize, char)], col: usize) -> Option<(String, Range<usize>)> {
    static STRICT_URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?i)(?:https?|file)://[^\s"'!*(){}|\\^<>`]*[^\s"':,.!?{}|\\^~\[\]`()<>]"#)
            .expect("valid terminal URL regex")
    });

    let text: String = row_text.iter().map(|(_, ch)| *ch).collect();
    for candidate in STRICT_URL_REGEX.find_iter(&text) {
        let start = candidate.start();
        let end = candidate.end();
        let start_index = text[..start].chars().count();
        let end_index = text[..end].chars().count();
        let Some(start_col) = row_text.get(start_index).map(|(col, _)| *col) else {
            continue;
        };
        let end_col = row_text
            .get(end_index.saturating_sub(1))
            .map(|(col, ch)| col.saturating_add(terminal_char_width(*ch)))
            .unwrap_or(start_col);
        if start_col <= col && col < end_col {
            let url = candidate.as_str().to_string();
            if is_openable_terminal_url(&url) {
                return Some((url, start_col..end_col));
            }
        }
    }
    None
}

/// Finds the absolute path that owns `col`, including shell-quoted paths and backslash-escaped
/// spaces. Delimiters commonly emitted by logs are removed without scanning beyond this row.
fn terminal_plain_path_at(row_text: &[(usize, char)], col: usize) -> Option<(String, Range<usize>)> {
    let clicked_index = row_text.iter().position(|(cell_col, ch)| {
        *cell_col <= col && col < cell_col.saturating_add(terminal_char_width(*ch))
    })?;
    let chars: Vec<char> = row_text.iter().map(|(_, ch)| *ch).collect();

    for start in 0..chars.len() {
        if chars[start] != '/' || !terminal_path_start_boundary(&chars, start) {
            continue;
        }

        let quote = start
            .checked_sub(1)
            .and_then(|index| matches!(chars[index], '\'' | '"').then_some(chars[index]));
        let mut end = start;
        let mut escaped = false;
        while end < chars.len() {
            let ch = chars[end];
            if escaped {
                escaped = false;
                end += 1;
                continue;
            }
            if ch == '\\' && quote != Some('\'') {
                escaped = true;
                end += 1;
                continue;
            }
            if quote.is_some_and(|quote| ch == quote) {
                break;
            }
            if quote.is_none()
                && (ch.is_whitespace() || matches!(ch, '\'' | '"' | '<' | '>' | '|' | '`'))
            {
                break;
            }
            end += 1;
        }

        end = trim_terminal_path_end(&chars, start, end);
        if !(start <= clicked_index && clicked_index < end) {
            continue;
        }
        let start_col = row_text[start].0;
        let end_col = row_text[end - 1]
            .0
            .saturating_add(terminal_char_width(row_text[end - 1].1));
        let raw: String = chars[start..end].iter().collect();
        return Some((unescape_terminal_path(&raw), start_col..end_col));
    }
    None
}

fn terminal_path_start_boundary(chars: &[char], start: usize) -> bool {
    // A scheme separator starts a URL, not a POSIX path.
    if start > 0 && chars[start - 1] == ':' && chars.get(start + 1) == Some(&'/') {
        return false;
    }
    start == 0
        || chars[start - 1].is_whitespace()
        || matches!(chars[start - 1], '\'' | '"' | '=' | '(' | '[' | '{' | ',' | ':')
}

fn trim_terminal_path_end(chars: &[char], start: usize, mut end: usize) -> usize {
    while end > start + 1 && matches!(chars[end - 1], ',' | '.' | ';' | '!' | '?' | ')' | ']' | '}')
    {
        end -= 1;
    }
    end
}

fn unescape_terminal_path(path: &str) -> String {
    let mut result = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && let Some(next) = chars.peek().copied()
            && (next.is_whitespace() || matches!(next, '\\' | '\'' | '"' | '(' | ')' | '[' | ']'))
        {
            result.push(next);
            chars.next();
            continue;
        }
        result.push(ch);
    }
    result
}

fn is_openable_terminal_url(url: &str) -> bool {
    url::Url::parse(url)
        .map(|url| matches!(url.scheme(), "http" | "https" | "file"))
        .unwrap_or(false)
}

fn terminal_char_width(ch: char) -> usize {
    if ch.is_ascii() { 1 } else { 2 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DisplayCursor {
    row: i32,
    col: usize,
}

impl DisplayCursor {
    fn shifted(self, row_shift: usize) -> Self {
        Self {
            row: self.row - row_shift as i32,
            col: self.col,
        }
    }
}

fn terminal_cell_hash(cell: &TerminalScreenCellSnapshot, hasher: &mut DefaultHasher) {
    cell.col.hash(hasher);
    cell.text.hash(hasher);
    cell.width.hash(hasher);
    terminal_screen_color_hash(&cell.fg, hasher);
    terminal_screen_color_hash(&cell.bg, hasher);
    cell.bold.hash(hasher);
    cell.dim.hash(hasher);
    cell.italic.hash(hasher);
    cell.underline.hash(hasher);
    if let Some(color) = &cell.underline_color {
        terminal_screen_color_hash(color, hasher);
    }
    cell.inverse.hash(hasher);
    cell.hidden.hash(hasher);
    cell.strikeout.hash(hasher);
}

fn terminal_screen_color_hash(color: &TerminalScreenColor, hasher: &mut DefaultHasher) {
    match color {
        TerminalScreenColor::Default => 0u8.hash(hasher),
        TerminalScreenColor::Named { name } => {
            1u8.hash(hasher);
            name.hash(hasher);
        }
        TerminalScreenColor::Rgb { r, g, b } => {
            2u8.hash(hasher);
            r.hash(hasher);
            g.hash(hasher);
            b.hash(hasher);
        }
        TerminalScreenColor::Indexed { index } => {
            3u8.hash(hasher);
            index.hash(hasher);
        }
    }
}

fn terminal_row_hash(cells: &[TerminalIndexedCell]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for indexed in cells {
        terminal_cell_hash(&indexed.cell, &mut hasher);
    }
    hasher.finish()
}
