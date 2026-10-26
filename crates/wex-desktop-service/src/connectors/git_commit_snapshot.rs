//! Read-only version collection from the authorized, pinned directory backend.
//! Host must hold its live authority lock; observations do not reserve writers.
use super::git_commit_evidence::{PreparedCommit, RepositoryVersion};
use crate::{Result, ServiceError};
use std::path::Path;
use wex_core::access::DirectoryGrant;
use wex_local_resources::DirectoryRegistry;

pub struct GitCommitSnapshotSource<'a> {
    pub registry: &'a DirectoryRegistry,
    pub directory_id: &'a str,
    pub system_access: bool,
    pub grants: &'a [DirectoryGrant<'a>],
    pub internal_roots: &'a [&'a Path],
}

pub fn prepare_git_commit_from_registry(
    source: &GitCommitSnapshotSource<'_>,
    message: &str,
    paths: &[String],
) -> Result<PreparedCommit> {
    super::git_commit_policy::validate_commit_request(message, paths)
        .map_err(|_| ServiceError::new("INVALID_INPUT", "Git 提交消息或路径无效"))?;
    let version = observe_stable(source, paths)?;
    PreparedCommit::new(message.into(), paths.to_vec(), version)
        .map_err(|_| ServiceError::new("GIT_SNAPSHOT_UNAVAILABLE", "仓库版本不能构成提交请求"))
}

pub fn observe_prepared_git_commit(
    source: &GitCommitSnapshotSource<'_>,
    prepared: &PreparedCommit,
) -> Result<RepositoryVersion> {
    observe_stable(source, prepared.paths())
}

/// Final read-only gate under Host resource/authority ownership. It derives
/// the root from the trusted registry instead of accepting a second path.
/// No write or durable InFlight record is performed by this function.
pub fn verify_live_git_commit_dispatch(
    dispatch: &wex_core::operation::Dispatch,
    resource_id: &str,
    source: &GitCommitSnapshotSource<'_>,
    prepared: &PreparedCommit,
) -> Result<()> {
    verify_current_git_write_access(source, prepared)?;
    let current = observe_prepared_git_commit(source, prepared)?;
    let binding = source
        .registry
        .export_binding(source.directory_id)
        .map_err(|_| ServiceError::new("GIT_SNAPSHOT_UNAVAILABLE", "已授权仓库身份不可用"))?;
    super::git_commit_binding::verify_git_commit_dispatch(
        dispatch,
        resource_id,
        &binding.path,
        prepared,
        &current,
    )
}

pub(super) fn verify_current_git_write_access(
    source: &GitCommitSnapshotSource<'_>,
    prepared: &PreparedCommit,
) -> Result<()> {
    for path in prepared.paths() {
        match source.registry.check_with_exclusions(
            source.directory_id,
            path,
            source.system_access,
            wex_core::access::Access::Write,
            source.grants,
            source.internal_roots,
        ) {
            Ok(wex_core::access::Decision::Allowed) => {}
            Ok(wex_core::access::Decision::Denied(_)) => {
                return Err(ServiceError::new(
                    "PERMISSION_DENIED",
                    "Git 提交文件写入许可已失效",
                ))
            }
            Err(_) => {
                return Err(ServiceError::new(
                    "GIT_SNAPSHOT_UNAVAILABLE",
                    "Git 提交范围不可用",
                ))
            }
        }
    }
    Ok(())
}

fn observe_stable(
    source: &GitCommitSnapshotSource<'_>,
    paths: &[String],
) -> Result<RepositoryVersion> {
    let first = observe_once(source, paths)?;
    let second = observe_once(source, paths)?;
    if first != second {
        return Err(ServiceError::new(
            "VERSION_CONFLICT",
            "采集期间仓库、索引或文件版本发生变化",
        ));
    }
    Ok(second)
}

fn observe_once(
    source: &GitCommitSnapshotSource<'_>,
    paths: &[String],
) -> Result<RepositoryVersion> {
    observe_once_with_files(source, paths).map(|(version, _)| version)
}

// The version digest and returned bytes must be derived from the SAME reads;
// sandwich observations alone could approve transient, different staged bytes.
pub(super) fn read_prepared_files(
    source: &GitCommitSnapshotSource<'_>,
    prepared: &PreparedCommit,
) -> Result<Vec<wex_local_resources::RelativeFileSnapshot>> {
    verify_current_git_write_access(source, prepared)?;
    let (version, files) = observe_once_with_files(source, prepared.paths())?;
    prepared
        .recheck(&version)
        .map_err(|_| ServiceError::new("VERSION_CONFLICT", "准备树时批准的文件版本已变化"))?;
    let current = observe_prepared_git_commit(source, prepared)?;
    prepared
        .recheck(&current)
        .map_err(|_| ServiceError::new("VERSION_CONFLICT", "准备树期间仓库版本发生变化"))?;
    Ok(files)
}

fn observe_once_with_files(
    source: &GitCommitSnapshotSource<'_>,
    paths: &[String],
) -> Result<(
    RepositoryVersion,
    Vec<wex_local_resources::RelativeFileSnapshot>,
)> {
    use super::intent_binding::{digest_arguments, file_content_sha256};
    let read = |path: &str| {
        source
            .registry
            .read_relative_file_snapshot(
                source.directory_id,
                path,
                source.system_access,
                source.grants,
                source.internal_roots,
            )
            .map_err(|e| match e {
                wex_local_resources::FileReadError::Denied(_) => {
                    ServiceError::new("PERMISSION_DENIED", "Git 目录或文件读取未获授权")
                }
                wex_local_resources::FileReadError::Changed => {
                    ServiceError::new("VERSION_CONFLICT", "采集期间目录或文件发生变化")
                }
                wex_local_resources::FileReadError::TooLarge => {
                    ServiceError::new("TOOL_OUTPUT_LIMIT", "Git 元数据或文件超过当前安全读取上限")
                }
                _ => ServiceError::new(
                    "GIT_SNAPSHOT_UNAVAILABLE",
                    "Git 目录、元数据或文件不符合安全读取条件",
                ),
            })
    };
    let head = read(".git/HEAD")?;
    if head.directory_stamps.len() != 2 {
        return Err(ServiceError::new(
            "GIT_SNAPSHOT_UNAVAILABLE",
            "Git 目录身份缺失",
        ));
    }
    let text = std::str::from_utf8(&head.bytes)
        .map_err(|_| ServiceError::new("GIT_SNAPSHOT_UNAVAILABLE", "Git HEAD 编码无效"))?;
    let branch = text
        .strip_suffix('\n')
        .unwrap_or(text)
        .strip_prefix("ref: ")
        .ok_or_else(|| {
            ServiceError::new("GIT_SNAPSHOT_UNAVAILABLE", "当前采集器要求本地分支 HEAD")
        })?;
    let name = branch
        .strip_prefix("refs/heads/")
        .ok_or_else(|| ServiceError::new("GIT_SNAPSHOT_UNAVAILABLE", "HEAD 不是本地分支引用"))?;
    super::git_commit_policy::validate_commit_request("branch", &[name.into()])
        .map_err(|_| ServiceError::new("GIT_SNAPSHOT_UNAVAILABLE", "Git 分支引用路径无效"))?;
    let reference = read(&format!(".git/{branch}"))?;
    let index = read(".git/index")?;
    for metadata in [&reference, &index] {
        if !metadata
            .directory_stamps
            .starts_with(&head.directory_stamps)
        {
            return Err(ServiceError::new(
                "VERSION_CONFLICT",
                "Git 目录身份发生变化",
            ));
        }
    }
    let ref_text = std::str::from_utf8(&reference.bytes)
        .map_err(|_| ServiceError::new("GIT_SNAPSHOT_UNAVAILABLE", "Git 引用编码无效"))?;
    let oid = ref_text.strip_suffix('\n').unwrap_or(ref_text);
    if !matches!(oid.len(), 40 | 64)
        || !oid
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ServiceError::new(
            "GIT_SNAPSHOT_UNAVAILABLE",
            "Git 引用对象标识无效",
        ));
    }
    let mut identity_bytes = Vec::new();
    for stamp in &head.directory_stamps {
        for number in stamp {
            identity_bytes.extend_from_slice(&number.to_be_bytes());
        }
    }
    let identity = digest_arguments(
        "git_repository_identity",
        source.directory_id,
        &[std::env::consts::OS.as_bytes(), &identity_bytes],
    );
    let mut encoded = Vec::new();
    let mut files = Vec::new();
    for path in paths {
        let file = read(path)?;
        if file.directory_stamps.first() != head.directory_stamps.first() {
            return Err(ServiceError::new(
                "VERSION_CONFLICT",
                "项目根目录身份发生变化",
            ));
        }
        let mut parents = Vec::new();
        for stamp in &file.directory_stamps {
            for number in stamp {
                parents.extend_from_slice(&number.to_be_bytes());
            }
        }
        encoded.push(path.as_bytes().to_vec());
        encoded.push(vec![u8::from(file.executable)]);
        encoded.push(parents);
        encoded.push(file_content_sha256(&file.bytes).into_bytes());
        files.push(file);
    }
    let parts: Vec<_> = encoded.iter().map(Vec::as_slice).collect();
    Ok((
        RepositoryVersion {
            identity,
            head: Some(oid.into()),
            branch: branch.into(),
            index_sha256: file_content_sha256(&index.bytes),
            selected_sha256: digest_arguments(
                "git_selected_files_version",
                source.directory_id,
                &parts,
            ),
        },
        files,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};
    use wex_core::access::Access;
    use wex_local_resources::Protection;
    fn fixture() -> (tempfile::TempDir, DirectoryRegistry, String) {
        let dir = tempfile::tempdir().unwrap();
        assert!(Command::new("git")
            .args(["init", "--quiet", "--object-format=sha1", "-b", "work"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a"), "initial").unwrap();
        assert!(Command::new("git")
            .current_dir(dir.path())
            .args(["add", "--", "src/a"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(dir.path())
            .args([
                "-c",
                "user.name=WEX",
                "-c",
                "user.email=wex@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=",
                "commit",
                "--quiet",
                "-m",
                "initial"
            ])
            .status()
            .unwrap()
            .success());
        let mut registry = DirectoryRegistry::new();
        let id = registry.register(dir.path(), Protection::General).unwrap();
        (dir, registry, id)
    }
    #[test]
    fn real_repository_observation_changes_when_selected_bytes_or_index_change() {
        let (dir, registry, id) = fixture();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::Read,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let p = prepare_git_commit_from_registry(&source, "commit", &["src/a".into()]).unwrap();
        let before = observe_prepared_git_commit(&source, &p).unwrap();
        assert_eq!(p.recheck(&before), Ok(()));
        fs::write(dir.path().join("src/a"), "modified").unwrap();
        let changed = observe_prepared_git_commit(&source, &p).unwrap();
        assert_ne!(changed.selected_sha256, before.selected_sha256);
        assert_eq!(changed.index_sha256, before.index_sha256);
        assert!(Command::new("git")
            .current_dir(dir.path())
            .args(["add", "--", "src/a"])
            .status()
            .unwrap()
            .success());
        let staged = observe_prepared_git_commit(&source, &p).unwrap();
        assert_ne!(staged.index_sha256, changed.index_sha256);
    }
    #[test]
    fn rejects_missing_permission_before_repository_metadata_probe() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = DirectoryRegistry::new();
        let id = registry.register(dir.path(), Protection::General).unwrap();
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &[],
            internal_roots: &[],
        };
        assert_eq!(
            prepare_git_commit_from_registry(&source, "commit", &["a".into()])
                .unwrap_err()
                .code,
            "PERMISSION_DENIED"
        );
    }
    #[test]
    fn real_git_directory_replacement_changes_repository_identity() {
        let (dir, registry, id) = fixture();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::Read,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let p = prepare_git_commit_from_registry(&source, "commit", &["src/a".into()]).unwrap();
        let old = observe_prepared_git_commit(&source, &p).unwrap();
        fs::rename(dir.path().join(".git"), dir.path().join("old-git")).unwrap();
        // A second real repository at the same path cannot inherit identity.
        assert!(Command::new("git")
            .args(["init", "--quiet", "--object-format=sha1", "-b", "work"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        fs::copy(
            dir.path().join("old-git/index"),
            dir.path().join(".git/index"),
        )
        .unwrap();
        fs::create_dir_all(dir.path().join(".git/refs/heads")).unwrap();
        fs::copy(
            dir.path().join("old-git/refs/heads/work"),
            dir.path().join(".git/refs/heads/work"),
        )
        .unwrap();
        let new = observe_prepared_git_commit(&source, &p).unwrap();
        assert_ne!(new.identity, old.identity);
    }

    #[test]
    fn branch_change_and_revocation_reject_the_prepared_version() {
        let (dir, registry, id) = fixture();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::Read,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let p = prepare_git_commit_from_registry(&source, "commit", &["src/a".into()]).unwrap();
        assert!(Command::new("git")
            .current_dir(dir.path())
            .args(["branch", "other"])
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .current_dir(dir.path())
            .args(["symbolic-ref", "HEAD", "refs/heads/other"])
            .status()
            .unwrap()
            .success());
        let changed = observe_prepared_git_commit(&source, &p).unwrap();
        assert!(p.recheck(&changed).is_err());
        let denied = GitCommitSnapshotSource {
            grants: &[],
            ..source
        };
        assert_eq!(
            observe_prepared_git_commit(&denied, &p).unwrap_err().code,
            "PERMISSION_DENIED"
        );
    }

    #[cfg(unix)]
    #[test]
    fn git_directory_symlink_and_selected_mode_changes_are_detected() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (dir, registry, id) = fixture();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::Read,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let p = prepare_git_commit_from_registry(&source, "commit", &["src/a".into()]).unwrap();
        fs::set_permissions(dir.path().join("src/a"), fs::Permissions::from_mode(0o700)).unwrap();
        let changed = observe_prepared_git_commit(&source, &p).unwrap();
        assert!(p.recheck(&changed).is_err());
        fs::rename(dir.path().join(".git"), dir.path().join("git-alias")).unwrap();
        symlink(dir.path().join("git-alias"), dir.path().join(".git")).unwrap();
        assert!(observe_prepared_git_commit(&source, &p).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn owner_execute_change_is_detected_even_when_group_execute_is_already_set() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, registry, id) = fixture();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::Read,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        fs::set_permissions(dir.path().join("src/a"), fs::Permissions::from_mode(0o654)).unwrap();
        let p = prepare_git_commit_from_registry(&source, "commit", &["src/a".into()]).unwrap();
        fs::set_permissions(dir.path().join("src/a"), fs::Permissions::from_mode(0o754)).unwrap();
        let actual = observe_prepared_git_commit(&source, &p).unwrap();
        assert!(
            p.recheck(&actual).is_err(),
            "owner execute determines Git's executable mode"
        );
    }

    #[test]
    fn actual_core_dispatch_is_checked_against_fresh_registry_observation() {
        use wex_core::operation::{Action, ExecutionChecks, OperationIntent, OperationLedger};
        let (dir, registry, id) = fixture();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let p = prepare_git_commit_from_registry(&source, "commit", &["src/a".into()]).unwrap();
        let intent = OperationIntent {
            id: "op".into(),
            actor_id: "agent".into(),
            conversation_id: "chat".into(),
            context_id: None,
            connection_id: "local".into(),
            capability_id: "git_commit".into(),
            resource_id: "repo".into(),
            action: Action::Write,
            expected_version: p.expected_head().map(str::to_owned),
            intent_digest: crate::git_commit_intent_digest(
                "repo",
                &registry.export_binding(&id).unwrap().path,
                &p,
            )
            .unwrap(),
        };
        let mut ledger = OperationLedger::default();
        ledger.propose(intent.clone()).unwrap();
        ledger.approve("op", &intent, 1).unwrap();
        let dispatch = ledger
            .begin(
                "op",
                "attempt",
                ExecutionChecks {
                    authority_revision: 1,
                    connection_enabled: true,
                    authenticated: true,
                    platform_available: true,
                    resource_allowed: true,
                },
            )
            .unwrap();
        assert!(verify_live_git_commit_dispatch(&dispatch, "repo", &source, &p).is_ok());
        let read_only = [DirectoryGrant {
            directory_id: &id,
            access: Access::Read,
            explicit_personal: false,
        }];
        let reduced = GitCommitSnapshotSource {
            grants: &read_only,
            ..source
        };
        assert_eq!(
            verify_live_git_commit_dispatch(&dispatch, "repo", &reduced, &p)
                .unwrap_err()
                .code,
            "PERMISSION_DENIED"
        );
        fs::write(dir.path().join("src/a"), "newer content").unwrap();
        assert_eq!(
            verify_live_git_commit_dispatch(&dispatch, "repo", &source, &p)
                .unwrap_err()
                .code,
            "VERSION_CONFLICT"
        );
        let denied = GitCommitSnapshotSource {
            grants: &[],
            ..source
        };
        assert_eq!(
            verify_live_git_commit_dispatch(&dispatch, "repo", &denied, &p)
                .unwrap_err()
                .code,
            "PERMISSION_DENIED"
        );
    }
}
