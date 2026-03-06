use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::tui_core::{EventResult, Widget};

pub struct SelectBox {
    pub options: Vec<String>,
    pub selected_index: usize,
    pub is_focused: bool,
    pub is_expanded: bool,
    pub label: String,
    pub list_state: ListState,
}

impl SelectBox {
    pub fn new(options: Vec<String>, selected_index: usize, label: impl Into<String>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(selected_index));
        Self {
            options,
            selected_index,
            is_focused: false,
            is_expanded: false,
            label: label.into(),
            list_state,
        }
    }

    pub fn focus(&mut self) {
        self.is_focused = true;
    }

    pub fn blur(&mut self) {
        self.is_focused = false;
        self.is_expanded = false;
    }

    pub fn toggle_expand(&mut self) {
        self.is_expanded = !self.is_expanded;
    }

    pub fn select_next(&mut self) {
        if self.selected_index < self.options.len().saturating_sub(1) {
            self.selected_index += 1;
            self.list_state.select(Some(self.selected_index));
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            self.list_state.select(Some(self.selected_index));
        }
    }
}

impl Widget for SelectBox {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let selected_text = self
            .options
            .get(self.selected_index)
            .cloned()
            .unwrap_or_else(|| "None".to_string());

        let display_text = format!("{}: [{}]", self.label, selected_text);

        let style = if self.is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };

        let paragraph = Paragraph::new(Line::from(Span::styled(display_text, style)))
            .block(Block::default().borders(Borders::NONE));

        frame.render_widget(paragraph, area);

        if self.is_expanded {
            // Render dropdown list below the select box
            let list_area = Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: self.options.len() as u16 + 2,
            };

            let items: Vec<ListItem> = self
                .options
                .iter()
                .map(|opt| ListItem::new(opt.clone()))
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White));

            frame.render_stateful_widget(list, list_area, &mut self.list_state);
        }
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.is_focused {
            return EventResult::Ignored;
        }

        if let Event::Key(KeyEvent { code, .. }) = event {
            if self.is_expanded {
                match code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.select_prev();
                        return EventResult::Consumed;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.select_next();
                        return EventResult::Consumed;
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        self.toggle_expand();
                        return EventResult::Changed;
                    }
                    KeyCode::Esc => {
                        self.toggle_expand();
                        return EventResult::Consumed;
                    }
                    _ => {}
                }
            } else {
                match code {
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        self.toggle_expand();
                        return EventResult::Consumed;
                    }
                    _ => {}
                }
            }
        }
        EventResult::Ignored
    }
}
