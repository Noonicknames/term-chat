use common::{split_message_stream, ClientId, ClientMessage, ReadStream, ServerMessage, WriteStream};
use futures::{SinkExt, StreamExt};
use log::info;
use tokio::{net::TcpSocket, sync::{Mutex, RwLock}};
use tokio_util::bytes::Bytes;

use crate::app::{vim::VimMode, AppError};

#[derive(Debug, Default)]
pub struct AppState {
    pub mode: VimMode,
}


pub struct AppResources {
    pub id: ClientId,
    pub read_msg: Mutex<ReadStream>,
    pub write_msg: Mutex<WriteStream>,
    pub state: RwLock<AppState>,
}

impl AppResources {
    pub async fn new(name: String) -> Result<Self, AppError> {
        let socket = TcpSocket::new_v4()?;

        let Some(server_addr) = tokio::net::lookup_host("www.banhana.org:6942")
            .await
            .unwrap()
            .next()
        else {
            return Err(AppError::ServerError);
        };

        info!("Resolved server socket address: {}", server_addr);

        let stream = socket.connect(server_addr).await?;

        let id = ClientId {
            name: name.clone(),
            addr: stream.local_addr().unwrap(),
        };

        let (mut write_msg, mut read_msg) = split_message_stream(stream);

        let buf = serde_cbor::to_vec(&ClientMessage::JoinRequest { name }).unwrap();

        write_msg.send(Bytes::from(buf)).await?;

        let Some(Ok(response)) = read_msg.next().await else {
            return Err(AppError::ServerError);
        };

        let response: ServerMessage = serde_cbor::de::from_slice(&response).unwrap();

        if response != ServerMessage::AcceptJoin {
            return Err(AppError::ServerError);
        }

        let read_msg = Mutex::new(read_msg);
        let write_msg = Mutex::new(write_msg);

        let state = RwLock::new(AppState::default());

        Ok(Self {
            id,
            read_msg,
            write_msg,
            state,
        })
    }
}
