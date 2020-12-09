use std::sync::Arc;

use futures::channel::{mpsc, oneshot};
use futures::future::Shared;
use futures::{FutureExt, SinkExt};

use crate::core::Core;
use crate::message::Message;
use crate::runtime::{spawn, JoinHandle};
use crate::{
    AppendEntriesRequest, AppendEntriesResponse, Config, Metrics, Network, NodeId, RaftError,
    Result, Storage, VoteRequest, VoteResponse,
};

pub struct Raft<N, D> {
    tx: mpsc::Sender<Message<N, D>>,
    join_handle: Shared<JoinHandle<RaftError>>,
}

impl<N, D> Clone for Raft<N, D> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            join_handle: self.join_handle.clone(),
        }
    }
}

impl<N, D> Raft<N, D>
where
    N: Send + Sync + Unpin + 'static,
    D: Send + Unpin + 'static,
{
    pub fn new(
        name: impl Into<String>,
        id: NodeId,
        config: Arc<Config>,
        storage: Arc<dyn Storage<N, D>>,
        network: Arc<dyn Network<N, D>>,
    ) -> Result<Self> {
        let (core, tx) = Core::new(name, id, config, storage, network)?;
        let join_handle = spawn(async move {
            let err = core.await;
            if !err.is_shutdown() {
                error!(
                    error = %err,
                    "Raft error",
                );
            } else {
                debug!(id = id, "Node shutdown");
            }
            err
        })
        .shared();
        Ok(Self { tx, join_handle })
    }

    pub async fn wait_for_end(&self) -> Result<()> {
        match self.join_handle.clone().await {
            RaftError::Shutdown => Ok(()),
            err => Err(err),
        }
    }

    pub async fn initialize(&self, members: impl IntoIterator<Item = (NodeId, N)>) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(Message::Initialize {
                members: members.into_iter().collect(),
                reply: tx,
            })
            .await
            .map_err(|_| RaftError::Shutdown)?;
        rx.await.map_err(|_| RaftError::Shutdown)?
    }

    pub async fn append_entries(
        &self,
        req: AppendEntriesRequest<N, D>,
    ) -> Result<AppendEntriesResponse> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(Message::AppendEntries { req, reply: tx })
            .await
            .map_err(|_| RaftError::Shutdown)?;
        rx.await.map_err(|_| RaftError::Shutdown)?
    }

    pub async fn vote(&self, req: VoteRequest) -> Result<VoteResponse> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(Message::Vote { req, reply: tx })
            .await
            .map_err(|_| RaftError::Shutdown)?;
        rx.await.map_err(|_| RaftError::Shutdown)?
    }

    pub async fn client_read(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(Message::ClientRead { reply: tx })
            .await
            .map_err(|_| RaftError::Shutdown)?;
        rx.await.map_err(|_| RaftError::Shutdown)?
    }

    pub async fn client_write(&self, action: D) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(Message::ClientWrite { action, reply: tx })
            .await
            .map_err(|_| RaftError::Shutdown)?;
        rx.await.map_err(|_| RaftError::Shutdown)?
    }

    pub async fn add_non_voter(&self, id: NodeId, info: N) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(Message::AddNode {
                id,
                info,
                reply: tx,
            })
            .await
            .map_err(|_| RaftError::Shutdown)?;
        rx.await.map_err(|_| RaftError::Shutdown)?
    }

    pub async fn remove_node(&self, id: NodeId) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(Message::RemoveNode { id, reply: tx })
            .await
            .map_err(|_| RaftError::Shutdown)?;
        rx.await.map_err(|_| RaftError::Shutdown)?
    }

    pub async fn metrics(&self) -> Result<Metrics<N>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(Message::Metrics { reply: tx })
            .await
            .map_err(|_| RaftError::Shutdown)?;
        rx.await.map_err(|_| RaftError::Shutdown)?
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.tx
            .send(Message::Shutdown)
            .await
            .map_err(|_| RaftError::Shutdown)?;
        let _ = self.join_handle.await;
        Ok(())
    }
}
