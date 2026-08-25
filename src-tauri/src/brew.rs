use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::PathBuf, process::Command};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrewSource {
    Official,
    Tsinghua,
    Ustc,
}

impl Default for BrewSource {
    fn default() -> Self {
        Self::Official
    }
}

#[derive(Default, Deserialize, Serialize)]
struct BrewConfig {
    source: BrewSource,
}

#[derive(Serialize)]
pub struct BrewStatus {
    installed: bool,
    path: String,
    version: String,
    source: BrewSource,
}

#[derive(Serialize)]
pub struct BrewPackage {
    name: String,
    version: String,
    kind: String,
    installed: bool,
    outdated: bool,
    trusted: bool,
    tap: Option<String>,
}

#[derive(Default, Deserialize)]
struct BrewTrust {
    taps: Vec<String>,
    casks: Vec<String>,
}

#[derive(Serialize)]
pub struct BrewOperationResult {
    message: String,
    output: String,
}

fn config_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".mymac").join("brew_config.json")
}

fn load_config() -> BrewConfig {
    fs::read_to_string(config_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_config(config: &BrewConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建配置目录：{e}"))?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(config).map_err(|e| format!("无法生成配置：{e}"))?,
    )
    .map_err(|e| format!("无法保存软件源设置：{e}"))
}

fn brew_path() -> Option<PathBuf> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .or_else(|| {
            let output = Command::new("/usr/bin/which").arg("brew").output().ok()?;
            if !output.status.success() {
                return None;
            }
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (!path.is_empty()).then(|| PathBuf::from(path))
        })
}

fn source_env(command: &mut Command, source: &BrewSource) {
    match source {
        BrewSource::Official => {}
        BrewSource::Tsinghua => {
            command
                .env(
                    "HOMEBREW_API_DOMAIN",
                    "https://mirrors.tuna.tsinghua.edu.cn/homebrew-bottles/api",
                )
                .env(
                    "HOMEBREW_BOTTLE_DOMAIN",
                    "https://mirrors.tuna.tsinghua.edu.cn/homebrew-bottles",
                );
        }
        BrewSource::Ustc => {
            command
                .env(
                    "HOMEBREW_API_DOMAIN",
                    "https://mirrors.ustc.edu.cn/homebrew-bottles/api",
                )
                .env(
                    "HOMEBREW_BOTTLE_DOMAIN",
                    "https://mirrors.ustc.edu.cn/homebrew-bottles",
                );
        }
    }
}

fn run_brew(args: &[&str]) -> Result<String, String> {
    let path = brew_path().ok_or_else(|| "尚未安装 Homebrew".to_string())?;
    let mut command = Command::new(path);
    command.args(args).env("HOMEBREW_NO_AUTO_UPDATE", "1");
    source_env(&mut command, &load_config().source);
    let output = command
        .output()
        .map_err(|e| format!("无法运行 Homebrew：{e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(if stderr.is_empty() {
            format!("Homebrew 操作失败（{}）", output.status)
        } else {
            stderr
        })
    }
}

fn validate_package(name: &str, kind: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 200
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '.' | '@' | '/'))
    {
        return Err("软件名称不合法".to_string());
    }
    if !matches!(kind, "formula" | "cask") {
        return Err("软件类型不合法".to_string());
    }
    Ok(())
}

fn parse_version_lines(text: &str, kind: &str, outdated: &HashSet<String>) -> Vec<BrewPackage> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?.to_string();
            Some(BrewPackage {
                version: parts.collect::<Vec<_>>().join(", "),
                installed: true,
                outdated: outdated.contains(&name),
                name,
                kind: kind.to_string(),
                trusted: true,
                tap: None,
            })
        })
        .collect()
}

fn brew_prefix() -> Option<PathBuf> {
    brew_path()?.parent()?.parent().map(PathBuf::from)
}

fn installed_cask_names() -> Result<Vec<String>, String> {
    Ok(run_brew(&["list", "--cask", "-1"])?
        .split_whitespace()
        .map(str::to_string)
        .collect())
}

fn load_brew_trust() -> BrewTrust {
    run_brew(&["trust", "--json=v1"])
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn cask_receipt(name: &str) -> Option<serde_json::Value> {
    let path = brew_prefix()?
        .join("Caskroom")
        .join(name)
        .join(".metadata")
        .join("INSTALL_RECEIPT.json");
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn cask_metadata(name: &str) -> (String, Option<String>) {
    let receipt = cask_receipt(name);
    let version = receipt
        .as_ref()
        .and_then(|value| value.get("source"))
        .and_then(|source| source.get("version"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let tap = receipt
        .as_ref()
        .and_then(|value| value.get("source"))
        .and_then(|source| source.get("tap"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    (version, tap)
}

fn is_official_tap(tap: &str) -> bool {
    matches!(tap, "homebrew/core" | "homebrew/cask")
}

fn is_cask_trusted(name: &str, tap: Option<&str>, trust: &BrewTrust) -> bool {
    tap.is_none_or(|tap| {
        is_official_tap(tap)
            || trust.taps.iter().any(|trusted| trusted == tap)
            || trust
                .casks
                .iter()
                .any(|trusted| trusted == name || trusted == &format!("{tap}/{name}"))
    })
}

fn untrusted_cask_from_error(error: &str) -> Option<(String, String)> {
    let full_name = error
        .split_whitespace()
        .skip_while(|part| *part != "cask")
        .nth(1)?;
    let mut parts = full_name.rsplitn(2, '/');
    let name = parts
        .next()?
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
    let tap = parts.next()?;
    if name.is_empty() || tap.split('/').count() != 2 {
        return None;
    }
    Some((name.to_string(), tap.to_string()))
}

#[tauri::command]
pub fn get_brew_status() -> BrewStatus {
    let source = load_config().source;
    match brew_path() {
        Some(path) => {
            let version = run_brew(&["--version"])
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            BrewStatus {
                installed: true,
                path: path.to_string_lossy().to_string(),
                version,
                source,
            }
        }
        None => BrewStatus {
            installed: false,
            path: String::new(),
            version: String::new(),
            source,
        },
    }
}

#[tauri::command]
pub fn start_brew_install() -> Result<String, String> {
    if brew_path().is_some() {
        return Err("Homebrew 已经安装".to_string());
    }
    let script = r#"tell application "Terminal"
activate
do script "/bin/bash -c '$(/usr/bin/curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)'"
end tell"#;
    let status = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .status()
        .map_err(|e| format!("无法打开终端：{e}"))?;
    if status.success() {
        Ok("已在终端中启动 Homebrew 安装，请按终端提示完成后返回刷新".to_string())
    } else {
        Err("无法在终端中启动安装流程".to_string())
    }
}

fn source_urls(source: &BrewSource) -> (&'static str, &'static str, &'static str) {
    match source {
        BrewSource::Official => (
            "https://github.com/Homebrew/brew.git",
            "https://github.com/Homebrew/homebrew-core.git",
            "https://github.com/Homebrew/homebrew-cask.git",
        ),
        BrewSource::Tsinghua => (
            "https://mirrors.tuna.tsinghua.edu.cn/git/homebrew/brew.git",
            "https://mirrors.tuna.tsinghua.edu.cn/git/homebrew/homebrew-core.git",
            "https://mirrors.tuna.tsinghua.edu.cn/git/homebrew/homebrew-cask.git",
        ),
        BrewSource::Ustc => (
            "https://mirrors.ustc.edu.cn/brew.git",
            "https://mirrors.ustc.edu.cn/homebrew-core.git",
            "https://mirrors.ustc.edu.cn/homebrew-cask.git",
        ),
    }
}

fn set_remote(repository: &str, url: &str) -> Result<(), String> {
    let output = Command::new("/usr/bin/git")
        .args(["-C", repository, "remote", "set-url", "origin", url])
        .output()
        .map_err(|e| format!("无法调整仓库地址：{e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[tauri::command]
pub fn set_brew_source(source: BrewSource) -> Result<String, String> {
    let (brew_url, core_url, cask_url) = source_urls(&source);
    let brew_repo = run_brew(&["--repository"])?;
    set_remote(&brew_repo, brew_url)?;
    for (tap, url) in [("homebrew/core", core_url), ("homebrew/cask", cask_url)] {
        if let Ok(repo) = run_brew(&["--repository", tap]) {
            if !repo.trim().is_empty() {
                set_remote(repo.trim(), url)?;
            }
        }
    }
    save_config(&BrewConfig { source })?;
    Ok("软件源已切换".to_string())
}

#[tauri::command]
pub fn list_brew_packages() -> Result<Vec<BrewPackage>, String> {
    let outdated_formula: HashSet<String> = run_brew(&["outdated", "--formula", "--quiet"])
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    // 未信任的第三方 cask 会令整个命令失败，此时只放弃 cask 更新状态，继续加载列表。
    let outdated_cask: HashSet<String> = run_brew(&["outdated", "--cask", "--quiet"])
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let mut packages = parse_version_lines(
        &run_brew(&["list", "--formula", "--versions"])?,
        "formula",
        &outdated_formula,
    );

    // 版本列表会加载所有 cask 定义；改用安全的名称列表和本地安装收据，避免单项失败阻断页面。
    let trust = load_brew_trust();
    for name in installed_cask_names()? {
        let (version, tap) = cask_metadata(&name);
        let trusted = is_cask_trusted(&name, tap.as_deref(), &trust);
        packages.push(BrewPackage {
            outdated: trusted && outdated_cask.contains(&name),
            installed: true,
            version,
            kind: "cask".to_string(),
            name,
            trusted,
            tap,
        });
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packages)
}

#[tauri::command]
pub fn search_brew_packages(query: String) -> Result<Vec<BrewPackage>, String> {
    let query = query.trim();
    if query.len() < 2 || query.len() > 100 || query.starts_with('-') {
        return Err("请输入至少 2 个字符的软件名称".to_string());
    }
    let installed = list_brew_packages().unwrap_or_default();
    let mut results = Vec::new();
    for (kind, flag) in [("formula", "--formula"), ("cask", "--cask")] {
        let output = match run_brew(&["search", flag, query]) {
            Ok(output) => output,
            Err(error) if kind == "cask" => {
                if let Some((name, tap)) = untrusted_cask_from_error(&error) {
                    if let Some(current) = installed
                        .iter()
                        .find(|item| item.kind == "cask" && item.name == name && !item.trusted)
                    {
                        results.push(BrewPackage {
                            name,
                            version: current.version.clone(),
                            kind: "cask".to_string(),
                            installed: true,
                            outdated: false,
                            trusted: false,
                            tap: Some(tap),
                        });
                        continue;
                    }
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        for name in output
            .lines()
            .filter(|line| !line.starts_with("==>"))
            .flat_map(str::split_whitespace)
        {
            if validate_package(name, kind).is_err() {
                continue;
            }
            let current = installed
                .iter()
                .find(|item| item.name == name && item.kind == kind);
            results.push(BrewPackage {
                name: name.to_string(),
                version: current.map(|item| item.version.clone()).unwrap_or_default(),
                kind: kind.to_string(),
                installed: current.is_some(),
                outdated: current.is_some_and(|item| item.outdated),
                trusted: current.is_none_or(|item| item.trusted),
                tap: current.and_then(|item| item.tap.clone()),
            });
        }
    }
    results.sort_by(|a, b| a.name.cmp(&b.name).then(a.kind.cmp(&b.kind)));
    results.dedup_by(|a, b| a.name == b.name && a.kind == b.kind);
    Ok(results)
}

fn package_action(action: &str, name: &str, kind: &str) -> Result<BrewOperationResult, String> {
    validate_package(name, kind)?;
    let flag = if kind == "cask" {
        "--cask"
    } else {
        "--formula"
    };
    let output = run_brew(&[action, flag, name])?;
    Ok(BrewOperationResult {
        message: match action {
            "install" => format!("已安装 {name}"),
            "uninstall" => format!("已卸载 {name}"),
            _ => format!("已更新 {name}"),
        },
        output,
    })
}

#[tauri::command]
pub fn install_brew_package(name: String, kind: String) -> Result<BrewOperationResult, String> {
    package_action("install", &name, &kind)
}

#[tauri::command]
pub fn uninstall_brew_package(name: String, kind: String) -> Result<BrewOperationResult, String> {
    package_action("uninstall", &name, &kind)
}

#[tauri::command]
pub fn upgrade_brew_package(name: String, kind: String) -> Result<BrewOperationResult, String> {
    package_action("upgrade", &name, &kind)
}

#[tauri::command]
pub fn upgrade_all_brew_packages() -> Result<BrewOperationResult, String> {
    let update = run_brew(&["update"])?;
    let upgrade = run_brew(&["upgrade"])?;
    let cask = run_brew(&["upgrade", "--cask"]).unwrap_or_else(|error| error);
    Ok(BrewOperationResult {
        message: "Homebrew 和全部软件已更新".to_string(),
        output: [update, upgrade, cask]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
    })
}
