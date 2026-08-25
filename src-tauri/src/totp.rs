use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Serialize, Deserialize)]
pub struct TotpAccount {
    pub id: String,
    pub name: String,
    pub issuer: String,
    pub secret: String,
    pub digits: u32,
    pub period: u64,
}

#[derive(Clone, Serialize)]
pub struct TotpAccountView {
    pub id: String,
    pub name: String,
    pub issuer: String,
    pub digits: u32,
    pub period: u64,
    pub code: String,
    pub remaining: u64,
}

pub struct TotpState {
    pub accounts: Mutex<Vec<TotpAccount>>,
}

fn accounts_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/MyMac/totp_accounts.json")
}

pub fn load_accounts() -> Vec<TotpAccount> {
    std::fs::read_to_string(accounts_path())
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_accounts(accounts: &[TotpAccount]) -> Result<(), String> {
    let path = accounts_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(accounts).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn normalize_secret(secret: &str) -> Result<String, String> {
    let normalized: String = secret
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if normalized.is_empty() {
        return Err("请输入密钥".to_string());
    }
    BASE32_NOPAD
        .decode(normalized.as_bytes())
        .map_err(|_| "密钥不是有效的 Base32 格式".to_string())?;
    Ok(normalized)
}

fn generate_code(account: &TotpAccount, now: u64) -> Result<String, String> {
    let key = BASE32_NOPAD
        .decode(account.secret.as_bytes())
        .map_err(|_| "账户密钥无效".to_string())?;
    let counter = now / account.period;
    let mut mac = Hmac::<Sha1>::new_from_slice(&key).map_err(|e| e.to_string())?;
    mac.update(&counter.to_be_bytes());
    let bytes = mac.finalize().into_bytes();
    let offset = (bytes[19] & 0x0f) as usize;
    let binary = ((bytes[offset] as u32 & 0x7f) << 24)
        | ((bytes[offset + 1] as u32) << 16)
        | ((bytes[offset + 2] as u32) << 8)
        | bytes[offset + 3] as u32;
    let modulo = 10_u32.pow(account.digits);
    Ok(format!(
        "{:0width$}",
        binary % modulo,
        width = account.digits as usize
    ))
}

fn view(account: &TotpAccount, now: u64) -> Result<TotpAccountView, String> {
    Ok(TotpAccountView {
        id: account.id.clone(),
        name: account.name.clone(),
        issuer: account.issuer.clone(),
        digits: account.digits,
        period: account.period,
        code: generate_code(account, now)?,
        remaining: account.period - (now % account.period),
    })
}

fn parse_otpauth(input: &str) -> Result<(String, String, String, u32, u64), String> {
    let parsed = url::Url::parse(input).map_err(|_| "otpauth 链接格式无效".to_string())?;
    if parsed.scheme() != "otpauth" || parsed.host_str() != Some("totp") {
        return Err("仅支持 otpauth://totp 链接".to_string());
    }
    let label = parsed.path().trim_start_matches('/');
    let label = url::form_urlencoded::parse(label.as_bytes())
        .map(|(k, v)| format!("{k}{v}"))
        .collect::<String>();
    let mut secret = None;
    let mut issuer_param = None;
    let mut digits = 6;
    let mut period = 30;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "secret" => secret = Some(value.into_owned()),
            "issuer" => issuer_param = Some(value.into_owned()),
            "digits" => digits = value.parse().map_err(|_| "验证码位数无效".to_string())?,
            "period" => period = value.parse().map_err(|_| "刷新周期无效".to_string())?,
            "algorithm" if !value.eq_ignore_ascii_case("SHA1") => {
                return Err("当前仅支持 SHA1 算法".to_string())
            }
            _ => {}
        }
    }
    let (label_issuer, name) = label
        .split_once(':')
        .map(|(issuer, name)| (issuer.trim().to_string(), name.trim().to_string()))
        .unwrap_or_else(|| (String::new(), label.trim().to_string()));
    let issuer = issuer_param.unwrap_or(label_issuer);
    if name.is_empty() {
        return Err("链接中缺少账户名称".to_string());
    }
    let secret = normalize_secret(&secret.ok_or("链接中缺少密钥")?)?;
    validate_options(digits, period)?;
    Ok((name, issuer, secret, digits, period))
}

fn validate_options(digits: u32, period: u64) -> Result<(), String> {
    if !matches!(digits, 6 | 8) {
        return Err("验证码位数仅支持 6 位或 8 位".to_string());
    }
    if !(5..=300).contains(&period) {
        return Err("刷新周期必须在 5 到 300 秒之间".to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn get_totp_accounts(state: tauri::State<TotpState>) -> Result<Vec<TotpAccountView>, String> {
    let now = timestamp();
    state
        .accounts
        .lock()
        .unwrap()
        .iter()
        .map(|a| view(a, now))
        .collect()
}

#[tauri::command]
pub fn add_totp_account(
    state: tauri::State<TotpState>,
    name: String,
    issuer: String,
    secret: String,
    digits: u32,
    period: u64,
) -> Result<TotpAccountView, String> {
    let (name, issuer, secret, digits, period) = if secret.trim().starts_with("otpauth://") {
        parse_otpauth(secret.trim())?
    } else {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("请输入账户名称".to_string());
        }
        validate_options(digits, period)?;
        (
            name,
            issuer.trim().to_string(),
            normalize_secret(&secret)?,
            digits,
            period,
        )
    };
    let account = TotpAccount {
        id: scru128::new_string(),
        name,
        issuer,
        secret,
        digits,
        period,
    };
    let result = view(&account, timestamp())?;
    let mut accounts = state.accounts.lock().unwrap();
    accounts.push(account);
    save_accounts(&accounts)?;
    Ok(result)
}

#[tauri::command]
pub fn delete_totp_account(state: tauri::State<TotpState>, id: String) -> Result<(), String> {
    let mut accounts = state.accounts.lock().unwrap();
    let old_len = accounts.len();
    accounts.retain(|a| a.id != id);
    if accounts.len() == old_len {
        return Err("账户不存在".to_string());
    }
    save_accounts(&accounts)
}

pub fn copy_code(
    totp_state: &TotpState,
    clipboard_state: &crate::clipboard::ClipboardState,
    id: &str,
) -> Result<String, String> {
    let account = totp_state
        .accounts
        .lock()
        .unwrap()
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or("账户不存在")?;
    let code = generate_code(&account, timestamp())?;
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_text(code.clone())
        .map_err(|e| e.to_string())?;
    *clipboard_state.last_seen.lock().unwrap() = Some(code.clone());
    Ok(code)
}

#[tauri::command]
pub fn copy_totp_code(
    totp_state: tauri::State<TotpState>,
    clipboard_state: tauri::State<crate::clipboard::ClipboardState>,
    id: String,
) -> Result<String, String> {
    copy_code(totp_state.inner(), clipboard_state.inner(), &id)
}

pub fn menu_entries(state: &TotpState) -> Vec<(String, String)> {
    let now = timestamp();
    state
        .accounts
        .lock()
        .unwrap()
        .iter()
        .filter_map(|account| {
            generate_code(account, now).ok().map(|code| {
                let label = if account.issuer.is_empty() {
                    format!("{}  {}", account.name, code)
                } else {
                    format!("{} · {}  {}", account.issuer, account.name, code)
                };
                (account.id.clone(), label)
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_rfc_6238_sha1_code() {
        let account = TotpAccount {
            id: String::new(),
            name: "test".to_string(),
            issuer: String::new(),
            secret: "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string(),
            digits: 8,
            period: 30,
        };
        assert_eq!(generate_code(&account, 59).unwrap(), "94287082");
    }

    #[test]
    fn parses_standard_otpauth_uri() {
        let parsed = parse_otpauth(
            "otpauth://totp/GitHub:alice%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=GitHub",
        )
        .unwrap();
        assert_eq!(parsed.0, "alice@example.com");
        assert_eq!(parsed.1, "GitHub");
        assert_eq!(parsed.2, "JBSWY3DPEHPK3PXP");
        assert_eq!(parsed.3, 6);
        assert_eq!(parsed.4, 30);
    }
}
