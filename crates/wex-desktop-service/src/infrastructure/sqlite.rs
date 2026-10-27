use crate::*;
use rusqlite::{params, Connection, TransactionBehavior};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    path::Path,
    time::Duration,
};
pub struct DesktopStore {
    db: Connection,
    _instance_lock: Option<File>,
}
fn storage(_: impl std::fmt::Display) -> ServiceError {
    ServiceError::new(
        "STORAGE_ERROR",
        "无法读写项目数据，请检查目录权限和磁盘空间",
    )
}
impl DesktopStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(storage)?;
        }
        let guard = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .map_err(storage)?;
        fs2::FileExt::try_lock_exclusive(&guard)
            .map_err(|_| ServiceError::new("APP_IN_USE", "该数据目录已被另一个 WEXPC 进程使用"))?;
        Self::init(Connection::open(path).map_err(storage)?, Some(guard))
    }
    pub fn memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(storage)?, None)
    }
    fn init(mut db: Connection, guard: Option<File>) -> Result<Self> {
        db.busy_timeout(Duration::from_secs(5)).map_err(storage)?;
        let version: u32 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(storage)?;
        if version > 8 {
            return Err(ServiceError::new(
                "UNSUPPORTED_VERSION",
                "数据由更新版本创建，请使用相应版本打开",
            ));
        }
        if version == 0 {
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(storage)?;
            tx.execute_batch("CREATE TABLE desktop_state (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);").map_err(storage)?;
            tx.execute(
                "INSERT INTO desktop_state (id,body) VALUES(1,?1)",
                [serde_json::to_string(&DesktopSnapshot::default()).map_err(storage)?],
            )
            .map_err(storage)?;
            tx.pragma_update(None, "user_version", 8).map_err(storage)?;
            tx.commit().map_err(storage)?;
        }
        if (1..=7).contains(&version) {
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(storage)?;
            let body: String = tx
                .query_row("SELECT body FROM desktop_state WHERE id=1", [], |r| {
                    r.get(0)
                })
                .map_err(storage)?;
            let _: DesktopSnapshot = serde_json::from_str(&body).map_err(|_| {
                ServiceError::new("INVALID_SNAPSHOT", "旧项目数据无法迁移，未覆盖原数据")
            })?;
            // Defaulted fields are read additively. Keep the exact old payload,
            // but fence out older binaries before any write can erase records.
            tx.pragma_update(None, "user_version", 8).map_err(storage)?;
            tx.commit().map_err(storage)?;
        }
        Ok(Self {
            db,
            _instance_lock: guard,
        })
    }
    pub fn load(&self) -> Result<DesktopSnapshot> {
        let body: String = self
            .db
            .query_row("SELECT body FROM desktop_state WHERE id=1", [], |row| {
                row.get(0)
            })
            .map_err(storage)?;
        let mut snapshot: DesktopSnapshot = serde_json::from_str(&body)
            .map_err(|_| ServiceError::new("INVALID_SNAPSHOT", "项目数据无法解析，未覆盖原数据"))?;
        snapshot.unsaved_turn_ids.clear();
        Ok(snapshot)
    }
    pub fn update<T>(
        &mut self,
        change: impl FnOnce(&mut DesktopSnapshot) -> Result<T>,
    ) -> Result<T> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let body: String = tx
            .query_row("SELECT body FROM desktop_state WHERE id=1", [], |row| {
                row.get(0)
            })
            .map_err(storage)?;
        let mut s: DesktopSnapshot = serde_json::from_str(&body).map_err(storage)?;
        let result = change(&mut s)?;
        s.revision = s
            .revision
            .checked_add(1)
            .ok_or_else(|| storage("revision"))?;
        // Typed snapshots intentionally omit legacy collections. Preserve unknown
        // top-level data without exposing it as current trusted execution facts.
        // RawValue keeps opaque numbers and nested JSON from being normalized.
        let mut persisted: BTreeMap<String, Box<serde_json::value::RawValue>> =
            serde_json::from_str(&body).map_err(storage)?;
        let current = serde_json::to_string(&s).map_err(storage)?;
        let updated: BTreeMap<String, Box<serde_json::value::RawValue>> =
            serde_json::from_str(&current).map_err(storage)?;
        persisted.extend(updated);
        let body = serde_json::to_string(&persisted).map_err(storage)?;
        tx.execute("UPDATE desktop_state SET body=?1 WHERE id=1", params![body])
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}

#[cfg(test)]
mod publication_schema_tests {
    use super::*;

    #[test]
    fn newly_created_store_uses_schema_eight() {
        let store = DesktopStore::memory().unwrap();
        let version: u32 = store
            .db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 8);
    }

    #[test]
    fn previous_schemas_upgrade_without_replacing_desktop_snapshot() {
        for old_version in [6, 7] {
            let db = Connection::open_in_memory().unwrap();
            db.execute_batch(&format!(
                "CREATE TABLE desktop_state (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL); PRAGMA user_version={old_version};"
            )).unwrap();
            let prior = serde_json::json!({
                "revision": 42,
                "connections": [],
                "projects": [],
                "agents": [],
                "tasks": [],
                "conversations": [],
                "turns": []
            })
            .to_string();
            db.execute("INSERT INTO desktop_state(id,body) VALUES (1,?1)", [&prior])
                .unwrap();
            let store = DesktopStore::init(db, None).unwrap();
            let version: u32 = store
                .db
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, 8);
            assert_eq!(store.load().unwrap().revision, 42);
            let preserved: String = store
                .db
                .query_row("SELECT body FROM desktop_state WHERE id=1", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(preserved, prior);
        }
    }
}
