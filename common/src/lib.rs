use std::{fmt::Display, net::SocketAddr};

use bytes::Bytes;
use futures::stream::{SplitSink, SplitStream};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;

use crate::secure::SecureStream;

pub mod secure;
pub mod codec;

pub type ReadStream = SplitStream<SecureStream<TcpStream, Bytes>>;
pub type WriteSink = SplitSink<SecureStream<TcpStream, Bytes>, Bytes>;

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
    ClientListUpdate {
        clients: Vec<ClientId>,
    },
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