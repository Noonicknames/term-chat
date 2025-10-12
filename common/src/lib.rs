use std::{fmt::Display, net::SocketAddr};

use futures::{
    StreamExt,
    stream::{SplitSink, SplitStream},
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};


pub type ReadStream = SplitStream<Framed<TcpStream, LengthDelimitedCodec>>;
pub type WriteStream = SplitSink<Framed<TcpStream, LengthDelimitedCodec>, tokio_util::bytes::Bytes>;

pub fn split_message_stream(stream: TcpStream) -> (WriteStream, ReadStream) {
    Framed::new(stream, LengthDelimitedCodec::new()).split()
}

/// Message coming from the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    JoinRequest {
        name: String,
    },
    /// Ask the server to broadcast a message for you.
    SendMessage {
        message: String,
    },
}

/// Message coming from the server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServerMessage {
    AcceptJoin,
    /// Client receives a messsage.
    ReceiveMessage {
        sender: ClientId,
        message: String,
    },
}

#[derive(Hash, PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
pub struct ClientId {
    pub name: String,
    pub addr: SocketAddr,
}

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.addr)
    }
}
