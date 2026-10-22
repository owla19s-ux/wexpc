//! In-memory identity and selection rules, not a runtime or authorization service.
//! The existing Android singleton is not migrated or controlled by this module.
//! A future trusted service must persist the catalog and mediate all mutations.
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionKind {
    Ai,
    /// A platform adapter key, not a model, member, role, or permission.
    Tool(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Enabled,
    Disabled,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRecord {
    pub id: String,
    pub name: String,
    pub kind: ConnectionKind,
    /// Opaque trusted Host reference only. Never supply a Key, Token or Cookie.
    pub credential_ref: Option<String>,
    /// Configuration state only; not authentication, health, or authorization.
    pub status: ConnectionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiSelection {
    pub connection_id: String,
    /// A requested model identifier, not proof of provider availability.
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationRecord {
    pub id: String,
    pub title: String,
    pub selection: Option<AiSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewConversation {
    Unconfigured,
    Default,
    Explicit(AiSelection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogError {
    InvalidInput,
    AlreadyExists,
    NotFound,
    Unavailable,
    NotAi,
    Unconfigured,
}

/// An owned projection. Editing it never changes the authoritative Catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogView {
    pub connections: Vec<ConnectionRecord>,
    pub conversations: Vec<ConversationRecord>,
    pub default_selection: Option<AiSelection>,
}

/// Single-owner domain state. No disk I/O, SDKs, grants or execution side effects.
/// IDs come from a trusted caller and are never interpreted as role or hierarchy.
#[derive(Debug, Default)]
pub struct Catalog {
    connections: BTreeMap<String, ConnectionRecord>,
    conversations: BTreeMap<String, ConversationRecord>,
    default_selection: Option<AiSelection>,
}

fn valid_id(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn valid_text(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

impl Catalog {
    /// Validates a persisted projection without publishing intermediate state.
    /// Unavailable connections retain their identities and historical selections.
    pub fn from_view(view: CatalogView) -> Result<Self, CatalogError> {
        let mut catalog = Self::default();
        for connection in &view.connections {
            catalog.add_connection(
                &connection.id,
                &connection.name,
                connection.kind.clone(),
                connection.credential_ref.as_deref(),
            )?;
        }
        for conversation in view.conversations {
            let choice = match conversation.selection {
                Some(selection) => NewConversation::Explicit(selection),
                None => NewConversation::Unconfigured,
            };
            catalog.create_conversation(&conversation.id, &conversation.title, choice)?;
        }
        catalog.set_default(view.default_selection)?;
        for connection in view.connections {
            match connection.status {
                ConnectionStatus::Enabled => {}
                ConnectionStatus::Disabled => catalog.set_enabled(&connection.id, false)?,
                ConnectionStatus::Retired => catalog.retire_connection(&connection.id)?,
            }
        }
        Ok(catalog)
    }

    pub fn view(&self) -> CatalogView {
        CatalogView {
            connections: self.connections.values().cloned().collect(),
            conversations: self.conversations.values().cloned().collect(),
            default_selection: self.default_selection.clone(),
        }
    }

    pub fn add_connection(
        &mut self,
        id: &str,
        name: &str,
        kind: ConnectionKind,
        credential_ref: Option<&str>,
    ) -> Result<(), CatalogError> {
        let kind_valid = match &kind {
            ConnectionKind::Ai => true,
            ConnectionKind::Tool(adapter) => valid_id(adapter, 64),
        };
        if !valid_id(id, 64)
            || !valid_text(name, 128)
            || !kind_valid
            || credential_ref.is_some_and(|reference| !valid_id(reference, 128))
        {
            return Err(CatalogError::InvalidInput);
        }
        if self.connections.contains_key(id) {
            return Err(CatalogError::AlreadyExists);
        }
        self.connections.insert(
            id.to_owned(),
            ConnectionRecord {
                id: id.to_owned(),
                name: name.to_owned(),
                kind,
                credential_ref: credential_ref.map(str::to_owned),
                status: ConnectionStatus::Enabled,
            },
        );
        Ok(())
    }

    pub fn rename_connection(&mut self, id: &str, name: &str) -> Result<(), CatalogError> {
        if !valid_text(name, 128) {
            return Err(CatalogError::InvalidInput);
        }
        let connection = self.connections.get_mut(id).ok_or(CatalogError::NotFound)?;
        connection.name = name.to_owned();
        Ok(())
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), CatalogError> {
        let connection = self.connections.get_mut(id).ok_or(CatalogError::NotFound)?;
        if connection.status == ConnectionStatus::Retired {
            return Err(CatalogError::Unavailable);
        }
        connection.status = if enabled {
            ConnectionStatus::Enabled
        } else {
            ConnectionStatus::Disabled
        };
        Ok(())
    }

    /// Logical removal retains identity and historical references, not access.
    /// In-flight execution revocation and vault cleanup belong to the future host.
    pub fn retire_connection(&mut self, id: &str) -> Result<(), CatalogError> {
        let connection = self.connections.get_mut(id).ok_or(CatalogError::NotFound)?;
        connection.status = ConnectionStatus::Retired;
        Ok(())
    }

    fn validate_selection(&self, selection: &AiSelection) -> Result<(), CatalogError> {
        if !valid_id(&selection.connection_id, 64) || !valid_text(&selection.model_id, 256) {
            return Err(CatalogError::InvalidInput);
        }
        let connection = self
            .connections
            .get(&selection.connection_id)
            .ok_or(CatalogError::NotFound)?;
        if connection.status != ConnectionStatus::Enabled {
            return Err(CatalogError::Unavailable);
        }
        if connection.kind != ConnectionKind::Ai {
            return Err(CatalogError::NotAi);
        }
        Ok(())
    }

    /// The default is an initializer, never a fallback for an existing binding.
    pub fn set_default(&mut self, selection: Option<AiSelection>) -> Result<(), CatalogError> {
        if let Some(selection) = &selection {
            self.validate_selection(selection)?;
        }
        self.default_selection = selection;
        Ok(())
    }

    pub fn create_conversation(
        &mut self,
        id: &str,
        title: &str,
        choice: NewConversation,
    ) -> Result<(), CatalogError> {
        if !valid_id(id, 64) || !valid_text(title, 128) {
            return Err(CatalogError::InvalidInput);
        }
        if self.conversations.contains_key(id) {
            return Err(CatalogError::AlreadyExists);
        }
        let selection = match choice {
            NewConversation::Unconfigured => None,
            NewConversation::Default => self.default_selection.clone(),
            NewConversation::Explicit(selection) => Some(selection),
        };
        if let Some(selection) = &selection {
            self.validate_selection(selection)?;
        }
        self.conversations.insert(
            id.to_owned(),
            ConversationRecord {
                id: id.to_owned(),
                title: title.to_owned(),
                selection,
            },
        );
        Ok(())
    }

    pub fn rename_conversation(&mut self, id: &str, title: &str) -> Result<(), CatalogError> {
        if !valid_text(title, 128) {
            return Err(CatalogError::InvalidInput);
        }
        let conversation = self
            .conversations
            .get_mut(id)
            .ok_or(CatalogError::NotFound)?;
        conversation.title = title.to_owned();
        Ok(())
    }

    /// Only resolves a local selection; it does not approve an API/tool request.
    /// No fallback is permitted when a selected connection becomes unavailable.
    pub fn resolve_conversation(&self, id: &str) -> Result<AiSelection, CatalogError> {
        let conversation = self.conversations.get(id).ok_or(CatalogError::NotFound)?;
        let selection = conversation
            .selection
            .as_ref()
            .ok_or(CatalogError::Unconfigured)?;
        self.validate_selection(selection)?;
        Ok(selection.clone())
    }
}
