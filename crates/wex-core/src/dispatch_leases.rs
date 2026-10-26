//! In-memory exclusive resource leases for concurrent dispatch attempts.
//! The Host must keep the registry alive across workers and revalidate durable
//! state after restart; this is not a persistent grant or filesystem lock.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    InvalidScope,
    Busy,
    Poisoned,
}

#[derive(Debug, Default)]
struct State {
    next_generation: u64,
    held: BTreeMap<String, (String, u64)>,
}

#[derive(Debug, Clone, Default)]
pub struct ResourceLeases {
    state: Arc<Mutex<State>>,
}

#[derive(Debug)]
pub struct ResourceLease {
    state: Arc<Mutex<State>>,
    scopes: Vec<String>,
    owner: String,
    generation: u64,
}

fn valid(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

impl ResourceLeases {
    /// Acquire all resource scopes atomically or none. Sorted, deduplicated
    /// scopes avoid order-dependent acquisition and self-conflicts.
    pub fn acquire(&self, owner: &str, scopes: &[String]) -> Result<ResourceLease, LeaseError> {
        if !valid(owner) || scopes.is_empty() || scopes.iter().any(|s| !valid(s)) {
            return Err(LeaseError::InvalidScope);
        }
        let mut unique = scopes.to_vec();
        unique.sort();
        unique.dedup();
        let mut state = self.state.lock().map_err(|_| LeaseError::Poisoned)?;
        if unique.iter().any(|scope| state.held.contains_key(scope)) {
            return Err(LeaseError::Busy);
        }
        state.next_generation = state
            .next_generation
            .checked_add(1)
            .ok_or(LeaseError::Busy)?;
        let generation = state.next_generation;
        for scope in &unique {
            state
                .held
                .insert(scope.clone(), (owner.to_owned(), generation));
        }
        Ok(ResourceLease {
            state: Arc::clone(&self.state),
            scopes: unique,
            owner: owner.to_owned(),
            generation,
        })
    }

    pub fn is_busy(&self, scope: &str) -> Result<bool, LeaseError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| LeaseError::Poisoned)?
            .held
            .contains_key(scope))
    }
}

impl Drop for ResourceLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            for scope in &self.scopes {
                if state.held.get(scope) == Some(&(self.owner.clone(), self.generation)) {
                    state.held.remove(scope);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scopes(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn overlapping_resources_are_exclusive_even_for_same_owner() {
        let registry = ResourceLeases::default();
        let first = registry
            .acquire("agent_a", &scopes(&["file_b", "file_a"]))
            .unwrap();
        assert!(matches!(
            registry.acquire("agent_b", &scopes(&["file_a"])),
            Err(LeaseError::Busy)
        ));
        assert!(matches!(
            registry.acquire("agent_a", &scopes(&["file_b"])),
            Err(LeaseError::Busy)
        ));
        assert!(!registry.is_busy("file_c").unwrap());
        drop(first);
        assert!(registry.acquire("agent_b", &scopes(&["file_a"])).is_ok());
    }

    #[test]
    fn failed_multi_scope_acquisition_does_not_reserve_partial_resources() {
        let registry = ResourceLeases::default();
        let _first = registry.acquire("agent_a", &scopes(&["b"])).unwrap();
        assert!(matches!(
            registry.acquire("agent_b", &scopes(&["a", "b"])),
            Err(LeaseError::Busy)
        ));
        assert!(!registry.is_busy("a").unwrap());
    }

    #[test]
    fn independent_scopes_can_run_concurrently_and_release_on_drop() {
        let registry = ResourceLeases::default();
        let a = registry.acquire("agent_a", &scopes(&["a"])).unwrap();
        let b = registry.acquire("agent_b", &scopes(&["b"])).unwrap();
        assert!(registry.is_busy("a").unwrap());
        assert!(registry.is_busy("b").unwrap());
        drop(a);
        assert!(!registry.is_busy("a").unwrap());
        assert!(registry.is_busy("b").unwrap());
        drop(b);
    }

    #[test]
    fn empty_and_control_character_scopes_are_rejected() {
        let registry = ResourceLeases::default();
        assert!(matches!(
            registry.acquire("agent", &[]),
            Err(LeaseError::InvalidScope)
        ));
        assert!(matches!(
            registry.acquire("agent", &scopes(&["bad\npath"])),
            Err(LeaseError::InvalidScope)
        ));
    }
    #[test]
    fn duplicate_scope_names_do_not_self_conflict() {
        let registry = ResourceLeases::default();
        let lease = registry
            .acquire("agent", &scopes(&["a", "a", "b"]))
            .unwrap();
        assert!(registry.is_busy("a").unwrap());
        assert!(registry.is_busy("b").unwrap());
        drop(lease);
        assert!(!registry.is_busy("a").unwrap());
    }

    #[test]
    fn cloned_registries_share_the_same_lock_state() {
        let registry = ResourceLeases::default();
        let other = registry.clone();
        let lease = registry.acquire("agent_a", &scopes(&["shared"])).unwrap();
        assert!(matches!(
            other.acquire("agent_b", &scopes(&["shared"])),
            Err(LeaseError::Busy)
        ));
        drop(lease);
        assert!(other.acquire("agent_b", &scopes(&["shared"])).is_ok());
    }
}
