use base64::Engine;
use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Serialize, Deserialize)]
pub struct TotpAccount {
    pub id: String,
    pub name: String,
    pub issuer: String,
    #[serde(default, skip_serializing)]
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

const KEYRING_SERVICE: &str = "com.mymac.manager.totp";
const KEYRING_VAULT_ACCOUNT: &str = "totp-secret-vault";

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn accounts_path() -> PathBuf {
    home_dir().join(".mymac/totp_accounts.json")
}

fn legacy_accounts_path() -> PathBuf {
    home_dir().join("Library/Application Support/MyMac/totp_accounts.json")
}

fn keyring_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, account)
        .map_err(|e| format!("无法访问 macOS 钥匙串：{e}"))
}

fn load_secret_vault() -> Result<Option<HashMap<String, String>>, String> {
    match keyring_entry(KEYRING_VAULT_ACCOUNT)?.get_password() {
        Ok(content) => serde_json::from_str(&content)
            .map(Some)
            .map_err(|e| format!("2FA 密钥库格式无效：{e}")),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("无法从 macOS 钥匙串读取 2FA 密钥库：{e}")),
    }
}

fn save_secret_vault(vault: &HashMap<String, String>) -> Result<(), String> {
    let content = serde_json::to_string(vault).map_err(|e| e.to_string())?;
    keyring_entry(KEYRING_VAULT_ACCOUNT)?
        .set_password(&content)
        .map_err(|e| format!("无法保存 2FA 密钥库到 macOS 钥匙串：{e}"))
}

fn delete_legacy_secret(id: &str) -> Result<(), String> {
    match keyring_entry(id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("无法删除旧钥匙串项目：{e}")),
    }
}

fn read_legacy_secret(id: &str) -> Result<String, String> {
    keyring_entry(id)?
        .get_password()
        .map_err(|e| format!("无法读取旧钥匙串项目：{e}"))
}

fn migrate_per_account_keyring(accounts: &[TotpAccount]) -> Result<HashMap<String, String>, String> {
    let mut vault = HashMap::new();
    for account in accounts {
        vault.insert(account.id.clone(), read_legacy_secret(&account.id)?);
    }
    save_secret_vault(&vault)?;
    for account in accounts {
        if let Err(e) = delete_legacy_secret(&account.id) {
            eprintln!("{e}");
        }
    }
    Ok(vault)
}

pub fn load_accounts() -> Vec<TotpAccount> {
    match load_accounts_inner() {
        Ok(accounts) => accounts,
        Err(e) => {
            eprintln!("2FA 账户加载失败：{e}");
            Vec::new()
        }
    }
}

fn load_accounts_inner() -> Result<Vec<TotpAccount>, String> {
    let path = accounts_path();
    if !path.exists() && legacy_accounts_path().exists() {
        migrate_legacy_accounts()?;
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut accounts: Vec<TotpAccount> = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let vault = match load_secret_vault()? {
        Some(vault) => vault,
        None if accounts.is_empty() => HashMap::new(),
        None => migrate_per_account_keyring(&accounts)?,
    };
    for account in &mut accounts {
        account.secret = vault
            .get(&account.id)
            .cloned()
            .ok_or_else(|| format!("账户“{}”的密钥不存在", account.name))?;
    }
    Ok(accounts)
}

fn migrate_legacy_accounts() -> Result<(), String> {
    let legacy_path = legacy_accounts_path();
    let content = std::fs::read_to_string(&legacy_path).map_err(|e| e.to_string())?;
    let accounts: Vec<TotpAccount> = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let vault: HashMap<String, String> = accounts
        .iter()
        .map(|account| (account.id.clone(), account.secret.clone()))
        .collect();
    save_secret_vault(&vault)?;
    if let Err(e) = save_accounts(&accounts) {
        let _ = keyring_entry(KEYRING_VAULT_ACCOUNT).and_then(|entry| {
            entry
                .delete_credential()
                .map_err(|error| error.to_string())
        });
        return Err(e);
    }
    std::fs::remove_file(&legacy_path).map_err(|e| format!("迁移完成但无法删除旧密钥文件：{e}"))?;
    Ok(())
}

fn save_accounts(accounts: &[TotpAccount]) -> Result<(), String> {
    let path = accounts_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
    }
    let content = serde_json::to_string_pretty(accounts).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, content).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    std::fs::rename(temp, path).map_err(|e| e.to_string())
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

fn validate_qr_payload(payload: &str) -> Result<String, String> {
    let payload = payload.trim();
    if payload.to_lowercase().starts_with("otpauth://") {
        parse_otpauth(payload)?;
        return Ok(payload.to_string());
    }
    normalize_secret(payload).map_err(|_| "二维码中没有有效的 2FA 密钥或 otpauth 链接".to_string())
}

fn decode_qr_image(bytes: &[u8]) -> Result<String, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|_| "无法读取图片，请选择 PNG 或 JPEG 截图".to_string())?
        .to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare(image);
    for grid in prepared.detect_grids() {
        if let Ok((_meta, payload)) = grid.decode() {
            if let Ok(valid) = validate_qr_payload(&payload) {
                return Ok(valid);
            }
        }
    }
    Err("未识别到有效的 2FA 二维码，请确保二维码清晰且完整".to_string())
}

#[tauri::command]
pub fn decode_totp_qr_image(data: String) -> Result<String, String> {
    let encoded = data
        .split_once(',')
        .map(|(_, value)| value)
        .unwrap_or(&data);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| "图片数据无效".to_string())?;
    decode_qr_image(&bytes)
}

#[tauri::command]
pub fn capture_totp_qr() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let path = std::env::temp_dir().join(format!("mymac-totp-{}.png", scru128::new_string()));
        let status = std::process::Command::new("/usr/sbin/screencapture")
            .arg("-i")
            .arg("-x")
            .arg(&path)
            .status()
            .map_err(|e| format!("无法启动系统截图：{e}"))?;
        if !status.success() || !path.exists() {
            return Err("已取消截图".to_string());
        }
        let result = std::fs::read(&path)
            .map_err(|e| format!("无法读取截图：{e}"))
            .and_then(|bytes| decode_qr_image(&bytes));
        let _ = std::fs::remove_file(path);
        result
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("当前仅支持在 macOS 上直接截图识别".to_string())
    }
}

#[tauri::command]
pub fn decode_totp_qr_clipboard() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let image = clipboard
        .get_image()
        .map_err(|_| "剪贴板中没有图片，请先复制二维码截图".to_string())?;
    let rgba = image::RgbaImage::from_raw(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )
    .ok_or("剪贴板图片数据无效")?;
    let dynamic = image::DynamicImage::ImageRgba8(rgba);
    let mut cursor = std::io::Cursor::new(Vec::new());
    dynamic
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    decode_qr_image(cursor.get_ref())
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
    let mut vault: HashMap<String, String> = accounts
        .iter()
        .map(|item| (item.id.clone(), item.secret.clone()))
        .collect();
    vault.insert(account.id.clone(), account.secret.clone());
    save_secret_vault(&vault)?;
    accounts.push(account.clone());
    if let Err(e) = save_accounts(&accounts) {
        accounts.pop();
        vault.remove(&account.id);
        let _ = save_secret_vault(&vault);
        return Err(e);
    }
    Ok(result)
}

#[tauri::command]
pub fn delete_totp_account(state: tauri::State<TotpState>, id: String) -> Result<(), String> {
    let mut accounts = state.accounts.lock().unwrap();
    let position = accounts
        .iter()
        .position(|account| account.id == id)
        .ok_or("账户不存在")?;
    let removed = accounts.remove(position);
    if let Err(e) = save_accounts(&accounts) {
        accounts.insert(position, removed);
        return Err(e);
    }
    let vault: HashMap<String, String> = accounts
        .iter()
        .map(|account| (account.id.clone(), account.secret.clone()))
        .collect();
    if let Err(e) = save_secret_vault(&vault) {
        accounts.insert(position, removed);
        let _ = save_accounts(&accounts);
        return Err(e);
    }
    Ok(())
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

    #[test]
    fn decodes_otpauth_qr_image() {
        let payload =
            "otpauth://totp/GitHub:alice%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=GitHub";
        let code = qrcode::QrCode::new(payload.as_bytes()).unwrap();
        let width = code.width() as u32;
        let scale = 8;
        let quiet = 4;
        let size = (width + quiet * 2) * scale;
        let mut qr_image = image::GrayImage::from_pixel(size, size, image::Luma([255]));
        for y in 0..width {
            for x in 0..width {
                if code[(x as usize, y as usize)] == qrcode::Color::Dark {
                    for py in 0..scale {
                        for px in 0..scale {
                            qr_image.put_pixel(
                                (x + quiet) * scale + px,
                                (y + quiet) * scale + py,
                                image::Luma([0]),
                            );
                        }
                    }
                }
            }
        }
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(qr_image)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        assert_eq!(decode_qr_image(png.get_ref()).unwrap(), payload);
    }
}
