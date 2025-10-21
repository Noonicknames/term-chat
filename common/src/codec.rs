use std::{marker::PhantomData, task::Poll};

use futures::{Sink, Stream};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::{
    bytes::Bytes,
    codec::{Framed, LengthDelimitedCodec},
};

pub struct CompressedCborStream<S, Item>
where
    S: AsyncWrite + AsyncRead,
    Item: DeserializeOwned + Serialize,
{
    inner: Framed<S, tokio_util::codec::LengthDelimitedCodec>,
    _phantom: PhantomData<Item>,
}

impl<S, Item> CompressedCborStream<S, Item>
where
    S: AsyncWrite + AsyncRead,
    Item: DeserializeOwned + Serialize,
{
    pub fn new(inner: S) -> Self {
        Self {
            inner: Framed::new(inner, LengthDelimitedCodec::new()),
            _phantom: PhantomData,
        }
    }
}

impl<S, Item> Stream for CompressedCborStream<S, Item>
where
    S: AsyncWrite + AsyncRead,
    Item: DeserializeOwned + Serialize,
{
    type Item = std::io::Result<Item>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }.poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(bytes)) => {
                let bytes = bytes?;
                let uncompressed_bytes = lz4::block::decompress(&bytes, None)?;
                Poll::Ready(Some(Ok(
                    serde_cbor::from_slice(&uncompressed_bytes).map_err(std::io::Error::other)?
                )))
            }
        }
    }
}

impl<S, Item> Sink<Item> for CompressedCborStream<S, Item>
where
    S: AsyncWrite + AsyncRead,
    Item: DeserializeOwned + Serialize,
{
    type Error = std::io::Error;
    fn poll_ready(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }.poll_ready(cx)
    }
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }.poll_close(cx)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }.poll_flush(cx)
    }
    fn start_send(mut self: std::pin::Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        let bytes = serde_cbor::ser::to_vec(&item).map_err(std::io::Error::other)?;
        let compressed_bytes =
            lz4::block::compress(&bytes, Some(lz4::block::CompressionMode::DEFAULT), true)?;
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }
            .start_send(Bytes::from_owner(compressed_bytes))
    }
}

pub struct CborStream<S, Item>
where
    S: AsyncWrite + AsyncRead,
    Item: DeserializeOwned + Serialize,
{
    inner: Framed<S, tokio_util::codec::LengthDelimitedCodec>,
    _phantom: PhantomData<Item>,
}

impl<S, Item> CborStream<S, Item>
where
    S: AsyncWrite + AsyncRead,
    Item: DeserializeOwned + Serialize,
{
    pub fn new(inner: S) -> Self {
        Self {
            inner: Framed::new(inner, LengthDelimitedCodec::new()),
            _phantom: PhantomData,
        }
    }
}

impl<S, Item> Stream for CborStream<S, Item>
where
    S: AsyncWrite + AsyncRead,
    Item: DeserializeOwned + Serialize,
{
    type Item = std::io::Result<Item>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }.poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(bytes)) => {
                let bytes = bytes?;
                Poll::Ready(Some(Ok(
                    serde_cbor::from_slice(&bytes).map_err(std::io::Error::other)?
                )))
            }
        }
    }
}

impl<S, Item> Sink<Item> for CborStream<S, Item>
where
    S: AsyncWrite + AsyncRead,
    Item: DeserializeOwned + Serialize,
{
    type Error = std::io::Error;
    fn poll_ready(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }.poll_ready(cx)
    }
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }.poll_close(cx)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }.poll_flush(cx)
    }
    fn start_send(mut self: std::pin::Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        let bytes = serde_cbor::ser::to_vec(&item).map_err(std::io::Error::other)?;
        unsafe { self.as_mut().map_unchecked_mut(|this| &mut this.inner) }
            .start_send(Bytes::from_owner(bytes))
    }
}

#[cfg(test)]
mod test {
    use futures::{SinkExt, StreamExt};
    use serde::{Deserialize, Serialize};

    use crate::codec::{CborStream, CompressedCborStream};

    #[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
    struct TestEnum {
        string: String,
        number: u32,
        void: (),
    }

        #[test]
    fn test_compressed_cbor_stream() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();

        let test_enum = TestEnum {
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

                let cbor_stream = CompressedCborStream::new(stream);

                let (mut send, mut recv) = cbor_stream.split();

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

                let cbor_stream = CompressedCborStream::new(stream);

                let (mut send, mut recv) = cbor_stream.split();

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

    #[test]
    fn test_cbor_stream() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();

        let test_enum = TestEnum {
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

                let cbor_stream = CborStream::new(stream);

                let (mut send, mut recv) = cbor_stream.split();

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

                let cbor_stream = CborStream::new(stream);

                let (mut send, mut recv) = cbor_stream.split();

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
