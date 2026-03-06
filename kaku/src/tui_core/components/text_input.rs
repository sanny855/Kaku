use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui_core::{EventResult, Widget};

pub struct TextInput {
    pub text: String,
    pub cursor_position: usize,
    pub is_focused: bool,
    pub placeholder: String,
}

impl TextInput {
    pub fn new(text: String) -> Self {
        let cursor_position = text.chars().count();
        Self {
            text,
            cursor_position,
            is_focused: false,
            placeholder: String::new(),
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn focus(&mut self) {
        self.is_focused = true;
    }

    pub fn blur(&mut self) {
        self.is_focused = false;
    }

    pub fn insert_char(&mut self, c: char) {
        let byte_idx = self.char_index_to_byte_index(self.cursor_position);
        self.text.insert(byte_idx, c);
        self.cursor_position += 1;
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            let byte_idx = self.char_index_to_byte_index(self.cursor_position - 1);
            self.text.remove(byte_idx);
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.text.chars().count() {
            self.cursor_position += 1;
        }
    }

    fn char_index_to_byte_index(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(self.text.len())
    }
}

impl Widget for TextInput {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let display_text = if self.text.is_empty() && !self.is_focused {
            Line::from(Span::styled(
                self.placeholder.clone(),
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(self.text.clone())
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(if self.is_focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            });

        let paragraph = Paragraph::new(display_text).block(block);
        frame.render_widget(paragraph, area);

        if self.is_focused {
            // Calculate cursor position
            // This is a simplified version. For full CJK support, we'd need unicode-width
            let text_before_cursor: String = self.text.chars().take(self.cursor_position).collect();
            let cursor_x = area.x + 1 + text_before_cursor.len() as u16; // Simplified width
            let cursor_y = area.y + 1;

            if cursor_x < area.x + area.width - 1 {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.is_focused {
            return EventResult::Ignored;
        }

        if let Event::Key(KeyEvent { code, modifiers, .. }) = event {
            match code {
                KeyCode::Char(c) => {
                    if !modifiers.contains(KeyModifiers::CONTROL)
                        && !modifiers.contains(KeyModifiers::SUPER)
                    {
                        self.insert_char(*c);
                        return EventResult::Changed;
                    }
                }
                KeyCode::Backspace => {
                    self.delete_char();
                    return EventResult::Changed;
                }
                KeyCode::Left => {
                    self.move_cursor_left();
                    return EventResult::Consumed;
                }
                KeyCode::Right => {
                    self.move_cursor_right();
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }
        EventResult::Ignored
    }
}
