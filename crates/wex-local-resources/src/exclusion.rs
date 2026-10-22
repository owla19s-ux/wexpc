use super::*;
use crate::platform;

/// Host-supplied internal directories are deny boundaries, never authorizations.
/// Names are not inferred: all paths originate in trusted application composition.
pub(super) fn guard(binding: &Binding, roots: &[&Path]) -> Result<(), ResourceError> {
    if roots.len() > 16 {
        return Err(ResourceError::LimitExceeded);
    }
    for root in roots {
        let text = root.to_str().ok_or(ResourceError::InvalidPath)?;
        if !root.is_absolute() || text.len() > 4096 || text.contains('\0') {
            return Err(ResourceError::InvalidPath);
        }
        let path = fs::canonicalize(root).map_err(|_| ResourceError::Unavailable)?;
        let pin = platform::open_directory(&path)?;
        let object = platform::identity_of_file(&pin)?;
        // Component relations, not a textual prefix. Reject an entire ancestor
        // binding rather than permit future traversal near private descendants.
        if binding.path.starts_with(&path)
            || path.starts_with(&binding.path)
            || binding.identity == object
        {
            return Err(ResourceError::InternalDirectory);
        }
    }
    Ok(())
}
