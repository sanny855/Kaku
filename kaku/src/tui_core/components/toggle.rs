use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::tui_core::{EventResult, Widget};

pub struct Toggle {
    pub is_on: bool,
    pub is_focused: bool,
    pub label: String,
}

impl Toggle {
    pub fn new(is_on: bool, label: impl Into<String>) -> Self {
        Self {
            is_on,
            is_focused: false,
            label: label.into(),
        }
    }

    pub fn focus(&mut self) {
        self.is_focused = true;
    }

    pub fn blur(&mut self) {
        self.is_focused = false;
    }

    pub fn toggle(&mut self) {
        self.is_on = !self.is_on;
    }
}

impl Widget for Toggle {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let status_text = if self.is_on { "[x]" } else { "[ ]" };
        let display_text = format!("{} {}", status_text, self.label);

        let style = if self.is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };

        let paragraph = Paragraph::new(Line::from(Span::styled(display_text, style)))
            .block(Block::default().borders(Borders::NONE));

        frame.render_widget(paragraph, area);
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.is_focused {
            return EventResult::Ignored;
        }

        if let Event::Key(KeyEvent { code, .. }) = event {
            match code {
                KeyCode::Char(' ') | KeyCode::Enter => {
                    self.toggle();
                    return EventResult::Changed;
                }
                _ => {}
            }
        }
        EventResult::Ignored
    }
}
