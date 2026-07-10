use crate::model::OcrError;

const SERVICE: &str = "cc.ccwu.paddledesk";
const ACCOUNT: &str = "paddleocr_access_token";

#[cfg(target_os = "windows")]
pub fn load_token() -> Result<String, OcrError> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|error| OcrError::Internal(format!("credential store unavailable: {error}")))?;
    entry.get_password().map_err(|error| match error {
        keyring::Error::NoEntry => OcrError::Auth,
        error => OcrError::Internal(format!("credential store read failed: {error}")),
    })
}

#[cfg(target_os = "windows")]
pub fn save_token(token: &str) -> Result<(), OcrError> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|error| OcrError::Internal(format!("credential store unavailable: {error}")))?;
    entry
        .set_password(token)
        .map_err(|error| OcrError::Internal(format!("credential store write failed: {error}")))
}

#[cfg(target_os = "windows")]
pub fn delete_token() -> Result<(), OcrError> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|error| OcrError::Internal(format!("credential store unavailable: {error}")))?;
    entry.delete_credential().map_err(|error| match error {
        keyring::Error::NoEntry => OcrError::Auth,
        error => OcrError::Internal(format!("credential store delete failed: {error}")),
    })
}

#[cfg(not(target_os = "windows"))]
pub fn load_token() -> Result<String, OcrError> {
    Err(OcrError::Auth)
}

#[cfg(not(target_os = "windows"))]
pub fn save_token(_token: &str) -> Result<(), OcrError> {
    Err(OcrError::Internal(
        "Windows Credential Manager is unavailable".into(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn delete_token() -> Result<(), OcrError> {
    Err(OcrError::Internal(
        "Windows Credential Manager is unavailable".into(),
    ))
}
