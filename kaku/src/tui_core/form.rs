use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    Frame,
};

use crate::tui_core::{EventResult, Widget};
use crate::tui_core::components::{
    text_input::TextInput,
    toggle::Toggle,
    select_box::SelectBox,
    list_editor::ListEditor,
};

pub enum FormFieldWidget {
    TextInput(TextInput),
    Toggle(Toggle),
    SelectBox(SelectBox),
    ListEditor(ListEditor),
}

impl Widget for FormFieldWidget {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        match self {
            FormFieldWidget::TextInput(w) => w.render(frame, area),
            FormFieldWidget::Toggle(w) => w.render(frame, area),
            FormFieldWidget::SelectBox(w) => w.render(frame, area),
            FormFieldWidget::ListEditor(w) => w.render(frame, area),
        }
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        match self {
            FormFieldWidget::TextInput(w) => w.handle_event(event),
            FormFieldWidget::Toggle(w) => w.handle_event(event),
            FormFieldWidget::SelectBox(w) => w.handle_event(event),
            FormFieldWidget::ListEditor(w) => w.handle_event(event),
        }
    }
}

pub struct FormField<T> {
    pub key: String,
    pub label: String,
    pub widget: FormFieldWidget,
    pub data: T,
}

pub struct FormApp<T> {
    pub fields: Vec<FormField<T>>,
    pub selected_index: usize,
}

impl<T> FormApp<T> {
    pub fn new(fields: Vec<FormField<T>>) -> Self {
        let mut app = Self {
            fields,
            selected_index: 0,
        };
        app.update_focus();
        app
    }

    pub fn update_focus(&mut self) {
        for (i, field) in self.fields.iter_mut().enumerate() {
            let is_focused = i == self.selected_index;
            match &mut field.widget {
                FormFieldWidget::TextInput(w) => w.is_focused = is_focused,
                FormFieldWidget::Toggle(w) => w.is_focused = is_focused,
                FormFieldWidget::SelectBox(w) => w.is_focused = is_focused,
                FormFieldWidget::ListEditor(w) => w.is_focused = is_focused,
            }
        }
    }

    pub fn select_next(&mut self) {
        if self.selected_index < self.fields.len().saturating_sub(1) {
            self.selected_index += 1;
            self.update_focus();
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            self.update_focus();
        }
    }

    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        if let Some(field) = self.fields.get_mut(self.selected_index) {
            let result = field.widget.handle_event(event);
            if matches!(result, EventResult::Consumed | EventResult::Changed) {
                return result;
            }
        }

        if let Event::Key(KeyEvent { code, .. }) = event {
            match code {
                KeyCode::Up | KeyCode::BackTab => {
                    self.select_prev();
                    return EventResult::Consumed;
                }
                KeyCode::Down | KeyCode::Tab => {
                    self.select_next();
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        EventResult::Ignored
    }
}

impl<T> Widget for FormApp<T> {
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        use ratatui::layout::{Constraint, Layout};
        
        let constraints: Vec<Constraint> = self.fields.iter().map(|_| Constraint::Length(3)).collect();
        let chunks = Layout::vertical(constraints).split(area);
        
        for (i, field) in self.fields.iter_mut().enumerate() {
            if i < chunks.len() {
                field.widget.render(frame, chunks[i]);
            }
        }
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        self.handle_event(event)
    }
}
