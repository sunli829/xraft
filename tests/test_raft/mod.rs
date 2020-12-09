mod mem_storage;
mod test_network;

use std::collections::HashMap;
use std::sync::Arc;

use mem_storage::MemoryStorage;
use parking_lot::RwLock;
use test_network::TestNetwork;
use tracing_subscriber::layer::SubscriberExt;
use xraft::{Config, NodeId, Raft, Result, Storage};

#[derive(Debug, Clone)]
pub enum Action {
    Put(String, i32),
    Delete(String),
}

#[derive(Clone)]
pub struct Member {
    pub raft: Raft<(), Action>,
    pub storage: Arc<dyn Storage<(), Action>>,
}

pub struct TestHarness {
    config: Arc<Config>,
    nodes: Arc<RwLock<HashMap<NodeId, Member>>>,
    network: Arc<TestNetwork>,
}

impl Default for TestHarness {
    fn default() -> Self {
        let subscriber = tracing_subscriber::Registry::default()
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::Layer::default());
        tracing::subscriber::set_global_default(subscriber).unwrap();

        let config = Arc::new(Config::default());
        let network = Arc::new(TestNetwork::default());
        Self {
            config,
            nodes: network.nodes.clone(),
            network,
        }
    }
}

impl TestHarness {
    pub fn add_node(&self, id: NodeId) {
        let mut nodes = self.nodes.write();
        let storage = Arc::new(MemoryStorage::default());
        let raft = Raft::new(
            format!("node-{}", id),
            id,
            self.config.clone(),
            storage.clone(),
            self.network.clone(),
        )
        .unwrap();
        nodes.insert(id, Member { raft, storage });
    }

    pub async fn initialize(&self) -> Result<()> {
        let nodes = self.nodes.read();
        assert!(!nodes.is_empty());
        let raft = &nodes.iter().next().unwrap().1.raft.clone();
        raft.initialize(nodes.keys().map(|id| (*id, ()))).await?;
        Ok(())
    }
}
