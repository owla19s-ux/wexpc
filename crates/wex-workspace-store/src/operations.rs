//! Trusted private operation persistence. No Connector or model-facing entry point.
//! A Dispatch is released only after InFlight storage is confirmed durable.
use super::{Lock, OrganizationError, OrganizationStore, SNAPSHOT_LIMIT};
use serde::{Deserialize, Serialize};
use std::path::Path;
use wex_core::operation::{
    Action, Dispatch, ExecutionChecks, LedgerView, OperationError, OperationIntent,
    OperationLedger, OperationRecord, OperationState, Outcome,
};

const INITIALIZED: &[u8] = b"operations-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStoreError {
    Storage(OrganizationError),
    Domain(OperationError),
    InvalidSnapshot,
    UnsupportedVersion,
    StaleSession,
}

impl From<OrganizationError> for OperationStoreError {
    fn from(error: OrganizationError) -> Self {
        Self::Storage(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStoreView {
    pub revision: u64,
    pub generation: u64,
    pub ledger: LedgerView,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommittedDispatch {
    pub revision: u64,
    pub generation: u64,
    pub dispatch: Dispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationChange {
    Propose(OperationIntent),
    Approve {
        id: String,
        exact_intent: OperationIntent,
        authority_revision: u64,
    },
    CancelUnstarted {
        id: String,
    },
    Finish {
        id: String,
        attempt_id: String,
        outcome: Outcome,
    },
}

// Remote derives keep persistence concerns out of the pure core crate.
#[derive(Serialize, Deserialize)]
#[serde(remote = "Action", rename_all = "snake_case")]
enum ActionDef {
    Read,
    Write,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "OperationIntent", deny_unknown_fields)]
struct IntentDef {
    id: String,
    actor_id: String,
    conversation_id: String,
    context_id: Option<String>,
    connection_id: String,
    capability_id: String,
    resource_id: String,
    #[serde(with = "ActionDef")]
    action: Action,
    expected_version: Option<String>,
    intent_digest: String,
}

// Empty struct variants deliberately reject fields; serde's tagged unit visitor
// otherwise discards payload fields even with enum deny_unknown_fields.
#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum StateWire {
    PendingApproval {},
    Approved {
        authority_revision: u64,
    },
    InFlight {
        attempt_id: String,
    },
    OutcomeUnknown {
        attempt_id: String,
    },
    Succeeded {
        attempt_id: String,
        evidence_ref: String,
    },
    Failed {
        attempt_id: String,
        evidence_ref: String,
    },
    Cancelled {},
}

impl From<OperationState> for StateWire {
    fn from(state: OperationState) -> Self {
        match state {
            OperationState::PendingApproval => Self::PendingApproval {},
            OperationState::Approved { authority_revision } => {
                Self::Approved { authority_revision }
            }
            OperationState::InFlight { attempt_id } => Self::InFlight { attempt_id },
            OperationState::OutcomeUnknown { attempt_id } => Self::OutcomeUnknown { attempt_id },
            OperationState::Succeeded {
                attempt_id,
                evidence_ref,
            } => Self::Succeeded {
                attempt_id,
                evidence_ref,
            },
            OperationState::Failed {
                attempt_id,
                evidence_ref,
            } => Self::Failed {
                attempt_id,
                evidence_ref,
            },
            OperationState::Cancelled => Self::Cancelled {},
        }
    }
}

impl From<StateWire> for OperationState {
    fn from(state: StateWire) -> Self {
        match state {
            StateWire::PendingApproval {} => Self::PendingApproval,
            StateWire::Approved { authority_revision } => Self::Approved { authority_revision },
            StateWire::InFlight { attempt_id } => Self::InFlight { attempt_id },
            StateWire::OutcomeUnknown { attempt_id } => Self::OutcomeUnknown { attempt_id },
            StateWire::Succeeded {
                attempt_id,
                evidence_ref,
            } => Self::Succeeded {
                attempt_id,
                evidence_ref,
            },
            StateWire::Failed {
                attempt_id,
                evidence_ref,
            } => Self::Failed {
                attempt_id,
                evidence_ref,
            },
            StateWire::Cancelled {} => Self::Cancelled,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    #[serde(with = "IntentDef")]
    intent: OperationIntent,
    state: StateWire,
}

#[derive(Deserialize)]
struct Version {
    schema_version: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: u32,
    revision: u64,
    generation: u64,
    records: Vec<Record>,
}

impl Snapshot {
    fn from_ledger(revision: u64, generation: u64, ledger: &OperationLedger) -> Self {
        Self {
            schema_version: 1,
            revision,
            generation,
            records: ledger
                .view()
                .records
                .into_iter()
                .map(|record| Record {
                    intent: record.intent,
                    state: record.state.into(),
                })
                .collect(),
        }
    }

    fn into_ledger(self) -> Result<(u64, u64, OperationLedger), OperationStoreError> {
        if self.generation == 0 || self.revision < self.generation {
            return Err(OperationStoreError::InvalidSnapshot);
        }
        let view = LedgerView {
            records: self
                .records
                .into_iter()
                .map(|record| OperationRecord {
                    intent: record.intent,
                    state: record.state.into(),
                })
                .collect(),
        };
        let ledger = OperationLedger::from_trusted_view(view)
            .map_err(|_| OperationStoreError::InvalidSnapshot)?;
        Ok((self.revision, self.generation, ledger))
    }
}

#[derive(Debug)]
pub struct OperationStore {
    io: OrganizationStore,
    marker: OrganizationStore,
    generation: u64,
}

impl OperationStore {
    /// Begins a new trusted service session and fences previous handles.
    /// Reuse this handle for ordinary transactions; do not open per UI query.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, OperationStoreError> {
        let mut store = Self {
            io: OrganizationStore::open_named(root.as_ref(), "operations")?,
            marker: OrganizationStore::open_named(root, "operations-initialized")?,
            generation: 0,
        };
        let mut lock = store.io.lock()?;
        let existing = store.read_locked()?;
        let (revision, generation, ledger) = match existing {
            Some((revision, generation, ledger)) => {
                let recovered = OperationLedger::from_view(ledger.view())
                    .map_err(|_| OperationStoreError::InvalidSnapshot)?;
                (revision, generation, recovered)
            }
            None => (0, 0, OperationLedger::default()),
        };
        let revision = revision
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        let generation = generation
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        if generation == 1 {
            // Marker first: interrupted initialization never looks like an empty store.
            if let Err(error) = store.marker.save_bytes_locked(INITIALIZED) {
                lock.release = false;
                return Err(error.into());
            }
        }
        store.save_locked(&mut lock, revision, generation, &ledger)?;
        store.generation = generation;
        Ok(store)
    }

    fn read_locked(&self) -> Result<Option<(u64, u64, OperationLedger)>, OperationStoreError> {
        let bytes = self.io.read_bytes_locked()?;
        let marker = self.marker.read_bytes_locked()?;
        let bytes = match (bytes, marker) {
            (None, None) => return Ok(None),
            (Some(bytes), Some(marker)) if marker == INITIALIZED => bytes,
            _ => return Err(OperationStoreError::InvalidSnapshot),
        };
        let version: Version =
            serde_json::from_slice(&bytes).map_err(|_| OperationStoreError::InvalidSnapshot)?;
        if version.schema_version != 1 {
            return Err(OperationStoreError::UnsupportedVersion);
        }
        let snapshot: Snapshot =
            serde_json::from_slice(&bytes).map_err(|_| OperationStoreError::InvalidSnapshot)?;
        Ok(Some(snapshot.into_ledger()?))
    }

    fn current_locked(&self) -> Result<(u64, OperationLedger), OperationStoreError> {
        let (revision, generation, ledger) = self
            .read_locked()?
            .ok_or(OperationStoreError::InvalidSnapshot)?;
        if generation != self.generation {
            return Err(OperationStoreError::StaleSession);
        }
        Ok((revision, ledger))
    }

    fn save_locked(
        &self,
        lock: &mut Lock,
        revision: u64,
        generation: u64,
        ledger: &OperationLedger,
    ) -> Result<(), OperationStoreError> {
        let bytes = serde_json::to_vec(&Snapshot::from_ledger(revision, generation, ledger))
            .map_err(|_| OrganizationError::Unavailable)?;
        if bytes.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::LimitExceeded.into());
        }
        // Reserve recovery/counter growth and the longest legal terminal receipt.
        // This projection estimates capacity only; it is never published or saved.
        let recovered = OperationLedger::from_view(ledger.view())
            .map_err(|_| OperationStoreError::InvalidSnapshot)?;
        let mut budget = Snapshot::from_ledger(u64::MAX, u64::MAX, &recovered);
        for record in &mut budget.records {
            if let StateWire::OutcomeUnknown { attempt_id } = &record.state {
                record.state = StateWire::Succeeded {
                    attempt_id: attempt_id.clone(),
                    evidence_ref: "x".repeat(128),
                };
            }
        }
        let budget_bytes =
            serde_json::to_vec(&budget).map_err(|_| OrganizationError::Unavailable)?;
        if budget_bytes.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::LimitExceeded.into());
        }
        if let Err(error) = self.io.save_bytes_locked(&bytes) {
            lock.release = false;
            return Err(error.into());
        }
        Ok(())
    }

    fn view(&self, revision: u64, ledger: &OperationLedger) -> OperationStoreView {
        OperationStoreView {
            revision,
            generation: self.generation,
            ledger: ledger.view(),
        }
    }

    pub fn load(&self) -> Result<OperationStoreView, OperationStoreError> {
        let _lock = self.io.lock()?;
        let (revision, ledger) = self.current_locked()?;
        Ok(self.view(revision, &ledger))
    }

    pub fn apply(
        &self,
        expected_revision: u64,
        change: OperationChange,
    ) -> Result<OperationStoreView, OperationStoreError> {
        let mut lock = self.io.lock()?;
        let (revision, mut ledger) = self.current_locked()?;
        if revision != expected_revision {
            return Err(OrganizationError::Conflict.into());
        }
        let before = ledger.view();
        apply_change(&mut ledger, change).map_err(OperationStoreError::Domain)?;
        // A repeated identical receipt does not create another disk revision.
        if ledger.view() == before {
            return Ok(self.view(revision, &ledger));
        }
        let revision = revision
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        self.save_locked(&mut lock, revision, self.generation, &ledger)?;
        Ok(self.view(revision, &ledger))
    }

    /// The caller may invoke the Connector only after this returns Ok.
    /// Never retry a failed/unknown dispatch commit by issuing external I/O.
    pub fn begin(
        &self,
        expected_revision: u64,
        id: &str,
        attempt_id: &str,
        checks: ExecutionChecks,
    ) -> Result<CommittedDispatch, OperationStoreError> {
        let mut lock = self.io.lock()?;
        let (revision, mut ledger) = self.current_locked()?;
        if revision != expected_revision {
            return Err(OrganizationError::Conflict.into());
        }
        let dispatch = ledger
            .begin(id, attempt_id, checks)
            .map_err(OperationStoreError::Domain)?;
        let revision = revision
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        self.save_locked(&mut lock, revision, self.generation, &ledger)?;
        Ok(CommittedDispatch {
            revision,
            generation: self.generation,
            dispatch,
        })
    }
}

fn apply_change(
    ledger: &mut OperationLedger,
    change: OperationChange,
) -> Result<(), OperationError> {
    match change {
        OperationChange::Propose(intent) => ledger.propose(intent),
        OperationChange::Approve {
            id,
            exact_intent,
            authority_revision,
        } => ledger.approve(&id, &exact_intent, authority_revision),
        OperationChange::CancelUnstarted { id } => ledger.cancel_unstarted(&id),
        OperationChange::Finish {
            id,
            attempt_id,
            outcome,
        } => ledger.finish(&id, &attempt_id, outcome),
    }
}
