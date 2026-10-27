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

fn valid_scope(value: &str) -> bool {
    valid(value)
        && !value.contains('\\')
        && !value.contains("//")
        && !value
            .split('/')
            .any(|segment| segment == "." || segment == "..")
        && (value == "/" || !value.ends_with('/'))
}

fn scopes_overlap(a: &str, b: &str) -> bool {
    a == b
        || (a == "/" && b.starts_with('/'))
        || (b == "/" && a.starts_with('/'))
        || a.strip_prefix(b).is_some_and(|rest| rest.starts_with('/'))
        || b.strip_prefix(a).is_some_and(|rest| rest.starts_with('/'))
}

impl ResourceLeases {
    /// Acquire all resource scopes atomically or none. Sorted, deduplicated
    /// scopes avoid order-dependent acquisition and self-conflicts.
    pub fn acquire(&self, owner: &str, scopes: &[String]) -> Result<ResourceLease, LeaseError> {
        if !valid(owner) || scopes.is_empty() || scopes.iter().any(|s| !valid_scope(s)) {
            return Err(LeaseError::InvalidScope);
        }
        let mut unique = scopes.to_vec();
        unique.sort();
        unique.dedup();
        let mut state = self.state.lock().map_err(|_| LeaseError::Poisoned)?;
        if unique
            .iter()
            .any(|scope| state.held.keys().any(|held| scopes_overlap(scope, held)))
        {
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
        if !valid_scope(scope) {
            return Err(LeaseError::InvalidScope);
        }
        Ok(self
            .state
            .lock()
            .map_err(|_| LeaseError::Poisoned)?
            .held
            .keys()
            .any(|held| scopes_overlap(scope, held)))
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
    fn ancestor_and_descendant_conflict_at_segment_boundaries() {
        let registry = ResourceLeases::default();
        let parent = registry.acquire("a", &scopes(&["src"])).unwrap();
        assert_eq!(registry.is_busy("src/main.rs"), Ok(true));
        assert!(matches!(
            registry.acquire("b", &scopes(&["src/main.rs"])),
            Err(LeaseError::Busy)
        ));
        assert_eq!(registry.is_busy("src-other/main.rs"), Ok(false));
        drop(parent);
        let root = registry.acquire("a", &scopes(&["/"])).unwrap();
        assert_eq!(registry.is_busy("/src/main.rs"), Ok(true));
        assert_eq!(registry.is_busy("relative/path"), Ok(false));
        drop(root);
        assert!(registry.acquire("b", &scopes(&["/src"])).is_ok());
    }

    #[test]
    fn alias_scopes_fail_closed_for_acquire_and_probe() {
        let registry = ResourceLeases::default();
        for alias in [
            "",
            "src/../src",
            "./src",
            "src/./file",
            "src//file",
            "src/",
            "src\\file",
        ] {
            assert!(matches!(
                registry.acquire("a", &scopes(&[alias])),
                Err(LeaseError::InvalidScope)
            ));
            assert_eq!(registry.is_busy(alias), Err(LeaseError::InvalidScope));
        }
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
    fn independent_registry_instances_do_not_share_ownership() {
        // Safety boundary: in-memory leases are not a cross-process lock.
        // The Host must not infer durable or multi-process exclusion from them.
        let first = ResourceLeases::default();
        let second = ResourceLeases::default();
        let _held = first.acquire("a", &scopes(&["shared"])).unwrap();
        assert_eq!(first.is_busy("shared"), Ok(true));
        assert_eq!(second.is_busy("shared"), Ok(false));
    }

    #[test]
    fn lease_drop_cannot_release_a_different_generation() {
        let registry = ResourceLeases::default();
        let first = registry.acquire("agent", &scopes(&["shared"])).unwrap();
        let stale_generation = first.generation;
        drop(first);
        let current = registry.acquire("agent", &scopes(&["shared"])).unwrap();
        assert!(current.generation > stale_generation);
        // Simulate a delayed stale lease being dropped after a new generation
        // acquired the same scope. Only the current generation may release it.
        let stale = ResourceLease {
            state: Arc::clone(&registry.state),
            scopes: scopes(&["shared"]),
            owner: "agent".to_owned(),
            generation: stale_generation,
        };
        drop(stale);
        assert_eq!(registry.is_busy("shared"), Ok(true));
        drop(current);
        assert_eq!(registry.is_busy("shared"), Ok(false));
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

    #[test]
    fn competing_threads_cannot_hold_overlapping_scopes_together() {
        use std::sync::{mpsc, Barrier};
        use std::thread;

        let registry = ResourceLeases::default();
        let start = Arc::new(Barrier::new(3));
        let (result_tx, result_rx) = mpsc::channel();
        let mut releases = Vec::new();
        let mut workers = Vec::new();

        for scope in ["src", "src/main.rs"] {
            let registry = registry.clone();
            let start = Arc::clone(&start);
            let result_tx = result_tx.clone();
            let (release_tx, release_rx) = mpsc::channel::<()>();
            releases.push(release_tx);
            workers.push(thread::spawn(move || {
                start.wait();
                let lease = registry.acquire("worker", &[scope.to_owned()]);
                result_tx.send(lease.is_ok()).unwrap();
                if let Ok(_held) = lease {
                    release_rx.recv().unwrap();
                }
            }));
        }
        drop(result_tx);
        start.wait();
        let winners = u8::from(result_rx.recv().unwrap()) + u8::from(result_rx.recv().unwrap());
        assert_eq!(winners, 1);
        for release in releases {
            let _ = release.send(());
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(!registry.is_busy("src/main.rs").unwrap());
    }
}
