use std::sync::Arc;

use futures::channel::mpsc;

use crate::core::Core;
use crate::message::Message;
use crate::runtime::{spawn, JoinHandle};
use crate::{Config, Network, NodeInfo, Result, Storage};

pub struct Raft<D> {
    tx: mpsc::Sender<Message<D>>,
    join_handle: JoinHandle<()>,
}

impl<D> Raft<D> {
    pub fn new<N>(
        name: impl Into<String>,
        node: NodeInfo<N>,
        config: Arc<Config>,
        storage: Arc<dyn Storage<D>>,
        network: Arc<dyn Network<N, D>>,
    ) -> Result<Self> {
        let (core, tx) = Core::new(name, node, config, storage, network)?;
        let join_handle = spawn(async move {
            if let Err(err) = core.await {
                error!(
                    target: "tdb-raft",
                    error = %err,
                    "Raft error",
                );
            }
        });
        Ok(Self { tx, join_handle })
    }
}
