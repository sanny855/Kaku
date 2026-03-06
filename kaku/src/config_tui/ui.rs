use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::App;
use crate::tui_core::theme::{bg, muted, purple, text_fg};
use crate::tui_core::Widget;

pub(super) fn ui(frame: &mut ratatui::Frame, app: &mut App) {
    let full = frame.area();
    if full.width < 2 || full.height < 2 {
        return;
    }

    let area = Rect::new(full.x, full.y, full.width - 1, full.height - 1);

    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(Style::default().bg(bg())), area);

    let chunks = Layout::vertical([
        Constraint::Length(2), // header
        Constraint::Fill(1),   // content
    ])
    .split(area);

    render_header(frame, chunks[0]);
    
    // Render the form
    app.form.render(frame, chunks[1]);
}

fn render_header(frame: &mut ratatui::Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            "  Kaku",
            Style::default().fg(purple()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(muted())),
        Span::styled("Settings", Style::default().fg(text_fg())),
    ]);
    frame.render_widget(Paragraph::new(vec![line, Line::from("")]), area);
}
