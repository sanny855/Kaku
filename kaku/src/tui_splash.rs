use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::tui_core::theme::muted;

/// Render a centered splash frame. Call via `terminal.draw(|f| render_splash(f, "Loading..."))`.
/// The next real UI draw naturally overwrites this frame.
pub fn render_splash(frame: &mut ratatui::Frame, message: &str) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default(), area);

    let text = format!("●  {message}");
    let chunks = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .split(area);

    let para = Paragraph::new(Line::from(text).style(Style::default().fg(muted())))
        .alignment(Alignment::Center);
    frame.render_widget(para, chunks[1]);
}
