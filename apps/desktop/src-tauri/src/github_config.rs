//! GitHub credential reference is local metadata; PAT lives in Windows vault.
/// This service is independent from AI API connections and project associations.
use std::{fs, io::Write, path::PathBuf};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use wex_desktop_service::{Result, ServiceError};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    enabled: bool,
    credential_ref: String,
}

#[derive(Clone, Serialize)]
pub struct GithubStatus {
    pub configured: bool,
    pub enabled: bool,
}

fn location(app: &AppHandle) -> Result<PathBuf> {
    let root = app.path().app_data_dir()
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法定位 GitHub 配置目录"))?;
    Ok(root.join("github-connection.json"))
}

fn load(app: &AppHandle) -> Result<Option<Config>> {
    let path = location(app)?;
    match fs::read(&path) {
        Ok(bytes) => {
            let config: Config = serde_json::from_slice(&bytes)
                .map_err(|_| ServiceError::new("INVALID_CONFIG", "GitHub 配置文件已损坏"))?;
            if config.credential_ref.is_empty() {
                return Err(ServiceError::new("INVALID_CONFIG", "GitHub 凭证引用为空"));
            }
            Ok(Some(config))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ServiceError::new("STORAGE_ERROR", "无法读取 GitHub 配置")),
    }
}

fn persist(app: &AppHandle, config: &Config) -> Result<()> {
    let path = location(app)?;
    let dir = path.parent().ok_or_else(|| ServiceError::new("STORAGE_ERROR", "路径无效"))?;
    fs::create_dir_all(dir)
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法创建配置目录"))?;
    let temporary = dir.join(format!("github_{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut file = fs::OpenOptions::new().write(true).create_new(true)
        .open(&temporary).map_err(|_| ServiceError::new("STORAGE_ERROR", "无法写入 GitHub 配置"))?;
    let body = serde_json::to_vec(config)
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法编码 GitHub 配置"))?;
    file.write_all(&body).and_then(|_| file.sync_all())
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法持久化 GitHub 配置"))?;
    fs::rename(&temporary, &path)
        .map_err(|_| ServiceError::new("STORAGE_ERROR", "无法提交 GitHub 配置"))?;
    Ok(())
}

pub fn status(app: &AppHandle) -> Result<GithubStatus> {
    let config = load(app)?;
    Ok(GithubStatus {
        configured: config.is_some(),
        enabled: config.is_some_and(|v| v.enabled),
    })
}

pub fn configure(app: &AppHandle, token: &str) -> Result<()> {
    let token = token.trim();
    if token.is_empty() || token.len() > 4096 {
        return Err(ServiceError::new("INVALID_INPUT", "GitHub 凭证无效"));
    }
    let previous = load(app)?;
    let reference = format!("github_{}", uuid::Uuid::new_v4().simple());
    crate::vault::save(&reference, token)?;
    if let Err(error) = persist(app, &Config {
        enabled: false,
        credential_ref: reference.clone(),
    }) {
        crate::vault::remove(&reference);
        return Err(error);
    }
    if let Some(previous) = previous {
        crate::vault::remove(&previous.credential_ref);
    }
    Ok(())
}

pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<()> {
    let mut config = load(app)?
        .ok_or_else(|| ServiceError::new("CREDENTIAL_UNAVAILABLE", "请先配置 GitHub 凭证"))?;
    if enabled {
        crate::vault::read(&config.credential_ref)?;
    }
    config.enabled = enabled;
    persist(app, &config)
}

pub fn credential(app: &AppHandle) -> Result<String> {
    let config = load(app)?
        .filter(|config| config.enabled)
        .ok_or_else(|| ServiceError::new("PERMISSION_DENIED", "GitHub 总开关未开启"))?;
    crate::vault::read(&config.credential_ref)
}

/// Revoke future GitHub access and remove the stored credential reference.
/// In-flight external operations are not cancelled by this action.
pub fn revoke(app: &AppHandle) -> Result<()> {
    let Some(mut config) = load(app)? else {
        return Ok(());
    };
    // Fail closed before attempting credential deletion. If deletion fails,
    // leave the connection configured but disabled so the user can retry.
    if config.enabled {
        config.enabled = false;
        persist(app, &config)?;
    }
    crate::vault::remove_checked(&config.credential_ref)?;
    match fs::remove_file(location(app)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ServiceError::new("STORAGE_ERROR", "凭证已删除，但无法清除 GitHub 配置，请重试")),
    }
}
