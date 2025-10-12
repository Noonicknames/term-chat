use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use log::{debug, info};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Widget},
};
use tui_textarea::{CursorMove, TextArea};

use crate::app::{
    event::{EventSender, InteractiveEvent},
    resources::AppResources,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
    Visual,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Action {
    Char(char),
    Number(u32),
    CharNumber(char, u32),
    #[default]
    Empty,
}

impl Action {
    pub fn clear(&mut self) {
        *self = Self::Empty
    }

    pub fn get_char(&self) -> Option<char> {
        match *self {
            Action::Char(c) | Action::CharNumber(c, _) => Some(c),
            _ => None,
        }
    }

    pub fn is_char(&self) -> bool {
        match self {
            Self::Char(_) => true,
            _ => false,
        }
    }

    pub fn get_number(&self) -> Option<u32> {
        match *self {
            Action::Number(num) | Action::CharNumber(_, num) => Some(num),
            _ => None,
        }
    }

    pub fn is_number(&self) -> bool {
        match self {
            Self::Number(_) => true,
            _ => false,
        }
    }

    pub fn from_key(code: KeyCode) -> Self {
        match code {
            KeyCode::Char(num @ '0'..'9') => Self::Number(num as u32 - '0' as u32),
            KeyCode::Char(c) => Self::Char(c),
            _ => Self::Empty,
        }
    }
    pub fn update(&mut self, event: KeyEvent) {
        if event.kind != KeyEventKind::Press {
            return;
        }

        match (*self, Self::from_key(event.code)) {
            (Self::Char(_) | Self::Empty | Self::Number(_), Self::Char(c)) => *self = Self::Char(c),
            (Self::Empty, Self::Number(num)) => *self = Self::Number(num),
            (Self::Char(c), Self::Number(num)) => *self = Self::CharNumber(c, num),
            (Self::CharNumber(c, num_before), Self::Number(num)) => {
                *self = Self::CharNumber(c, num_before * 10 + num)
            }
            (Self::CharNumber(_, _), Self::Char(c2)) => *self = Self::Char(c2),
            (Self::Number(num_before), Self::Number(num)) => {
                *self = Self::Number(num_before * 10 + num)
            }
            (_, Self::Empty) => *self = Self::Empty,
            (_, Self::CharNumber(_, _)) => unreachable!(),
        }
    }
}

pub struct SendMessageWidget {
    resources: Arc<AppResources>,
    text_area: TextArea<'static>,
    command_text_area: TextArea<'static>,
    prev_action: Action,
    is_line_yank: bool,
}

fn key_to_cursor_move(code: KeyCode) -> Option<CursorMove> {
    match code {
        KeyCode::Char('h') | KeyCode::Left => Some(CursorMove::Back),
        KeyCode::Char('j') | KeyCode::Down => Some(CursorMove::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(CursorMove::Up),
        KeyCode::Char('l') | KeyCode::Right => Some(CursorMove::Forward),
        KeyCode::Char('w') => Some(CursorMove::WordForward),
        KeyCode::Char('b') => Some(CursorMove::WordBack),
        _ => None,
    }
}

impl SendMessageWidget {
    pub fn new(resources: Arc<AppResources>) -> Self {
        let mut text_area = TextArea::new(Vec::new());
        text_area.set_block(
            Block::bordered()
                .title_top(Line::from("Normal").left_aligned())
                .border_style(Style::new().fg(Color::Rgb(255, 242, 197))),
        );

        let mut command_text_area = TextArea::new(Vec::new());
        command_text_area.set_block(
            Block::bordered()
                .title_top(Line::from("Command").left_aligned())
                .border_style(Style::new().fg(Color::Rgb(255, 242, 197))),
        );

        let prev_action = Action::Empty;
        let is_line_yank = false;

        Self {
            resources,
            text_area,
            command_text_area,
            prev_action,
            is_line_yank,
        }
    }
    async fn send_message(&mut self, event_sender: &EventSender) -> bool {
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
    async fn normal_input(&mut self, event: KeyEvent, event_sender: &EventSender) -> bool {
        match event {
            KeyEvent {
                code: KeyCode::Esc,
                kind: KeyEventKind::Press,
                ..
            } => {
                self.prev_action.clear();
                false
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
                self.prev_action.clear();
                true
            }
            KeyEvent {
                code: KeyCode::Char('v'),
                kind: KeyEventKind::Press,
                ..
            } => {
                self.resources.state.write().await.mode = VimMode::Visual;
                self.text_area.set_block(
                    Block::bordered()
                        .title_top(Line::from("Visual").left_aligned())
                        .border_style(Style::new().fg(Color::Rgb(255, 242, 197))),
                );
                self.text_area.start_selection();
                self.prev_action.clear();
                true
            }
            KeyEvent {
                code: KeyCode::Enter,
                kind: KeyEventKind::Press,
                ..
            } => {
                self.prev_action.clear();
                if !self.text_area.is_empty() {
                    self.send_message(event_sender).await
                } else {
                    false
                }
            }
            KeyEvent {
                code: KeyCode::Char('0'..='9'),
                kind: KeyEventKind::Press,
                ..
            } if self.prev_action.is_number() || event.code != KeyCode::Char('0') => {
                self.prev_action.update(event);
                false
            }
            KeyEvent {
                code: KeyCode::Char('0'),
                kind: KeyEventKind::Press,
                ..
            } => {
                self.text_area.move_cursor(CursorMove::Head);
                true
            }
            KeyEvent {
                code: KeyCode::Char('g'),
                kind: KeyEventKind::Press,
                ..
            } if self.prev_action == Action::Char('g') => {
                self.text_area.move_cursor(CursorMove::Top);
                self.prev_action.clear();
                true
            }
            KeyEvent {
                code: KeyCode::Char('g'),
                kind: KeyEventKind::Press,
                ..
            } => {
                self.prev_action.update(event);
                true
            }
            KeyEvent {
                code: KeyCode::Char('G'),
                kind: KeyEventKind::Press,
                ..
            } => {
                self.text_area.move_cursor(CursorMove::Bottom);
                true
            }
            KeyEvent {
                code: KeyCode::Char('d'),
                kind: KeyEventKind::Press,
                ..
            } => {
                if self.prev_action == Action::Char('d') {
                    let position = self.text_area.cursor();
                    self.text_area.move_cursor(CursorMove::Head);
                    self.text_area.delete_line_by_end();
                    self.text_area.move_cursor(CursorMove::Down);
                    self.text_area.delete_newline();
                    self.text_area
                        .move_cursor(CursorMove::Jump(position.0 as u16, position.1 as u16));
                    self.is_line_yank = true;
                    self.prev_action.clear();
                    true
                } else {
                    self.prev_action.update(event);
                    false
                }
            }
            KeyEvent {
                code: KeyCode::Char('y'),
                kind: KeyEventKind::Press,
                ..
            } => {
                if self.prev_action == Action::Char('y') {
                    let position = self.text_area.cursor();
                    self.text_area.move_cursor(CursorMove::Head);
                    self.text_area.start_selection();
                    self.text_area.move_cursor(CursorMove::End);
                    self.text_area.copy();
                    self.text_area
                        .move_cursor(CursorMove::Jump(position.0 as u16, position.1 as u16));
                    self.is_line_yank = true;
                    self.prev_action.clear();
                    true
                } else {
                    self.prev_action.update(event);
                    false
                }
            }
            KeyEvent {
                code: KeyCode::Char('p'),
                kind: KeyEventKind::Press,
                ..
            } => {
                let num = match self.prev_action {
                    Action::Number(num) => num,
                    _ => 1,
                };
                for _ in 0..num {
                    if self.is_line_yank {
                        self.text_area.move_cursor(CursorMove::End);
                        self.text_area.insert_newline();
                    }
                    self.text_area.paste();
                }
                if self.is_line_yank {
                    self.text_area.move_cursor(CursorMove::Head);
                }
                self.prev_action.clear();
                true
            }
            KeyEvent {
                code: KeyCode::Char('$'),
                kind: KeyEventKind::Press,
                ..
            } => {
                self.text_area.move_cursor(CursorMove::End);
                self.prev_action.clear();
                true
            }
            KeyEvent {
                code: KeyCode::Char(':'),
                kind: KeyEventKind::Press,
                ..
            } => {
                self.command_text_area = TextArea::new(vec![":".to_owned()]);
                self.command_text_area.move_cursor(CursorMove::End);
                self.command_text_area
                    .set_yank_text(self.text_area.yank_text());
                self.command_text_area.set_block(
                    Block::bordered()
                        .title_top(Line::from("Command").left_aligned())
                        .border_style(Style::new().fg(Color::Rgb(255, 242, 197))),
                );
                self.resources.state.write().await.mode = VimMode::Command;
                self.prev_action.clear();
                true
            }
            KeyEvent {
                code:
                    KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Char('h')
                    | KeyCode::Char('j')
                    | KeyCode::Char('k')
                    | KeyCode::Char('l')
                    | KeyCode::Char('w')
                    | KeyCode::Char('b'),
                kind: KeyEventKind::Press,
                ..
            } => {
                let position = self.text_area.cursor();
                if let Some('d') | Some('y') = self.prev_action.get_char() {
                    self.text_area.start_selection();
                }
                if let Some(num) = self.prev_action.get_number() {
                    for _ in 0..num {
                        self.text_area
                            .move_cursor(key_to_cursor_move(event.code).unwrap());
                    }
                    if let KeyCode::Char('j') | KeyCode::Char('k') = event.code {
                        self.text_area.move_cursor(CursorMove::End);
                    }
                } else {
                    self.text_area
                        .move_cursor(key_to_cursor_move(event.code).unwrap());
                }
                if let Some('d') = self.prev_action.get_char() {
                    self.text_area.cut();
                    self.is_line_yank = false;
                } else if let Some('y') = self.prev_action.get_char() {
                    self.text_area.copy();
                    self.is_line_yank = false;
                    self.text_area
                        .move_cursor(CursorMove::Jump(position.0 as u16, position.1 as u16));
                }
                self.prev_action.clear();
                true
            }
            _ => false,
        }
    }

    async fn visual_input(&mut self, event: KeyEvent, _event_sender: &EventSender) -> bool {
        match event {
            KeyEvent {
                code: KeyCode::Esc,
                kind: KeyEventKind::Press,
                ..
            } => {
                self.resources.state.write().await.mode = VimMode::Normal;
                self.text_area.set_block(
                    Block::bordered()
                        .title_top(Line::from("Normal").left_aligned())
                        .border_style(Style::new().fg(Color::Rgb(255, 242, 197))),
                );
                self.prev_action.clear();
                self.text_area.cancel_selection();
                true
            }
            KeyEvent {
                code: KeyCode::Char('y'),
                kind: KeyEventKind::Press,
                ..
            } => {
                self.is_line_yank = false;
                self.text_area.copy();
                self.resources.state.write().await.mode = VimMode::Normal;
                self.text_area.set_block(
                    Block::bordered()
                        .title_top(Line::from("Normal").left_aligned())
                        .border_style(Style::new().fg(Color::Rgb(255, 242, 197))),
                );
                true
            }
            KeyEvent {
                code: KeyCode::Char('d'),
                kind: KeyEventKind::Press,
                ..
            } => {
                self.is_line_yank = false;
                self.text_area.cut();
                self.resources.state.write().await.mode = VimMode::Normal;
                self.text_area.set_block(
                    Block::bordered()
                        .title_top(Line::from("Normal").left_aligned())
                        .border_style(Style::new().fg(Color::Rgb(255, 242, 197))),
                );
                true
            }
            KeyEvent {
                code:
                    KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Char('h')
                    | KeyCode::Char('j')
                    | KeyCode::Char('k')
                    | KeyCode::Char('l')
                    | KeyCode::Char('w')
                    | KeyCode::Char('b'),
                kind: KeyEventKind::Press,
                ..
            } => {
                if let Some(num) = self.prev_action.get_number() {
                    for _ in 0..num {
                        self.text_area
                            .move_cursor(key_to_cursor_move(event.code).unwrap());
                    }
                } else {
                    self.text_area
                        .move_cursor(key_to_cursor_move(event.code).unwrap());
                }
                self.prev_action.clear();
                true
            }
            _ => false,
        }
    }
    async fn command_input(&mut self, event: KeyEvent, event_sender: &EventSender) -> bool {
        match event {
            KeyEvent {
                code: KeyCode::Esc,
                kind: KeyEventKind::Press,
                ..
            } => {
                self.command_text_area = TextArea::new(Vec::new());
                self.resources.state.write().await.mode = VimMode::Normal;
                true
            }
            KeyEvent {
                code: KeyCode::Enter,
                kind: KeyEventKind::Press,
                ..
            } => {
                let command = self.command_text_area.lines()[0].clone();

                info!("Entered command: {}", command);

                match command.as_str() {
                    ":q" => {
                        event_sender.send(InteractiveEvent::Quit).await.unwrap();
                    }
                    ":w" => {
                        self.send_message(event_sender).await;
                    }
                    ":wq" | ":qw" => {
                        self.send_message(event_sender).await;
                        event_sender.send(InteractiveEvent::Quit).await.unwrap();
                    }
                    _ => {}
                }
                self.command_text_area = TextArea::new(Vec::new());
                self.resources.state.write().await.mode = VimMode::Normal;
                true
            }
            KeyEvent {
                code:
                    KeyCode::Char(_)
                    | KeyCode::Backspace
                    | KeyCode::Tab
                    | KeyCode::Delete
                    | KeyCode::Insert
                    | KeyCode::Left
                    | KeyCode::Right,
                kind: KeyEventKind::Press,
                ..
            } => {
                let result = self.command_text_area.input(event);
                if self.command_text_area.cursor().0 == 0 {
                    self.command_text_area.move_cursor(CursorMove::Forward);
                }
                result
            }
            _ => false,
        }
    }
    async fn insert_input(&mut self, event: KeyEvent, _event_sender: &EventSender) -> bool {
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
    pub async fn input(&mut self, event: KeyEvent, event_sender: &EventSender) -> bool {
        let mode = self.resources.state.read().await.mode;
        let cursor_before = self.text_area.cursor();
        let text_changed = match mode {
            VimMode::Normal => self.normal_input(event, event_sender).await,
            VimMode::Insert => self.insert_input(event, event_sender).await,
            VimMode::Command => self.command_input(event, event_sender).await,
            VimMode::Visual => self.visual_input(event, event_sender).await,
        };

        let cursor_changed = cursor_before != self.text_area.cursor();

        text_changed | cursor_changed
    }
}

impl Widget for &mut SendMessageWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.text_area
            .set_line_number_style(Style::new().fg(Color::Blue));

        if self.command_text_area.is_empty() {
            self.text_area.render(area, buf);
        } else {
            let layout = Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]);
            let [text_area, command_area] = layout.areas(area);

            self.text_area.render(text_area, buf);
            self.command_text_area.render(command_area, buf);
        }
    }
}
