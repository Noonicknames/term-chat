use std::sync::Arc;

use common::{ClientId, ClientMessage, ServerMessage};
use crossterm::event::{MouseEvent, MouseEventKind};
use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Block, HighlightSpacing, List, ListItem, ListState, StatefulWidget, Widget},
};

use tokio_util::bytes::Bytes;

use crate::{
    CommandArgs,
    app::{
        event::{Event, EventSender, EventStream, InteractiveEvent, TermEvent},
        resources::AppResources,
        vim::SendMessageWidget,
    },
};

pub mod event;
pub mod resources;
pub mod vim;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("An issue occurred communicating with the server.")]
    ServerError,
    #[error("No valid ports were found")]
    NoValidPorts,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    JoinError(#[from] tokio::task::JoinError),
}

pub async fn run_app(args: CommandArgs) -> Result<(), AppError> {
    let CommandArgs { name } = args;
    let resources = Arc::new(AppResources::new(name).await?);

    let mut app = App::new(resources).await?;

    app.run().await?;

    Ok(())
}

pub struct App {
    resources: Arc<AppResources>,
    messages: MessageListWidget,
    send_message: SendMessageWidget,
}

impl App {
    pub async fn new(resources: Arc<AppResources>) -> Result<Self, AppError> {
        Ok(Self {
            messages: MessageListWidget::new(),
            send_message: SendMessageWidget::new(Arc::clone(&resources)),
            resources,
        })
    }
    pub async fn run(&mut self) -> Result<(), AppError> {
        let mut terminal = ratatui::init();
        let event_stream = EventStream::new();

        let event_sender = event_stream.event_sender().clone();
        let resources = Arc::clone(&self.resources);

        tokio::select! {
            res = self.interactive_loop(&resources, &mut terminal, event_stream) => {
                res
            }
            res = tokio::task::spawn(Self::network_loop(Arc::clone(&resources), event_sender)) => {
                res?
            }
        }
    }

    pub async fn network_loop(
        resources: Arc<AppResources>,
        event_sender: EventSender,
    ) -> Result<(), AppError> {
        // Connect to server
        while let Some(Ok(message)) = resources.read_msg.lock().await.next().await {
            let message: ServerMessage = match serde_cbor::de::from_slice(&message) {
                Ok(message) => message,
                Err(err) => {
                    warn!("Received a corrupted message from server: {}", err);
                    continue;
                }
            };

            match message {
                ServerMessage::AcceptJoin => {
                    info!("Server accepted your join request.")
                }
                ServerMessage::ReceiveMessage { message, sender } => {
                    event_sender
                        .send(InteractiveEvent::ReceiveMessage {
                            sender,
                            content: message,
                        })
                        .await
                        .unwrap();
                }
            }
        }

        Ok(())
    }

    pub async fn interactive_loop(
        &mut self,
        resources: &Arc<AppResources>,
        terminal: &mut DefaultTerminal,
        mut event_stream: EventStream,
    ) -> Result<(), AppError> {
        let exit_result = loop {
            match event_stream.next().await {
                Some(Ok(event)) => {
                    match self
                        .handle_event(resources, event, event_stream.event_sender(), terminal)
                        .await
                    {
                        Ok(false) => (),
                        Ok(true) => {
                            break Ok(());
                        }
                        Err(err) => {
                            break Err(err);
                        }
                    }
                }
                Some(Err(err)) => {
                    error!("Error in interactive loop: {}", err)
                }
                None => (),
            }
        };

        ratatui::restore();

        self.on_exit(resources).await;

        exit_result
    }

    pub async fn on_exit(&mut self, _resources: &Arc<AppResources>) {}

    fn render(&mut self, frame: &mut Frame) {
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(6),
        ]);
        let [title_area, messages_area, send_area] = layout.areas(frame.area());
        let title = Line::from("term-chat")
            .centered()
            .bold()
            .fg(Color::Rgb(255, 242, 197));
        frame.render_widget(title, title_area);
        frame.render_widget(&mut self.messages, messages_area);
        frame.render_widget(&mut self.send_message, send_area);
    }

    async fn handle_event(
        &mut self,
        resources: &Arc<AppResources>,
        event: Event,
        event_sender: &EventSender,
        terminal: &mut DefaultTerminal,
    ) -> Result<bool, AppError> {
        match event {
            Event::Interactive(event) => {
                self.handle_interactive_event(resources, event, event_sender, terminal)
                    .await
            }
            Event::Term(event) => self.handle_term_event(event, event_sender, terminal).await,
        }
    }

    async fn handle_interactive_event(
        &mut self,
        resources: &Arc<AppResources>,
        event: InteractiveEvent,
        event_sender: &EventSender,
        terminal: &mut DefaultTerminal,
    ) -> Result<bool, AppError> {
        match event {
            InteractiveEvent::Quit => Ok(true),
            InteractiveEvent::RedrawRequest => {
                // terminal.swap_buffers();
                terminal.draw(|frame| self.render(frame)).unwrap();
                Ok(false)
            }
            InteractiveEvent::ReceiveMessage { sender, content } => {
                self.messages.messages.push(Message {
                    id: sender,
                    content,
                });
                event_sender
                    .send(InteractiveEvent::RedrawRequest)
                    .await
                    .unwrap();
                Ok(false)
            }
            InteractiveEvent::SendMessage { content } => {
                let resources = Arc::clone(resources);
                tokio::spawn(async move {
                    let message =
                        serde_cbor::to_vec(&ClientMessage::SendMessage { message: content })
                            .unwrap();

                    let mut write_msg = resources.write_msg.lock().await;
                    if let Err(err) = write_msg.send(Bytes::from(message)).await {
                        error!("Error writing to server: {}", err);
                    }
                });
                Ok(false)
            }
        }
    }

    async fn handle_term_event(
        &mut self,
        event: TermEvent,
        event_sender: &EventSender,
        _terminal: &mut DefaultTerminal,
    ) -> Result<bool, AppError> {
        if let TermEvent::Key(event) = event {
            if self.send_message.input(event, event_sender).await {
                event_sender
                    .send(InteractiveEvent::RedrawRequest)
                    .await
                    .unwrap();
            }
        }
        match event {
            TermEvent::FocusGained | TermEvent::Resize(_, _) => {
                event_sender
                    .send(InteractiveEvent::RedrawRequest)
                    .await
                    .unwrap();
                Ok(false)
            }
            TermEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                ..
            }) => {
                self.messages.scroll_up();
                event_sender
                    .send(InteractiveEvent::RedrawRequest)
                    .await
                    .unwrap();
                Ok(false)
            }
            TermEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                ..
            }) => {
                info!("We are scrolling down right now");
                self.messages.scroll_down();
                event_sender
                    .send(InteractiveEvent::RedrawRequest)
                    .await
                    .unwrap();
                Ok(false)
            }
            _ => Ok(false),
        }
    }
}

struct MessageListWidget {
    messages: Vec<Message>,
    list_state: ListState,
}

impl MessageListWidget {
    fn new() -> Self {
        Self {
            messages: vec![],
            list_state: ListState::default(),
        }
    }
    fn scroll_up(&mut self) {
        self.list_state.scroll_up_by(1);
    }
    fn scroll_down(&mut self) {
        self.list_state.scroll_down_by(1);
    }
}

#[derive(Debug, Clone)]
struct Message {
    id: ClientId,
    content: String,
}

impl From<&'_ Message> for ListItem<'_> {
    fn from(value: &'_ Message) -> Self {
        ListItem::new(format!("{}: {}", value.id, value.content))
    }
}

impl Widget for &mut MessageListWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // a block with a right aligned title with the loading state on the right
        let block = Block::bordered()
            .border_style(Style::new().fg(Color::Rgb(255, 242, 197)))
            .title("Messages")
            .title_bottom(Line::from("Esc to quit").right_aligned());

        // a table with the list of pull requests
        let items = self.messages.iter();
        let list = List::new(items)
            .block(block)
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_symbol(">")
            .highlight_style(Style::new().on_blue());

        StatefulWidget::render(list, area, buf, &mut self.list_state);
    }
}
