use std::{net::SocketAddr, sync::Arc};

use common::{ClientId, ClientMessage, ServerMessage, WriteSink, split_message_stream};
use futures::{SinkExt, StreamExt, stream::FuturesUnordered};
use log::{error, info, warn};
use papaya::HashMap;
use serde::{Deserialize, Serialize};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tokio_util::bytes::Bytes;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    JoinError(#[from] tokio::task::JoinError),
}

pub struct Client {
    id: ClientId,
    write_msg: Mutex<WriteSink>,
}

#[derive(Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSettings {
    pub listen_addresses: Vec<SocketAddr>,
    pub max_concurrency: usize,
    pub max_message_buffer_size: usize,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            listen_addresses: vec!["0.0.0.0:6942".parse().unwrap()],
            max_concurrency: 128,
            max_message_buffer_size: 2048,
        }
    }
}

pub struct Server {
    clients: HashMap<ClientId, Arc<Client>>,

    settings: ServerSettings,
}

impl Server {
    pub async fn new(settings: ServerSettings) -> Result<Self, ServerError> {
        let clients = HashMap::new();

        Ok(Self { clients, settings })
    }
    pub async fn run_loop(self: &Arc<Self>) -> Result<(), ServerError> {
        info!("Started server!");
        let mut futures = FuturesUnordered::new();
        let mut listeners = Vec::new();

        for address in self.settings.listen_addresses.iter() {
            listeners.push(TcpListener::bind(address).await?);
        }
        for listener in listeners {
            let this = Arc::clone(self);
            futures.push(tokio::spawn(async move {
                let mut futures = FuturesUnordered::new();
                loop {
                    if let Ok((stream, addr)) = listener.accept().await {
                        futures.push(tokio::spawn(
                            Arc::clone(&this).handle_new_connection(stream, addr),
                        ));

                        // Enforce max concurrency
                        while futures.len() >= this.settings.max_concurrency {
                            futures.next().await;
                        }
                    }
                }
            }));
        }

        futures.next().await;
        Ok(())
    }

    pub async fn handle_new_connection(self: Arc<Self>, stream: TcpStream, addr: SocketAddr) {
        let (write_msg, mut read_msg) = split_message_stream(stream);

        info!("Accepted {}", addr);

        let client_id = loop {
            let message = match read_msg.next().await {
                Some(Ok(message)) => message,
                Some(Err(err)) => {
                    error!("Error deserialising message from {}: {}", addr, err);
                    return;
                }
                None => {
                    error!(
                        "Error deserialising message from {}: Didn't receive any messages.",
                        addr
                    );
                    return;
                }
            };
            let message: ClientMessage = match serde_cbor::from_slice(&message) {
                Ok(message) => message,
                Err(err) => {
                    error!("Error deserialising message from {}: {}", addr, err);
                    return;
                }
            };
            match message {
                ClientMessage::JoinRequest { name } => {
                    let client_id = ClientId { name, addr };

                    let client = Client {
                        id: client_id.clone(),
                        write_msg: Mutex::new(write_msg),
                    };

                    let clients = self.clients.pin_owned();
                    clients.insert(client_id.clone(), Arc::new(client));

                    let response = serde_cbor::ser::to_vec(&ServerMessage::AcceptJoin).unwrap();

                    if let Err(err) = clients
                        .get(&client_id)
                        .unwrap()
                        .write_msg
                        .lock()
                        .await
                        .send(Bytes::from(response))
                        .await
                    {
                        error!("Error writing to client {}: {}", client_id, err)
                    }
                    break client_id;
                }
                message => {
                    warn!(
                        "Message ignored since client has not joined yet: {:?}",
                        message
                    );
                }
            }
        };

        let message = ServerMessage::ClientListUpdate {
            clients: self.clients.pin_owned().keys().cloned().collect(),
        };
        let message = match serde_cbor::to_vec(&message) {
            Ok(message) => Bytes::from(message),
            Err(err) => {
                error!("Error deserialising message: {}", err);
                return;
            }
        };

        let this = Arc::clone(&self);
        tokio::task::spawn(async move {
            this.broadcast(&message).await;
        });

        loop {
            let message = match read_msg.next().await {
                Some(Ok(message)) => message,
                Some(Err(err)) => {
                    error!("Error deserialising message from {}: {}", addr, err);
                    continue;
                }
                None => {
                    error!("Error deserialising message from {}", addr);
                    break;
                }
            };
            let message: ClientMessage = match serde_cbor::from_slice(&message) {
                Ok(message) => message,
                Err(err) => {
                    error!("Error deserialising message from {}: {}", addr, err);
                    continue;
                }
            };
            match message {
                ClientMessage::JoinRequest { name: _ } => {
                    warn!("Client {} has already joined", client_id);
                }
                ClientMessage::SendMessage { message } => {
                    info!("Client {} sent message: {:?}", client_id, message);

                    let message = ServerMessage::ReceiveMessage {
                        sender: client_id.clone(),
                        message,
                    };
                    let message = match serde_cbor::to_vec(&message) {
                        Ok(message) => Bytes::from(message),
                        Err(err) => {
                            error!("Error deserialising message from {}: {}", client_id, err);
                            continue;
                        }
                    };
                    let this = Arc::clone(&self);
                    tokio::task::spawn(async move {
                        this.broadcast(&message).await;
                    });
                }
            }
        }
        self.clients.pin().remove(&client_id);
        info!("{} has been removed from clients list.", client_id);

        let message = ServerMessage::ClientListUpdate {
            clients: self.clients.pin_owned().keys().cloned().collect(),
        };
        let message = match serde_cbor::to_vec(&message) {
            Ok(message) => Bytes::from(message),
            Err(err) => {
                error!("Error deserialising message: {}", err);
                return;
            }
        };
        let this = Arc::clone(&self);
        tokio::task::spawn(async move {
            this.broadcast(&message).await;
        });
    }

    pub async fn broadcast(self: &Arc<Self>, message: &Bytes) {
        let mut futures = FuturesUnordered::new();

        let mut client_vec = Vec::new();
        for (_id, client) in self.clients.pin().iter() {
            client_vec.push(Arc::clone(client));
        }

        for client in client_vec {
            let message = message.clone();
            futures.push(async move {
                if let Err(err) = client.write_msg.lock().await.send(message).await {
                    error!("Error sending message to {}: {}", client.id, err);
                }
            })
        }

        while let Some(()) = futures.next().await {}
    }
}
