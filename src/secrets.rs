// Purpose: Secure credential management using macOS Keychain via the security framework.
// Owner: Antigravity Agent
use std::process::Command;

const SERVICE_NAME: &str = "ReclassUtility";
const QBO_ACCESS_TOKEN_ACCOUNT: &str = "qbo_access_token";
const QBO_REFRESH_TOKEN_ACCOUNT: &str = "qbo_refresh_token";
const QBO_REALM_ID_ACCOUNT: &str = "qbo_realm_id";
const QBO_CLIENT_ID_ACCOUNT: &str = "qbo_client_id";
const QBO_CLIENT_SECRET_ACCOUNT: &str = "qbo_client_secret";

fn set_keychain_password(account: &str, password: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            SERVICE_NAME,
            "-a",
            account,
            "-w",
            password,
        ])
        .output()?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "macOS Security CLI failed to store {}: {}",
            account, err_msg
        )
        .into());
    }

    Ok(())
}

fn delete_keychain_password(account: &str) -> Result<(), Box<dyn std::error::Error>> {
    let _output = Command::new("security")
        .args(["delete-generic-password", "-s", SERVICE_NAME, "-a", account])
        .output()?;

    // We don't strictly check success here because it might already be deleted
    Ok(())
}

fn get_keychain_password(account: &str) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            SERVICE_NAME,
            "-a",
            account,
            "-w",
        ])
        .output()?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "macOS Security CLI failed to fetch {}: {}",
            account, err_msg
        )
        .into());
    }

    let secret = String::from_utf8(output.stdout)?
        .trim_end_matches('\n')
        .to_string();

    Ok(secret)
}

pub fn store_access_token(token: &str) -> Result<(), Box<dyn std::error::Error>> {
    set_keychain_password(QBO_ACCESS_TOKEN_ACCOUNT, token)
}

pub fn store_oauth_data(
    token: &str,
    refresh: &str,
    realm: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    set_keychain_password(QBO_ACCESS_TOKEN_ACCOUNT, token)?;
    set_keychain_password(QBO_REFRESH_TOKEN_ACCOUNT, refresh)?;
    set_keychain_password(QBO_REALM_ID_ACCOUNT, realm)?;
    Ok(())
}

pub fn delete_oauth_data() -> Result<(), Box<dyn std::error::Error>> {
    let _ = delete_keychain_password(QBO_ACCESS_TOKEN_ACCOUNT);
    let _ = delete_keychain_password(QBO_REFRESH_TOKEN_ACCOUNT);
    let _ = delete_keychain_password(QBO_REALM_ID_ACCOUNT);
    Ok(())
}

pub fn get_oauth_data() -> Result<(String, String, String), Box<dyn std::error::Error>> {
    Ok((
        get_keychain_password(QBO_ACCESS_TOKEN_ACCOUNT)?,
        get_keychain_password(QBO_REFRESH_TOKEN_ACCOUNT)?,
        get_keychain_password(QBO_REALM_ID_ACCOUNT)?,
    ))
}

pub fn store_client_credentials(
    client_id: &str,
    client_secret: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    set_keychain_password(QBO_CLIENT_ID_ACCOUNT, client_id)?;
    set_keychain_password(QBO_CLIENT_SECRET_ACCOUNT, client_secret)?;
    Ok(())
}

pub fn get_client_credentials() -> Result<(String, String), Box<dyn std::error::Error>> {
    Ok((
        get_keychain_password(QBO_CLIENT_ID_ACCOUNT)?,
        get_keychain_password(QBO_CLIENT_SECRET_ACCOUNT)?,
    ))
}
