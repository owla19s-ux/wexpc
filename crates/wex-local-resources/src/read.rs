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
