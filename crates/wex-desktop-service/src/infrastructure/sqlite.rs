use crate::*;
use rusqlite::{params, Connection, TransactionBehavior};
use std::{
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
        if version > 3 {
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
            tx.pragma_update(None, "user_version", 3).map_err(storage)?;
            tx.commit().map_err(storage)?;
        }
        if version == 1 || version == 2 {
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
            // Defaulted organization fields are read additively. Keep the exact
            // old payload, but fence out v1 binaries before any new data write.
            tx.pragma_update(None, "user_version", 3).map_err(storage)?;
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
        let body = serde_json::to_string(&s).map_err(storage)?;
        tx.execute("UPDATE desktop_state SET body=?1 WHERE id=1", params![body])
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}
