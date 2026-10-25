use wex_desktop_service::{Result, ServiceError};

#[cfg(windows)]
fn entry(reference: &str) -> Result<keyring::Entry> {
    keyring::Entry::new("WEXPC", reference).map_err(|_| unavailable())
}
fn unavailable() -> ServiceError {
    ServiceError::new(
        "CREDENTIAL_UNAVAILABLE",
        "Windows 凭证存储不可用，未以明文保存密钥",
    )
}

pub fn save(reference: &str, secret: &str) -> Result<()> {
    #[cfg(windows)]
    {
        entry(reference)?
            .set_password(secret)
            .map_err(|_| unavailable())
    }
    #[cfg(not(windows))]
    {
        let _ = (reference, secret);
        Err(unavailable())
    }
}

pub fn read(reference: &str) -> Result<String> {
    #[cfg(windows)]
    {
        entry(reference)?.get_password().map_err(|_| unavailable())
    }
    #[cfg(not(windows))]
    {
        let _ = reference;
        Err(unavailable())
    }
}

pub fn remove(reference: &str) {
    #[cfg(windows)]
    if let Ok(entry) = entry(reference) {
        let _ = entry.delete_credential();
    }
    #[cfg(not(windows))]
    let _ = reference;
}

/// Unlike best-effort cleanup, explicit user revocation must report failure.
pub fn remove_checked(reference: &str) -> Result<()> {
    #[cfg(windows)]
    {
        entry(reference)?
            .delete_credential()
            .map_err(|_| unavailable())
    }
    #[cfg(not(windows))]
    {
        let _ = reference;
        Err(unavailable())
    }
}
