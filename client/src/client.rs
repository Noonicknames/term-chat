use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct Client {}

impl Client {
    pub async fn new() -> Result<Self, ClientError> {
        Ok(Self {})
    }

    pub async fn run(self: &Arc<Self>) -> Result<Self, ClientError> {
        loop {

        }
    }
}
