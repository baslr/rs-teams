use keyring::Entry;

use crate::auth::token::TokenSet;
use crate::error::AppError;

const SERVICE: &str = "com.teams-rs.auth";
const ACCOUNT: &str = "token_set";

pub fn store_tokens(tokens: &TokenSet) -> Result<(), AppError> {
    let json = serde_json::to_string(tokens)?;
    let entry = Entry::new(SERVICE, ACCOUNT)?;
    entry.set_password(&json)?;
    Ok(())
}

pub fn load_tokens() -> Result<Option<TokenSet>, AppError> {
    let entry = Entry::new(SERVICE, ACCOUNT)?;
    match entry.get_password() {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn clear_tokens() -> Result<(), AppError> {
    let entry = Entry::new(SERVICE, ACCOUNT)?;
    let _ = entry.delete_credential();
    Ok(())
}
