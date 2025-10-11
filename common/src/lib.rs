use serde::{Deserialize, Serialize};

/// Message coming from the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    JoinRequest {
        name: String,
    },
    /// Ask the server to broadcast a message for you.
    SendMessage {
        name: String,
        message: String,
    },
}

/// Message coming from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Client receives a messsage.
    ReceiveMessage {
        message: String,
    }
}