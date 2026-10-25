use super::*;
use crate::platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileReadError {
    Denied(Denial),
    Resource(ResourceError),
    TooLarge,
    Changed,
}

impl DirectoryRegistry {
    fn check_pinned_scope(
        &self,
        pin: &File,
        root_id: &str,
        internal_pins: &[File],
    ) -> Result<(), FileReadError> {
        let identity = platform::identity_of_file(pin).map_err(FileReadError::Resource)?;
        if self.bindings.iter().any(|b| {
            b.id != root_id && b.protection == Protection::Protected && b.identity == identity
        }) {
            return Err(FileReadError::Resource(ResourceError::ProtectedBoundary));
        }
        for internal in internal_pins {
            if platform::identity_of_file(internal).map_err(FileReadError::Resource)? == identity {
                return Err(FileReadError::Resource(ResourceError::InternalDirectory));
            }
        }
        Ok(())
    }
    /// Read one relative ordinary file, opening each directory component
    /// beneath its pinned parent. Host must hold the authority-store lock.
    pub fn read_relative_file_snapshot(
        &self,
        directory_id: &str,
        relative: &str,
        system_access: bool,
        grants: &[DirectoryGrant<'_>],
        internal_roots: &[&Path],
    ) -> Result<RelativeFileSnapshot, FileReadError> {
        let parts: Vec<_> = relative.split('/').collect();
        if relative.len() > 4096
            || parts.len() > 64
            || parts
                .iter()
                .any(|s| !platform::valid_leaf(s) || s.contains('\\') || s.contains('\0'))
        {
            return Err(FileReadError::Resource(ResourceError::InvalidPath));
        }
        match self.check_with_exclusions(
            directory_id,
            relative,
            system_access,
            Access::Read,
            grants,
            internal_roots,
        ) {
            Ok(Decision::Allowed) => {}
            Ok(Decision::Denied(reason)) => return Err(FileReadError::Denied(reason)),
            Err(error) => return Err(FileReadError::Resource(error)),
        }
        let binding = self
            .bindings
            .iter()
            .find(|b| b.id == directory_id)
            .ok_or(FileReadError::Resource(ResourceError::UnknownDirectory))?;
        let root = binding
            ._pin
            .try_clone()
            .map_err(|_| FileReadError::Resource(ResourceError::Unavailable))?;
        let mut chain = vec![root];
        // Retain deny-root objects too: a concurrent rename cannot turn an
        // internal directory into an apparently ordinary child by changing its
        // name, nor recycle its identity before the read finishes.
        let internal_pins: Vec<_> = internal_roots
            .iter()
            .map(|path| {
                let canonical = fs::canonicalize(path)
                    .map_err(|_| FileReadError::Resource(ResourceError::Unavailable))?;
                platform::open_directory(&canonical).map_err(FileReadError::Resource)
            })
            .collect::<Result<_, _>>()?;
        self.check_pinned_scope(&chain[0], directory_id, &internal_pins)?;
        for part in &parts[..parts.len() - 1] {
            let child = platform::open_child_directory(chain.last().expect("pinned root"), part)?;
            self.check_pinned_scope(&child, directory_id, &internal_pins)?;
            chain.push(child);
        }
        let directory_stamps: Vec<_> = chain
            .iter()
            .map(durable_stamp)
            .collect::<Result<_, _>>()
            .map_err(FileReadError::Resource)?;
        let file = platform::read_leaf_snapshot(
            chain.last().expect("pinned parent"),
            parts.last().expect("validated leaf"),
        )?;
        // Reopen each name under the SAME pinned parent. Never use an ordinary
        // path open after permission checking; reject renamed/replaced chains.
        for (index, part) in parts[..parts.len() - 1].iter().enumerate() {
            let reopened = platform::open_child_directory(&chain[index], part)?;
            if durable_stamp(&reopened).map_err(FileReadError::Resource)?
                != directory_stamps[index + 1]
            {
                return Err(FileReadError::Changed);
            }
        }
        for registered in &self.bindings {
            registered.verify().map_err(FileReadError::Resource)?;
        }
        Ok(RelativeFileSnapshot {
            bytes: file.bytes,
            executable: file.executable,
            directory_stamps,
        })
    }

    /// One direct ordinary file; trusted grants, all bytes or an error.
    /// Store::read_registered holds the authority lock throughout.
    pub fn read_file(
        &self,
        directory_id: &str,
        leaf: &str,
        system_access: bool,
        grants: &[DirectoryGrant<'_>],
    ) -> Result<Vec<u8>, FileReadError> {
        self.read_file_with_exclusions(directory_id, leaf, system_access, grants, &[])
    }

    /// Internal roots are trusted deny-only Host configuration.
    ///
    /// The bytes are read through the platform backend, which opens the leaf
    /// relative to the pinned directory handle and re-checks object identity
    /// before and after the read. The user supplied path is never resolved a
    /// second time.
    pub fn read_file_with_exclusions(
        &self,
        directory_id: &str,
        leaf: &str,
        system_access: bool,
        grants: &[DirectoryGrant<'_>],
        internal_roots: &[&Path],
    ) -> Result<Vec<u8>, FileReadError> {
        if !platform::valid_leaf(leaf) {
            return Err(FileReadError::Resource(ResourceError::InvalidPath));
        }
        match self.check_with_exclusions(
            directory_id,
            leaf,
            system_access,
            Access::Read,
            grants,
            internal_roots,
        ) {
            Ok(Decision::Allowed) => {}
            Ok(Decision::Denied(reason)) => return Err(FileReadError::Denied(reason)),
            Err(error) => return Err(FileReadError::Resource(error)),
        }
        if !platform::supported() {
            return Err(FileReadError::Resource(ResourceError::Unsupported));
        }
        let binding = self
            .bindings
            .iter()
            .find(|b| b.id == directory_id)
            .ok_or(FileReadError::Resource(ResourceError::UnknownDirectory))?;
        let bytes = platform::read_leaf(&binding._pin, leaf)?;
        for registered in &self.bindings {
            registered.verify().map_err(FileReadError::Resource)?;
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod snapshot_scope_tests {
    use super::*;
    #[test]
    fn renamed_protected_or_internal_directory_is_denied_by_its_pinned_identity() {
        let root = std::env::temp_dir().join(format!(
            "wex-pin-scope-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("private")).unwrap();
        fs::create_dir(root.join("internal")).unwrap();
        let mut registry = DirectoryRegistry::new();
        let id = registry.register(&root, Protection::General).unwrap();
        registry
            .register(root.join("private"), Protection::Protected)
            .unwrap();
        let parent = &registry.bindings.iter().find(|b| b.id == id).unwrap()._pin;
        let private = platform::open_child_directory(parent, "private").unwrap();
        let internal = platform::open_child_directory(parent, "internal").unwrap();
        fs::rename(root.join("private"), root.join("allowed-name")).unwrap();
        // The decision must use the object, irrespective of the name under
        // which it was exchanged after a prior policy check.
        let result = registry.check_pinned_scope(&private, &id, &[]);
        let internal_result =
            registry.check_pinned_scope(&internal, &id, &[internal.try_clone().unwrap()]);
        drop(private);
        drop(internal);
        drop(registry);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            result,
            Err(FileReadError::Resource(ResourceError::ProtectedBoundary))
        );
        assert_eq!(
            internal_result,
            Err(FileReadError::Resource(ResourceError::InternalDirectory))
        );
    }
}
