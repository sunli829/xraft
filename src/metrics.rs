use serde::{Deserialize, Serialize};

use crate::{LogIndex, NodeId, NodeInfo, Role, TermId};

#[derive(Serialize, Deserialize)]
pub struct Metrics {
    id: NodeId,
    role: Role,
    current_term: TermId,
    last_log_index: LogIndex,
    last_applied: LogIndex,
    has_leader: bool,
    current_leader: NodeId,
}
