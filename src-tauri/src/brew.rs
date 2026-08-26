use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::PathBuf, process::Command};
use tauri::Emitter;

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
    top_level: bool,
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

#[derive(Clone, Serialize)]
pub struct BrewProgress {
    operation: String,
    stage: String,
    current: usize,
    total: usize,
    percent: u8,
    item: Option<String>,
}

fn emit_progress(
    app: &tauri::AppHandle,
    operation: &str,
    stage: &str,
    current: usize,
    total: usize,
    percent: u8,
    item: Option<String>,
) {
    let _ = app.emit(
        "brew-progress",
        BrewProgress {
            operation: operation.to_string(),
            stage: stage.to_string(),
            current,
            total,
            percent,
            item,
        },
    );
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

fn parse_version_lines(
    text: &str,
    kind: &str,
    outdated: &HashSet<String>,
    top_level: &HashSet<String>,
) -> Vec<BrewPackage> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?.to_string();
            Some(BrewPackage {
                version: parts.collect::<Vec<_>>().join(", "),
                installed: true,
                outdated: outdated.contains(&name),
                top_level: top_level.contains(&name),
                name,
                kind: kind.to_string(),
                trusted: true,
                tap: None,
            })
        })
        .collect()
}

fn installed_formulae(
    outdated: &HashSet<String>,
    top_level: &HashSet<String>,
) -> Result<Vec<BrewPackage>, String> {
    Ok(parse_version_lines(
        &run_brew(&["list", "--formula", "--versions"])?,
        "formula",
        outdated,
        top_level,
    ))
}

fn direct_formula_dependencies(name: &str) -> Result<Vec<String>, String> {
    Ok(
        run_brew(&["deps", "--installed", "--direct", "--formula", name])?
            .split_whitespace()
            .map(str::to_string)
            .collect(),
    )
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

fn get_brew_status_sync() -> BrewStatus {
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

fn set_brew_source_sync(source: BrewSource) -> Result<String, String> {
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

fn list_brew_packages_sync() -> Result<Vec<BrewPackage>, String> {
    let top_level_formulae: HashSet<String> =
        run_brew(&["leaves"])?.lines().map(str::to_string).collect();
    let outdated_formula: HashSet<String> = run_brew(&["outdated", "--formula", "--quiet"])
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let mut packages = parse_version_lines(
        &run_brew(&["list", "--formula", "--versions"])?,
        "formula",
        &outdated_formula,
        &top_level_formulae,
    );

    // 版本列表会加载所有 cask 定义；改用安全的名称列表和本地安装收据，避免单项失败阻断页面。
    let trust = load_brew_trust();
    for name in installed_cask_names()? {
        let (version, tap) = cask_metadata(&name);
        let trusted = is_cask_trusted(&name, tap.as_deref(), &trust);
        // 逐个检查可信 cask，避免一个未信任项目让全部 cask 的更新状态丢失。
        let outdated = trusted
            && run_brew(&["outdated", "--cask", "--quiet", &name])
                .is_ok_and(|output| output.lines().any(|item| item == name));
        packages.push(BrewPackage {
            outdated,
            installed: true,
            version,
            kind: "cask".to_string(),
            name,
            trusted,
            tap,
            top_level: true,
        });
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packages)
}

fn get_brew_dependencies_sync(name: String, kind: String) -> Result<Vec<BrewPackage>, String> {
    validate_package(&name, &kind)?;
    if kind == "cask" {
        return Ok(Vec::new());
    }

    // 展开节点时只读取 formula 数据，避免重新扫描 cask、信任状态和全部过期软件。
    // 依赖名称来自独立的直接依赖命令，再与已安装 formula 版本表匹配。
    let empty = HashSet::new();
    let installed = installed_formulae(&empty, &empty)?;
    let by_name: std::collections::HashMap<&str, &BrewPackage> = installed
        .iter()
        .map(|item| (item.name.as_str(), item))
        .collect();
    let mut dependencies = direct_formula_dependencies(&name)?
        .into_iter()
        .filter_map(|dependency| by_name.get(dependency.as_str()).copied())
        .map(|item| BrewPackage {
            name: item.name.clone(),
            version: item.version.clone(),
            kind: item.kind.clone(),
            installed: true,
            outdated: false,
            trusted: true,
            tap: None,
            top_level: false,
        })
        .collect::<Vec<_>>();
    dependencies.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(dependencies)
}

fn search_brew_packages_sync(query: String) -> Result<Vec<BrewPackage>, String> {
    let query = query.trim();
    if query.len() < 2 || query.len() > 100 || query.starts_with('-') {
        return Err("请输入至少 2 个字符的软件名称".to_string());
    }
    let installed = list_brew_packages_sync().unwrap_or_default();
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
                            top_level: current.top_level,
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
                top_level: current.is_none_or(|item| item.top_level),
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
pub async fn get_brew_status() -> Result<BrewStatus, String> {
    tauri::async_runtime::spawn_blocking(get_brew_status_sync)
        .await
        .map_err(|error| format!("读取 Homebrew 状态失败：{error}"))
}

#[tauri::command]
pub async fn set_brew_source(app: tauri::AppHandle, source: BrewSource) -> Result<String, String> {
    emit_progress(&app, "source", "正在切换软件源", 0, 1, 15, None);
    let result = tauri::async_runtime::spawn_blocking(move || set_brew_source_sync(source))
        .await
        .map_err(|error| format!("切换软件源任务失败：{error}"))?;
    emit_progress(&app, "source", "软件源切换完成", 1, 1, 100, None);
    result
}

#[tauri::command]
pub async fn list_brew_packages(app: tauri::AppHandle) -> Result<Vec<BrewPackage>, String> {
    emit_progress(&app, "load", "正在读取顶层软件", 0, 3, 10, None);
    let result = tauri::async_runtime::spawn_blocking(list_brew_packages_sync)
        .await
        .map_err(|error| format!("读取软件列表任务失败：{error}"))?;
    emit_progress(&app, "load", "软件列表加载完成", 3, 3, 100, None);
    result
}

#[tauri::command]
pub async fn get_brew_dependencies(name: String, kind: String) -> Result<Vec<BrewPackage>, String> {
    tauri::async_runtime::spawn_blocking(move || get_brew_dependencies_sync(name, kind))
        .await
        .map_err(|error| format!("读取依赖任务失败：{error}"))?
}

#[tauri::command]
pub async fn search_brew_packages(query: String) -> Result<Vec<BrewPackage>, String> {
    tauri::async_runtime::spawn_blocking(move || search_brew_packages_sync(query))
        .await
        .map_err(|error| format!("搜索软件任务失败：{error}"))?
}

async fn run_package_action(
    app: tauri::AppHandle,
    action: &'static str,
    name: String,
    kind: String,
) -> Result<BrewOperationResult, String> {
    let stage = match action {
        "install" => "正在安装软件",
        "uninstall" => "正在卸载软件",
        _ => "正在更新软件",
    };
    emit_progress(&app, action, stage, 0, 1, 10, Some(name.clone()));
    let result = tauri::async_runtime::spawn_blocking(move || package_action(action, &name, &kind))
        .await
        .map_err(|error| format!("Homebrew 后台任务失败：{error}"))?;
    emit_progress(&app, action, "操作完成", 1, 1, 100, None);
    result
}

#[tauri::command]
pub async fn install_brew_package(
    app: tauri::AppHandle,
    name: String,
    kind: String,
) -> Result<BrewOperationResult, String> {
    run_package_action(app, "install", name, kind).await
}

#[tauri::command]
pub async fn uninstall_brew_package(
    app: tauri::AppHandle,
    name: String,
    kind: String,
) -> Result<BrewOperationResult, String> {
    run_package_action(app, "uninstall", name, kind).await
}

#[tauri::command]
pub async fn upgrade_brew_package(
    app: tauri::AppHandle,
    name: String,
    kind: String,
) -> Result<BrewOperationResult, String> {
    run_package_action(app, "upgrade", name, kind).await
}

fn upgrade_top_level_packages(app: &tauri::AppHandle) -> Result<BrewOperationResult, String> {
    emit_progress(app, "upgrade-all", "正在更新 Homebrew 索引", 0, 1, 5, None);
    let update_output = run_brew(&["update"])?;
    emit_progress(app, "upgrade-all", "正在检查顶层软件更新", 0, 1, 15, None);

    let packages = list_brew_packages_sync()?;
    let targets = packages
        .into_iter()
        .filter(|item| item.top_level && item.outdated && item.trusted)
        .collect::<Vec<_>>();
    let total = targets.len();
    if total == 0 {
        emit_progress(app, "upgrade-all", "所有顶层软件均为最新", 0, 0, 100, None);
        return Ok(BrewOperationResult {
            message: "所有顶层软件均为最新".to_string(),
            output: update_output,
        });
    }

    let mut outputs = vec![update_output];
    let mut failures = Vec::new();
    for (index, item) in targets.into_iter().enumerate() {
        let current = index + 1;
        let percent = 15 + ((current * 85 / total) as u8);
        emit_progress(
            app,
            "upgrade-all",
            "正在更新顶层软件",
            index,
            total,
            percent.saturating_sub(1),
            Some(item.name.clone()),
        );
        match package_action("upgrade", &item.name, &item.kind) {
            Ok(result) => outputs.push(result.output),
            Err(error) => failures.push(format!("{}：{}", item.name, error)),
        }
        emit_progress(
            app,
            "upgrade-all",
            "正在更新顶层软件",
            current,
            total,
            percent,
            Some(item.name),
        );
    }
    if !failures.is_empty() {
        outputs.push(format!("以下软件更新失败：\n{}", failures.join("\n")));
    }
    emit_progress(
        app,
        "upgrade-all",
        "顶层软件更新完成",
        total,
        total,
        100,
        None,
    );
    Ok(BrewOperationResult {
        message: if failures.is_empty() {
            format!("已更新 {total} 个顶层软件")
        } else {
            format!("顶层软件更新完成，{} 个失败", failures.len())
        },
        output: outputs
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
    })
}

#[tauri::command]
pub async fn upgrade_all_brew_packages(
    app: tauri::AppHandle,
) -> Result<BrewOperationResult, String> {
    let progress_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || upgrade_top_level_packages(&progress_app))
        .await
        .map_err(|error| format!("批量更新后台任务失败：{error}"))?
}
