//! Publish immutable internal evidence; never use this for user project edits.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
};

pub(crate) fn publish(path: &Path, bytes: &[u8]) -> io::Result<()> {
    publish_with(path, |file| file.write_all(bytes))
}

fn publish_with(path: &Path, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing evidence directory"))?;
    let staging = parent.join(format!(".pending-{}", uuid::Uuid::new_v4().simple()));
    let mut file = options.open(&staging)?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let cleanup = Cleanup(staging.clone());
    write(&mut file)?;
    file.sync_all()?;
    drop(file);
    // Same-directory hard-link publication is atomic and never replaces an
    // existing name (unlike rename on Unix). Unsupported filesystems fail;
    // do not fall back to writing directly into the visible evidence file.
    fs::hard_link(&staging, path)?;
    fs::remove_file(&staging)?;
    drop(cleanup);
    #[cfg(unix)]
    {
        // Callers may have just created root/conversation/turn directories.
        // Flush every parent entry as well as the record's own directory;
        // syncing only the deepest directory can lose the entire new subtree.
        for directory in parent.ancestors().filter(|p| !p.as_os_str().is_empty()) {
            File::open(directory)?.sync_all()?;
        }
    }
    // Windows file data was flushed before publication. Namespace durability
    // across power loss / inherited ACLs still require Windows verification.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_write_does_not_publish_partial_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt.json");
        assert!(publish_with(&path, |f| {
            f.write_all(b"{\"partial\":")?;
            assert!(!path.exists(), "readers must not see an uncommitted record");
            Err(io::Error::other("injected disk failure"))
        })
        .is_err());
        assert!(!path.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        publish(&path, b"complete").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"complete");
    }
    #[test]
    fn duplicate_publication_keeps_original_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");
        publish(&path, b"first").unwrap();
        assert!(publish(&path, b"second").is_err());
        assert_eq!(fs::read(path).unwrap(), b"first");
    }
    #[test]
    fn competing_publishers_never_replace_the_winner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.json");
        let barrier = std::sync::Barrier::new(8);
        let successes = std::thread::scope(|scope| {
            let jobs = (0..8)
                .map(|n| {
                    let path = &path;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        publish(path, &[n]).is_ok()
                    })
                })
                .collect::<Vec<_>>();
            jobs.into_iter()
                .map(|job| usize::from(job.join().unwrap()))
                .sum::<usize>()
        });
        assert_eq!(successes, 1);
        assert_eq!(fs::read(path).unwrap().len(), 1);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn published_evidence_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private.json");
        publish(&path, b"receipt").unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
