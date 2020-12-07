use crate::{Entry, NodeId};
use serde::{Deserialize, Serialize};

pub type StorageResult<T> = anyhow::Result<T>;

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct HardState {
    pub current_term: u64,
    pub voted_for: Option<NodeId>,
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct InitialState {
    pub last_log_index: u64,
    pub last_log_term: u64,
    pub hard_state: Option<HardState>,
}

pub trait Storage<D>: Send + Sync + 'static {
    fn get_initial_state(&self) -> StorageResult<InitialState>;

    fn save_hard_state(&self, hard_state: HardState) -> StorageResult<()>;

    fn last_applied(&self) -> StorageResult<u64>;

    fn get_log_entries(&self, start: u64, end: u64) -> StorageResult<Vec<Entry<D>>>;

    fn delete_logs_from(&self, start: u64, end: Option<u64>) -> StorageResult<()>;

    fn append_entries_to_log(&self, entries: &[Entry<D>]) -> StorageResult<()>;

    fn apply_entries_to_state_machine(&self, entries: &[Entry<D>]) -> StorageResult<()>;
}
