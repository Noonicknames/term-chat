use std::{fmt::Display, net::SocketAddr, sync::Arc, time::SystemTime};

use common::ClientMessage;
use futures::{StreamExt, stream::FuturesUnordered};
use log::{error, info};
use papaya::HashMap;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream, ToSocketAddrs},
    runtime,
};

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub struct ClientId {
    pub name: String,
    pub addr: SocketAddr,
}

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.addr)
    }
}

pub struct Client {
    id: ClientId,
    stream: TcpStream,
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct ServerSettings {
    pub max_concurrency: usize,
    pub max_message_buffer_size: usize,
}

pub struct Server {
    listener: TcpListener,
    clients: HashMap<ClientId, Client>,

    settings: ServerSettings,
}

impl Server {
    pub async fn new(
        listen_addr: impl ToSocketAddrs,
        settings: ServerSettings,
    ) -> Result<Self, ServerError> {
        let clients = HashMap::new();

        let listener = TcpListener::bind(listen_addr).await?;

        Ok(Self {
            clients,
            listener,
            settings,
        })
    }
    pub async fn run_loop(self: &Arc<Self>) -> Result<(), ServerError> {
        let mut futures = FuturesUnordered::new();

        loop {
            if let Ok((stream, addr)) = self.listener.accept().await {
                futures.push(tokio::spawn(
                    Arc::clone(self).handle_new_connection(stream, addr),
                ));

                // Enforce max concurrency
                if futures.len() >= self.settings.max_concurrency {
                    futures.next().await;
                }
            }
        }
    }

    pub async fn handle_new_connection(self: Arc<Self>, mut stream: TcpStream, addr: SocketAddr) {
        let mut message = vec![0u8; self.settings.max_message_buffer_size];
        info!("Accepted {}", addr);
        let message_len = match stream.read(&mut message).await {
            Ok(message_len) => message_len,
            Err(err) => {
                error!("Error reading message from {}: {}", addr, err);
                return;
            }
        };

        let message: ClientMessage = match serde_cbor::from_slice(&message[0..message_len]) {
            Ok(message) => message,
            Err(err) => {
                error!("Error deserialising message from {}: {}", addr, err);
                return;
            }
        };

        info!("Got message: {:?}", message);
        match message {
            ClientMessage::JoinRequest { name } => {
                let client_id = ClientId { name, addr };

                let client = Client {
                    id: client_id.clone(),
                    stream,
                };

                let clients_pin = self.clients.pin();
                clients_pin.insert(client_id, client);
            }
            ClientMessage::SendMessage { name, message } => {
                let client_id = ClientId { name, addr };
                info!("Client {} sent message: {:?}", client_id, message);
            }
        }
    }
}
