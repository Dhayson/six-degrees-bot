use crate::client_utils::*;
use nostr_sdk::prelude::*;

#[derive(Debug, Clone)]
pub struct User {
    public_key: PublicKey,
    metadata: Metadata,
    last_updated: Timestamp,
}

#[derive(Debug)]
pub enum CreateUserError {
    MetadataNotFound,
    GetMetadataClientError(Error),
}

impl std::fmt::Display for CreateUserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", "User metadata not found")
    }
}
impl std::error::Error for CreateUserError {}

impl User {
    pub async fn new(public_key: PublicKey, client: &Client) -> Result<User, CreateUserError> {
        let mut meta = match get_metadata_users(&[public_key], &client).await {
            Ok(ok) => ok,
            Err(err) => return Err(CreateUserError::GetMetadataClientError(err)),
        };
        let (metadata, timestamp) = match meta.remove(&public_key).unwrap() {
            Some((m, t)) => (m, t),
            None => return Err(CreateUserError::MetadataNotFound).into(),
        };

        Ok(User {
            public_key,
            metadata,
            last_updated: timestamp,
        })
    }

    pub fn public_key(&self) -> PublicKey {
        self.public_key
    }

    pub fn metadata(&self) -> Metadata {
        self.metadata.clone()
    }

    pub fn last_updated(&self) -> Timestamp {
        self.last_updated
    }
}
