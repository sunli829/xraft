use thiserror::Error;

use crate::NodeId;

#[derive(Debug, Error)]
pub enum RaftError {
    #[error("Storage error: {0}")]
    Storage(anyhow::Error),

    #[error("Fatal error: {0}")]
    Fatal(anyhow::Error),

    #[error("Shutdown")]
    Shutdown,
}

pub type Result<T, E = RaftError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Raft error: {0}")]
    RaftError(#[from] RaftError),

    #[error("Forward to leader: {0:?}")]
    ForwardToLeader(Option<NodeId>),
}
