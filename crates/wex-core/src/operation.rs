//! Pure single-owner operation lifecycle, not a credential or execution service.
//! All mutations are trusted service calls, never model-facing tools.
//! A durable service must confirm InFlight storage BEFORE any external dispatch.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationIntent {
    pub id: String,
    pub actor_id: String,
    pub conversation_id: String,
    pub context_id: Option<String>,
    /// Tool connection identity, not the actor's AI/model/role identity.
    pub connection_id: String,
    pub capability_id: String,
    pub resource_id: String,
    pub action: Action,
    pub expected_version: Option<String>,
    /// Host-computed digest of the entire canonical external operation.
    /// Core validates format only; it cannot verify raw parameters or evidence.
    pub intent_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationState {
    PendingApproval,
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
    Cancelled,
}

impl OperationState {
    fn attempt_id(&self) -> Option<&str> {
        match self {
            Self::InFlight { attempt_id }
            | Self::OutcomeUnknown { attempt_id }
            | Self::Succeeded { attempt_id, .. }
            | Self::Failed { attempt_id, .. } => Some(attempt_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRecord {
    pub intent: OperationIntent,
    pub state: OperationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerView {
    pub records: Vec<OperationRecord>,
}

/// Current trusted execution facts, not an authorization claim supplied by AI.
/// The authority must advance revision for relevant identity/scope/policy changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionChecks {
    pub authority_revision: u64,
    pub connection_enabled: bool,
    pub authenticated: bool,
    pub platform_available: bool,
    pub resource_allowed: bool,
}

/// Not Clone and not a security token. A trusted host must not replay this value.
#[derive(Debug, PartialEq, Eq)]
pub struct Dispatch {
    pub intent: OperationIntent,
    pub attempt_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Unknown,
    /// Platform success evidence, not independent Verify/review acceptance.
    Succeeded {
        evidence_ref: String,
    },
    /// A trusted adapter's confirmed failure; never infer this from a timeout.
    Failed {
        evidence_ref: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationError {
    InvalidInput,
    AlreadyExists,
    NotFound,
    IntentMismatch,
    InvalidState,
    StaleApproval,
    ConnectionUnavailable,
    AuthenticationUnavailable,
    PlatformUnavailable,
    ResourceDenied,
    AttemptMismatch,
    ReceiptConflict,
}

#[derive(Debug, Default)]
pub struct OperationLedger {
    records: BTreeMap<String, OperationRecord>,
}

fn valid_id(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn valid_intent(intent: &OperationIntent) -> bool {
    [
        &intent.id,
        &intent.actor_id,
        &intent.conversation_id,
        &intent.connection_id,
        &intent.capability_id,
        &intent.resource_id,
    ]
    .into_iter()
    .all(|id| valid_id(id, 64))
        && intent.context_id.as_ref().is_none_or(|id| valid_id(id, 64))
        && intent.expected_version.as_ref().is_none_or(|version| {
            !version.is_empty()
                && version.len() <= 256
                && version.trim() == version
                && !version.chars().any(char::is_control)
        })
        && intent.intent_digest.len() == 64
        && intent
            .intent_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_state(state: &OperationState) -> bool {
    if state.attempt_id().is_some_and(|id| !valid_id(id, 64)) {
        return false;
    }
    match state {
        OperationState::Succeeded { evidence_ref, .. }
        | OperationState::Failed { evidence_ref, .. } => valid_id(evidence_ref, 128),
        _ => true,
    }
}

impl OperationLedger {
    pub fn view(&self) -> LedgerView {
        LedgerView {
            records: self.records.values().cloned().collect(),
        }
    }

    /// Cold restoration only: invalidate old approvals and retain uncertain attempts.
    pub fn from_view(view: LedgerView) -> Result<Self, OperationError> {
        let mut ledger = Self::from_trusted_view(view)?;
        for record in ledger.records.values_mut() {
            record.state = match &record.state {
                OperationState::Approved { .. } => OperationState::PendingApproval,
                OperationState::InFlight { attempt_id } => OperationState::OutcomeUnknown {
                    attempt_id: attempt_id.clone(),
                },
                state => state.clone(),
            };
        }
        Ok(ledger)
    }

    /// Trusted transaction hydration, not a cold restart or approval API.
    /// Preserves valid current states; callers must fence the active service session.
    pub fn from_trusted_view(view: LedgerView) -> Result<Self, OperationError> {
        let mut ledger = Self::default();
        let mut attempts = BTreeSet::new();
        for record in view.records {
            if !valid_intent(&record.intent) || !valid_state(&record.state) {
                return Err(OperationError::InvalidInput);
            }
            if let Some(attempt) = record.state.attempt_id() {
                if !attempts.insert(attempt.to_owned()) {
                    return Err(OperationError::AlreadyExists);
                }
            }
            if ledger.records.contains_key(&record.intent.id) {
                return Err(OperationError::AlreadyExists);
            }
            ledger.records.insert(record.intent.id.clone(), record);
        }
        Ok(ledger)
    }

    pub fn propose(&mut self, intent: OperationIntent) -> Result<(), OperationError> {
        if !valid_intent(&intent) {
            return Err(OperationError::InvalidInput);
        }
        if self.records.contains_key(&intent.id) {
            return Err(OperationError::AlreadyExists);
        }
        self.records.insert(
            intent.id.clone(),
            OperationRecord {
                intent,
                state: OperationState::PendingApproval,
            },
        );
        Ok(())
    }

    /// Trusted human approval of this exact immutable intent; never an AI tool.
    pub fn approve(
        &mut self,
        id: &str,
        exact_intent: &OperationIntent,
        authority_revision: u64,
    ) -> Result<(), OperationError> {
        let record = self.records.get_mut(id).ok_or(OperationError::NotFound)?;
        if record.intent != *exact_intent {
            return Err(OperationError::IntentMismatch);
        }
        if !matches!(
            record.state,
            OperationState::PendingApproval | OperationState::Approved { .. }
        ) {
            return Err(OperationError::InvalidState);
        }
        record.state = OperationState::Approved { authority_revision };
        Ok(())
    }

    /// Consume a single approval before dispatch. This is not durable by itself.
    pub fn begin(
        &mut self,
        id: &str,
        attempt_id: &str,
        checks: ExecutionChecks,
    ) -> Result<Dispatch, OperationError> {
        if !valid_id(attempt_id, 64) {
            return Err(OperationError::InvalidInput);
        }
        let record = self.records.get(id).ok_or(OperationError::NotFound)?;
        let OperationState::Approved { authority_revision } = record.state else {
            return Err(OperationError::InvalidState);
        };
        if authority_revision != checks.authority_revision {
            return Err(OperationError::StaleApproval);
        }
        if !checks.connection_enabled {
            return Err(OperationError::ConnectionUnavailable);
        }
        if !checks.authenticated {
            return Err(OperationError::AuthenticationUnavailable);
        }
        if !checks.platform_available {
            return Err(OperationError::PlatformUnavailable);
        }
        if !checks.resource_allowed {
            return Err(OperationError::ResourceDenied);
        }
        if self
            .records
            .values()
            .any(|record| record.state.attempt_id() == Some(attempt_id))
        {
            return Err(OperationError::AlreadyExists);
        }
        let record = self.records.get_mut(id).ok_or(OperationError::NotFound)?;
        record.state = OperationState::InFlight {
            attempt_id: attempt_id.to_owned(),
        };
        Ok(Dispatch {
            intent: record.intent.clone(),
            attempt_id: attempt_id.to_owned(),
        })
    }

    /// Cancel only unstarted work; never claim rollback of an issued operation.
    pub fn cancel_unstarted(&mut self, id: &str) -> Result<(), OperationError> {
        let record = self.records.get_mut(id).ok_or(OperationError::NotFound)?;
        if !matches!(
            record.state,
            OperationState::PendingApproval | OperationState::Approved { .. }
        ) {
            return Err(OperationError::InvalidState);
        }
        record.state = OperationState::Cancelled;
        Ok(())
    }

    /// Trusted platform/reconciliation result for the original attempt only.
    /// Current revocation does not erase facts of an already dispatched attempt.
    pub fn finish(
        &mut self,
        id: &str,
        attempt_id: &str,
        outcome: Outcome,
    ) -> Result<(), OperationError> {
        let record = self.records.get_mut(id).ok_or(OperationError::NotFound)?;
        let original_attempt = record
            .state
            .attempt_id()
            .ok_or(OperationError::InvalidState)?;
        if original_attempt != attempt_id {
            return Err(OperationError::AttemptMismatch);
        }
        let state = match outcome {
            Outcome::Unknown => OperationState::OutcomeUnknown {
                attempt_id: attempt_id.to_owned(),
            },
            Outcome::Succeeded { evidence_ref } => OperationState::Succeeded {
                attempt_id: attempt_id.to_owned(),
                evidence_ref,
            },
            Outcome::Failed { evidence_ref } => OperationState::Failed {
                attempt_id: attempt_id.to_owned(),
                evidence_ref,
            },
        };
        if !valid_state(&state) {
            return Err(OperationError::InvalidInput);
        }
        if matches!(
            record.state,
            OperationState::Succeeded { .. } | OperationState::Failed { .. }
        ) {
            return if record.state == state {
                Ok(())
            } else {
                Err(OperationError::ReceiptConflict)
            };
        }
        record.state = state;
        Ok(())
    }
}
