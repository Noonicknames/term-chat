use futures::stream::{SplitSink, SplitStream};
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

pub struct SecureStream {
    /// Sends raw data.
    raw: SplitStream<Framed<TcpStream, LengthDelimitedCodec>>,
    has_handshaked: bool,
}

pub struct SecureSink {
    /// Receives raw data.
    raw: SplitSink<Framed<TcpStream, LengthDelimitedCodec>, tokio_util::bytes::Bytes>,
    has_handshaked: bool,
}
