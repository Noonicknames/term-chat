use common::ClientId;
pub use crossterm::event::Event as TermEvent;
use crossterm::event::EventStream as TermEventStream;
use futures::Stream;
use futures::StreamExt;
use tokio::sync::mpsc::{Receiver, Sender};

pub type EventSender = Sender<InteractiveEvent>;

pub enum Event {
    Interactive(InteractiveEvent),
    Term(TermEvent),
}

pub enum InteractiveEvent {
    RedrawRequest,
    ClientListUpdate {
        clients: Vec<ClientId>,
    },
    SendMessage {
        content: String,
    },
    ReceiveMessage {
        sender: ClientId,
        content: String,
    },
    Quit,
}

pub struct EventStream {
    interactive_recv: Receiver<InteractiveEvent>,
    interactive_send: EventSender,
    term_stream: TermEventStream,
}

impl EventStream {
    pub fn new() -> Self {
        let term_stream = TermEventStream::new();
        let (interactive_send, interactive_recv) = tokio::sync::mpsc::channel(256);

        Self {
            interactive_recv,
            interactive_send,
            term_stream,
        }
    }

    pub fn event_sender(&self) -> &EventSender {
        &self.interactive_send
    }
}

impl Stream for EventStream {
    type Item = std::io::Result<Event>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let interactive = self.interactive_recv.poll_recv(cx);

        if let std::task::Poll::Ready(Some(item)) = interactive {
            return std::task::Poll::Ready(Some(Ok(Event::Interactive(item))));
        }

        let term = self.term_stream.poll_next_unpin(cx);

        if let std::task::Poll::Ready(Some(item)) = term {
            return std::task::Poll::Ready(Some(item.map(|event| Event::Term(event))));
        }

        if interactive.is_pending() || term.is_pending() {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(None)
        }
    }
}
