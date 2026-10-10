//! Quick Open: paths arrive in batches; matching never reads document contents.
use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;

use anyhow::Result;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::{App, Mode};
use crate::file_search::{
    rank,
    scan::{Scan, Update},
};
use crate::theme::palette;

struct Candidate {
    path: PathBuf,
    name: String,
    relative: String,
}

pub(super) struct FileSearch {
    root: PathBuf,
    scan: Scan,
    query: String,
    files: Vec<Candidate>,
    results: Vec<usize>,
    cursor: usize,
    done: bool,
    errors: usize,
    error: Option<String>,
}

impl FileSearch {
    fn rematch(&mut self, keep_selection: bool) {
        let selected = keep_selection.then(|| self.results.get(self.cursor).copied()).flatten();
        let query = self.query.to_lowercase();
        let mut ranked: Vec<_> = self
            .files
            .iter()
            .enumerate()
            .filter_map(|(index, file)| rank(&file.name, &query).map(|score| (score, index)))
            .collect();
        ranked.sort_by(|(a, i), (b, j)| {
            a.cmp(b).then_with(|| {
                self.files.get(*i).map(|f| &f.relative).cmp(&self.files.get(*j).map(|f| &f.relative))
            })
        });
        self.results = ranked.into_iter().map(|(_, index)| index).collect();
        self.cursor = selected.and_then(|index| self.results.iter().position(|i| *i == index)).unwrap_or(0);
    }

    fn selected_path(&self) -> Option<PathBuf> {
        self.results.get(self.cursor).and_then(|i| self.files.get(*i)).map(|f| f.path.clone())
    }
}

impl App {
    pub(super) fn start_file_search(&mut self) -> Result<()> {
        let Some(tree) = &self.tree else { return Ok(()) };
        let root = tree.root().to_path_buf();
        self.file_search = Some(FileSearch {
            scan: Scan::start(root.clone(), tree.show_hidden())?,
            root,
            query: String::new(),
            files: Vec::new(),
            results: Vec::new(),
            cursor: 0,
            done: false,
            errors: 0,
            error: None,
        });
        self.mode = Mode::FileSearch;
        Ok(())
    }

    pub(crate) fn file_search_scanning(&self) -> bool {
        self.file_search.as_ref().is_some_and(|search| !search.done)
    }

    /// Called on idle ticks too, so an untouched popup still receives scan results.
    pub(crate) fn poll_file_search(&mut self) -> bool {
        let Some(search) = &mut self.file_search else { return false };
        if search.done {
            return false;
        }
        let mut changed = false;
        let mut added = false;
        for _ in 0..8 {
            match search.scan.updates.try_recv() {
                Ok(Update::Files(paths)) => {
                    for path in paths {
                        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        let relative =
                            path.strip_prefix(&search.root).unwrap_or(&path).to_string_lossy().into_owned();
                        search.files.push(Candidate { path, name, relative });
                    }
                    added = true;
                }
                Ok(Update::Error(error)) => {
                    search.errors += 1;
                    search.error = Some(error);
                }
                Ok(Update::Done) => search.done = true,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    search.done = true;
                    search.errors += 1;
                    search.error = Some("filename search stopped unexpectedly".into());
                }
            }
            changed = true;
            if search.done {
                break;
            }
        }
        if added {
            search.rematch(true);
        }
        changed
    }

    pub(super) fn file_search_paste(&mut self, text: &str) {
        if let Some(search) = &mut self.file_search {
            search.query.extend(text.chars().filter(|ch| !ch.is_control()));
            search.rematch(false);
        }
    }

    pub(super) fn file_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.file_search = None;
                self.mode = Mode::Browse;
            }
            KeyCode::Enter => {
                let path = self.file_search.as_ref().and_then(FileSearch::selected_path);
                if let Some(path) = path {
                    if let Err(error) = self.open_file_path(&path) {
                        if let Some(search) = &mut self.file_search {
                            search.error = Some(format!("{error:#}"));
                        }
                        return;
                    }
                    let revealed = self.tree.as_mut().map(|tree| tree.reveal(&path)).transpose();
                    match revealed {
                        Ok(Some(Some(index))) => self.tree_cursor = index,
                        Err(error) => {
                            self.status =
                                Some(format!("opened file; revealing {}: {error:#}", path.display()));
                        }
                        _ => {}
                    }
                    self.sync_tree_counts();
                    self.keep_tree_cursor_visible(usize::from(self.geometry.tree.height));
                    self.file_search = None;
                    self.mode = Mode::Browse;
                }
            }
            KeyCode::Up => {
                if let Some(search) = &mut self.file_search {
                    search.cursor = search.cursor.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(search) = &mut self.file_search {
                    search.cursor = (search.cursor + 1).min(search.results.len().saturating_sub(1));
                }
            }
            KeyCode::Backspace => {
                if let Some(search) = &mut self.file_search {
                    search.query.pop();
                    search.rematch(false);
                }
            }
            KeyCode::Char(ch) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                self.file_search_paste(&ch.to_string());
            }
            _ => {}
        }
    }

    pub(super) fn draw_file_search(&self, frame: &mut Frame) {
        let Some(search) = &self.file_search else { return };
        let area = frame.area();
        let width = area.width.saturating_sub(4).clamp(1, 90).min(area.width);
        let height = area.height.saturating_sub(2).clamp(1, 18).min(area.height);
        let rect = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::Cyan))
            .title(" Open Markdown file ")
            .title_bottom(" ↑↓ select · Enter open · Esc cancel ");
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        let available = usize::from(inner.height.saturating_sub(3));
        let start = search.cursor.saturating_sub(available.saturating_sub(1));
        let mut lines = vec![Line::from(format!("> {}", search.query))];
        for (row, index) in search.results.iter().enumerate().skip(start).take(available) {
            if let Some(file) = search.files.get(*index) {
                let style = if row == search.cursor { palette().selection } else { Style::new() };
                lines.push(Line::from(Span::styled(format!(" {}", file.relative), style)));
            }
        }
        if search.results.is_empty() && available > 0 {
            lines.push(Line::from(if !search.done {
                " Searching…"
            } else if search.errors > 0 {
                " No matches in readable folders"
            } else {
                " No matching files"
            }));
        }
        let progress = if !search.done {
            "scanning…"
        } else if search.errors > 0 {
            "incomplete"
        } else {
            "complete"
        };
        lines.push(Line::from(format!(
            " {progress} · {} matches · {} unreadable",
            search.results.len(),
            search.errors
        )));
        if let Some(error) = &search.error {
            lines.push(Line::from(Span::styled(error.clone(), Style::new().fg(Color::Yellow))));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests;
