use thiserror::Error;

use crate::NodeId;

#[derive(Debug, Error)]
pub enum RaftError {
    #[error("Storage error: {0}")]
    Storage(anyhow::Error),

    #[error("This Raft has already been initialized.")]
    AlreadyInitialized,

    #[error("Forward to leader: {0:?}")]
    ForwardToLeader(Option<NodeId>),

    #[error("Shutdown")]
    Shutdown,
}

pub type Result<T, E = RaftError> = std::result::Result<T, E>;
