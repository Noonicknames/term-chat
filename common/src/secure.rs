use std::{io, marker::PhantomData, task::Poll};

use crate::codec::CompressedCborStream;
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng, Payload},
};
use futures::{Sink, SinkExt, Stream, StreamExt};
use hkdf::Hkdf;
use p521::{PublicKey, ecdh::EphemeralSecret};
use rand::TryRngCore;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::Sha512;
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Handshake { public_key: PublicKey },
    Encrypted { data: Vec<u8>, nonce: [u8; 12] },
}

#[derive(thiserror::Error, Debug)]
pub enum SecureStreamError {
    #[error("Expected handshake, received: {:?}", message_received)]
    ExpectedHandshake { message_received: Message },
    #[error("Already handshaked, received: {:?}", handshake_message)]
    AlreadyHandshaked { handshake_message: Message },
    #[error("Failed to decrypt message.")]
    FailedDecryption { bytes: Vec<u8> },
    #[error("Failed to encrypt message.")]
    FailedEncryption { bytes: Vec<u8> },
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub struct SecureStream<S, Item>
where
    S: AsyncRead + AsyncWrite + Unpin,
    Item: Serialize + DeserializeOwned,
{
    inner: CompressedCborStream<S, Message>,
    aes: Aes256Gcm,
    _phantom: PhantomData<Item>,
}

impl<S, Item> SecureStream<S, Item>
where
    S: AsyncRead + AsyncWrite + Unpin,
    Item: Serialize + DeserializeOwned,
{
    pub async fn handshake(inner: S) -> Result<Self, SecureStreamError> {
        let mut inner = CompressedCborStream::new(inner);
        let secret = EphemeralSecret::random(&mut OsRng);

        inner
            .send(Message::Handshake {
                public_key: secret.public_key(),
            })
            .await?;

        let shared_secret = match inner.next().await {
            Some(Ok(Message::Handshake { public_key })) => secret.diffie_hellman(&public_key),
            Some(Ok(message)) => {
                return Err(SecureStreamError::ExpectedHandshake {
                    message_received: message,
                });
            }
            Some(Err(err)) => return Err(err.into()),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "Closed before sending handshake back.",
                )
                .into());
            }
        };

        let hk = Hkdf::<Sha512>::new(None, shared_secret.raw_secret_bytes());
        let mut key_bytes = [0u8; 32];
        hk.expand(b"handshake context", &mut key_bytes).unwrap();

        let aes = Aes256Gcm::new_from_slice(&key_bytes).unwrap();

        Ok(Self {
            inner,
            aes,
            _phantom: PhantomData,
        })
    }
}

impl<S, Item> Stream for SecureStream<S, Item>
where
    S: AsyncRead + AsyncWrite + Unpin,
    Item: Serialize + DeserializeOwned,
{
    type Item = Result<Item, SecureStreamError>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }.poll_next(cx) {
            Poll::Ready(Some(msg)) => {
                let msg = msg?;
                match msg {
                    Message::Handshake { .. } => {
                        return Poll::Ready(Some(Err(io::Error::other(
                            SecureStreamError::AlreadyHandshaked {
                                handshake_message: msg,
                            },
                        )
                        .into())));
                    }
                    Message::Encrypted { data, nonce } => {
                        let Ok(message) = self.aes.decrypt(
                            Nonce::from_slice(&nonce),
                            Payload {
                                msg: &data,
                                aad: b"",
                            },
                        ) else {
                            return Poll::Ready(Some(Err(SecureStreamError::FailedDecryption {
                                bytes: data,
                            })));
                        };
                        let item = serde_cbor::de::from_slice(&message)
                            .map_err(|err| std::io::Error::new(io::ErrorKind::InvalidData, err))?;

                        return Poll::Ready(Some(Ok(item)));
                    }
                }
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S, Item> Sink<Item> for SecureStream<S, Item>
where
    S: AsyncRead + AsyncWrite + Unpin,
    Item: Serialize + DeserializeOwned,
{
    type Error = SecureStreamError;
    fn poll_ready(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }
            .poll_ready(cx)
            .map_err(Into::into)
    }
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }
            .poll_close(cx)
            .map_err(Into::into)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }
            .poll_flush(cx)
            .map_err(Into::into)
    }
    fn start_send(mut self: std::pin::Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        let bytes = serde_cbor::ser::to_vec(&item).map_err(std::io::Error::other)?;

        let mut nonce = [0u8; 12];
        rand::rngs::OsRng.try_fill_bytes(&mut nonce).unwrap();

        let encrypted_bytes = self
            .aes
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &bytes,
                    aad: b"",
                },
            )
            .map_err(|_| SecureStreamError::FailedEncryption { bytes })?;

        let message = Message::Encrypted {
            nonce,
            data: encrypted_bytes,
        };

        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }
            .start_send(message)
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod test {
    use futures::{SinkExt, StreamExt};
    use serde::{Deserialize, Serialize};

    use crate::secure::SecureStream;

    #[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
    struct TestStruct {
        string: String,
        number: u32,
        void: (),
    }

    #[test]
    fn test_handshake() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();

        let test_enum = TestStruct {
            string: "Bro".to_owned(),
            number: 69,
            void: (),
        };
        let client = {
            let test_enum = test_enum.clone();
            rt.spawn(async move {
                let stream = tokio::net::TcpStream::connect("localhost:1234")
                    .await
                    .unwrap();

                let stream = SecureStream::handshake(stream).await.unwrap();

                let (mut send, mut recv) = stream.split();

                send.send(test_enum.clone()).await.unwrap();

                assert_eq!(
                    recv.next().await.transpose().unwrap(),
                    Some(test_enum.clone())
                );
            })
        };

        let server = {
            rt.spawn(async move {
                let test_enum = test_enum.clone();
                let listener = tokio::net::TcpListener::bind("localhost:1234")
                    .await
                    .unwrap();

                let (stream, _) = listener.accept().await.unwrap();

                let stream = SecureStream::handshake(stream).await.unwrap();

                let (mut send, mut recv) = stream.split();

                send.send(test_enum.clone()).await.unwrap();

                assert_eq!(
                    recv.next().await.transpose().unwrap(),
                    Some(test_enum.clone())
                );
            })
        };

        rt.block_on(async {
            client.await.unwrap();
            server.await.unwrap();
        });
    }
}
