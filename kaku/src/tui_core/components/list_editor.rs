use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::tui_core::{EventResult, Widget};

pub struct ListEditor {
    pub items: Vec<String>,
    pub selected_index: usize,
    pub is_focused: bool,
    pub label: String,
    pub list_state: ListState,
}

impl ListEditor {
    pub fn new(items: Vec<String>, label: impl Into<String>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            items,
            selected_index: 0,
            is_focused: false,
            label: label.into(),
            list_state,
        }
    }

    pub fn focus(&mut self) {
        self.is_focused = true;
    }

    pub fn blur(&mut self) {
        self.is_focused = false;
    }

    pub fn select_next(&mut self) {
        if self.selected_index < self.items.len().saturating_sub(1) {
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

    pub fn add_item(&mut self, item: String) {
        self.items.push(item);
        self.selected_index = self.items.len() - 1;
        self.list_state.select(Some(self.selected_index));
    }

    pub fn remove_item(&mut self) {
        if !self.items.is_empty() {
            self.items.remove(self.selected_index);
            if self.selected_index >= self.items.len() {
                self.selected_index = self.items.len().saturating_sub(1);
            }
            self.list_state.select(Some(self.selected_index));
        }
    }
}

impl Widget for ListEditor {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let display_text = format!("{}:", self.label);

        let style = if self.is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };

        let paragraph = Paragraph::new(Line::from(Span::styled(display_text, style)))
            .block(Block::default().borders(Borders::NONE));

        frame.render_widget(paragraph, area);

        let list_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        };

        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|item| ListItem::new(item.clone()))
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White));

        frame.render_stateful_widget(list, list_area, &mut self.list_state);
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.is_focused {
            return EventResult::Ignored;
        }

        if let Event::Key(KeyEvent { code, .. }) = event {
            match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.select_prev();
                    return EventResult::Consumed;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.select_next();
                    return EventResult::Consumed;
                }
                KeyCode::Backspace | KeyCode::Delete => {
                    self.remove_item();
                    return EventResult::Changed;
                }
                _ => {}
            }
        }
        EventResult::Ignored
    }
}
