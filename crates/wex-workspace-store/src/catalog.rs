//! Trusted local catalog persistence. This does not authorize or execute requests.
use super::{OrganizationError, OrganizationStore, SNAPSHOT_LIMIT};
use serde::{Deserialize, Serialize};
use std::path::Path;
use wex_core::connection_session::{
    AiSelection, Catalog, CatalogError, CatalogView, ConnectionKind, ConnectionRecord,
    ConnectionStatus, ConversationRecord, NewConversation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogStoreError {
    Storage(OrganizationError),
    Domain(CatalogError),
    InvalidSnapshot,
    UnsupportedVersion,
}

impl From<OrganizationError> for CatalogStoreError {
    fn from(error: OrganizationError) -> Self {
        Self::Storage(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogState {
    pub revision: u64,
    pub catalog: CatalogView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogChange {
    AddConnection {
        id: String,
        name: String,
        kind: ConnectionKind,
        credential_ref: Option<String>,
    },
    RenameConnection {
        id: String,
        name: String,
    },
    SetEnabled {
        id: String,
        enabled: bool,
    },
    RetireConnection {
        id: String,
    },
    SetDefault(Option<AiSelection>),
    CreateConversation {
        id: String,
        title: String,
        choice: NewConversation,
    },
    RenameConversation {
        id: String,
        title: String,
    },
}

// Probe only the version; V1 is still decoded again with strict field checking.
#[derive(Deserialize)]
struct Version {
    schema_version: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: u32,
    revision: u64,
    connections: Vec<Connection>,
    conversations: Vec<Conversation>,
    default_selection: Option<Selection>,
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "adapter",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Kind {
    Ai,
    Tool(String),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Enabled,
    Disabled,
    Retired,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Connection {
    id: String,
    name: String,
    kind: Kind,
    credential_ref: Option<String>,
    status: Status,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    connection_id: String,
    model_id: String,
}

impl From<Selection> for AiSelection {
    fn from(value: Selection) -> Self {
        Self {
            connection_id: value.connection_id,
            model_id: value.model_id,
        }
    }
}

impl From<AiSelection> for Selection {
    fn from(value: AiSelection) -> Self {
        Self {
            connection_id: value.connection_id,
            model_id: value.model_id,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Conversation {
    id: String,
    title: String,
    selection: Option<Selection>,
}

impl Snapshot {
    fn into_catalog(self) -> Result<(u64, Catalog), CatalogStoreError> {
        if self.schema_version != 1 {
            return Err(CatalogStoreError::UnsupportedVersion);
        }
        let view = CatalogView {
            connections: self
                .connections
                .into_iter()
                .map(|record| ConnectionRecord {
                    id: record.id,
                    name: record.name,
                    kind: match record.kind {
                        Kind::Ai => ConnectionKind::Ai,
                        Kind::Tool(adapter) => ConnectionKind::Tool(adapter),
                    },
                    credential_ref: record.credential_ref,
                    status: match record.status {
                        Status::Enabled => ConnectionStatus::Enabled,
                        Status::Disabled => ConnectionStatus::Disabled,
                        Status::Retired => ConnectionStatus::Retired,
                    },
                })
                .collect(),
            conversations: self
                .conversations
                .into_iter()
                .map(|record| ConversationRecord {
                    id: record.id,
                    title: record.title,
                    selection: record.selection.map(Into::into),
                })
                .collect(),
            default_selection: self.default_selection.map(Into::into),
        };
        let catalog = Catalog::from_view(view).map_err(|_| CatalogStoreError::InvalidSnapshot)?;
        Ok((self.revision, catalog))
    }

    fn from_catalog(revision: u64, catalog: &Catalog) -> Self {
        let view = catalog.view();
        Self {
            schema_version: 1,
            revision,
            connections: view
                .connections
                .into_iter()
                .map(|record| Connection {
                    id: record.id,
                    name: record.name,
                    kind: match record.kind {
                        ConnectionKind::Ai => Kind::Ai,
                        ConnectionKind::Tool(adapter) => Kind::Tool(adapter),
                    },
                    credential_ref: record.credential_ref,
                    status: match record.status {
                        ConnectionStatus::Enabled => Status::Enabled,
                        ConnectionStatus::Disabled => Status::Disabled,
                        ConnectionStatus::Retired => Status::Retired,
                    },
                })
                .collect(),
            conversations: view
                .conversations
                .into_iter()
                .map(|record| Conversation {
                    id: record.id,
                    title: record.title,
                    selection: record.selection.map(Into::into),
                })
                .collect(),
            default_selection: view.default_selection.map(Into::into),
        }
    }
}

#[derive(Debug)]
pub struct CatalogStore {
    io: OrganizationStore,
}

impl CatalogStore {
    /// Only a trusted Host may supply this app-private root.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, CatalogStoreError> {
        Ok(Self {
            io: OrganizationStore::open_named(root, "catalog")?,
        })
    }

    fn read_locked(&self) -> Result<(u64, Catalog), CatalogStoreError> {
        let Some(bytes) = self.io.read_bytes_locked()? else {
            return Ok((0, Catalog::default()));
        };
        let version: Version =
            serde_json::from_slice(&bytes).map_err(|_| CatalogStoreError::InvalidSnapshot)?;
        if version.schema_version != 1 {
            return Err(CatalogStoreError::UnsupportedVersion);
        }
        let snapshot: Snapshot =
            serde_json::from_slice(&bytes).map_err(|_| CatalogStoreError::InvalidSnapshot)?;
        snapshot.into_catalog()
    }

    pub fn load(&self) -> Result<CatalogState, CatalogStoreError> {
        let _lock = self.io.lock()?;
        let (revision, catalog) = self.read_locked()?;
        Ok(CatalogState {
            revision,
            catalog: catalog.view(),
        })
    }

    /// Re-reads under an exclusive local lock; never retries a stale revision.
    pub fn apply(
        &self,
        expected_revision: u64,
        change: CatalogChange,
    ) -> Result<CatalogState, CatalogStoreError> {
        let mut lock = self.io.lock()?;
        let (revision, mut catalog) = self.read_locked()?;
        if revision != expected_revision {
            return Err(OrganizationError::Conflict.into());
        }
        apply_change(&mut catalog, change).map_err(CatalogStoreError::Domain)?;
        let revision = revision
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        let bytes = serde_json::to_vec(&Snapshot::from_catalog(revision, &catalog))
            .map_err(|_| OrganizationError::Unavailable)?;
        // Reject before starting I/O, so an ordinary size error releases the lock.
        if bytes.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::LimitExceeded.into());
        }
        if let Err(error) = self.io.save_bytes_locked(&bytes) {
            // Keep evidence after partial/uncertain writes. No automatic lock stealing.
            lock.release = false;
            return Err(error.into());
        }
        Ok(CatalogState {
            revision,
            catalog: catalog.view(),
        })
    }
}

fn apply_change(catalog: &mut Catalog, change: CatalogChange) -> Result<(), CatalogError> {
    match change {
        CatalogChange::AddConnection {
            id,
            name,
            kind,
            credential_ref,
        } => catalog.add_connection(&id, &name, kind, credential_ref.as_deref()),
        CatalogChange::RenameConnection { id, name } => catalog.rename_connection(&id, &name),
        CatalogChange::SetEnabled { id, enabled } => catalog.set_enabled(&id, enabled),
        CatalogChange::RetireConnection { id } => catalog.retire_connection(&id),
        CatalogChange::SetDefault(selection) => catalog.set_default(selection),
        CatalogChange::CreateConversation { id, title, choice } => {
            catalog.create_conversation(&id, &title, choice)
        }
        CatalogChange::RenameConversation { id, title } => catalog.rename_conversation(&id, &title),
    }
}
