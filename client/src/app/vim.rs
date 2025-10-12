use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use log::debug;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Widget},
};
use tui_textarea::TextArea;

use crate::app::{
    event::{EventSender, InteractiveEvent},
    resources::AppResources,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
}

pub struct SendMessageWidget {
    resources: Arc<AppResources>,
    text_area: TextArea<'static>,
}

impl SendMessageWidget {
    pub fn new(resources: Arc<AppResources>) -> Self {
        let mut text_area = TextArea::new(Vec::new());
        text_area.set_block(
            Block::bordered()
                .title_top(Line::from("Normal").left_aligned())
                .border_style(Style::new().fg(Color::Rgb(255, 242, 197))),
        );

        Self {
            resources,
            text_area,
        }
    }
    pub async fn input(&mut self, event: KeyEvent, event_sender: &EventSender) -> bool {
        match self.resources.state.read().await.mode {
            VimMode::Normal => match event {
                KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press,
                    ..
                } => {
                    event_sender.send(InteractiveEvent::Quit).await.unwrap();
                    false
                }
                KeyEvent {
                    code: KeyCode::Enter,
                    kind: KeyEventKind::Press,
                    ..
                } => {
                    debug!("Sending message");
                    self.text_area.select_all();
                    let need_rerender = self.text_area.cut();
                    event_sender
                        .send(InteractiveEvent::SendMessage {
                            content: self.text_area.yank_text(),
                        })
                        .await
                        .unwrap();
                    debug!("Sent message: {}", self.text_area.yank_text());
                    need_rerender
                }
                KeyEvent {
                    code: KeyCode::Char('i'),
                    kind: KeyEventKind::Press,
                    ..
                } => {
                    self.resources.state.write().await.mode = VimMode::Insert;
                    self.text_area.set_block(
                        Block::bordered()
                            .title_top(Line::from("Insert").left_aligned())
                            .border_style(Style::new().fg(Color::Rgb(255, 242, 197))),
                    );
                    true
                }
                KeyEvent {
                    code: KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down,
                    kind: KeyEventKind::Press,
                    ..
                } => self.text_area.input(event),
                _ => false,
            },
            VimMode::Insert => {
                if event.code == KeyCode::Esc {
                    self.resources.state.write().await.mode = VimMode::Normal;
                    self.text_area.set_block(
                        Block::bordered()
                            .title_top(Line::from("Normal").left_aligned())
                            .border_style(Style::new().fg(Color::Rgb(255, 242, 197))),
                    );
                    true
                } else {
                    self.text_area.input(event)
                }
            }
        }
    }
}

impl Widget for &mut SendMessageWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Widget::render(&self.text_area, area, buf);
    }
}
