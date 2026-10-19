//! 应用目录约定与 Bundle ID 切换时的一次性迁移。
//!
//! M7 起正式 Bundle ID 为 `com.hubertshelley.mymac`（原 `com.mymac.manager`）。
//! 缓存目录按 Bundle ID 命名，首次启动时把旧目录整体搬过去；
//! `~/Library/Application Support/MyMac/` 与 `~/.mymac/` 不含 Bundle ID，不动。

use std::path::{Path, PathBuf};

/// 正式 Bundle ID
pub const BUNDLE_ID: &str = "com.hubertshelley.mymac";
/// 旧版（Tauri）Bundle ID，仅用于迁移
pub const LEGACY_BUNDLE_ID: &str = "com.mymac.manager";

/// `~/Library/Caches`
fn caches_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Caches"))
}

/// 应用缓存目录 `~/Library/Caches/com.hubertshelley.mymac`。
pub fn cache_dir() -> Option<PathBuf> {
    caches_root().map(|root| root.join(BUNDLE_ID))
}

/// 迁移结果
#[derive(Debug, PartialEq, Eq)]
pub enum CacheMigration {
    /// 没有旧目录，无需迁移
    NothingToMigrate,
    /// 新目录已存在（已迁移过或新版已写入），保留旧目录不动
    AlreadyPresent,
    /// 已把旧目录搬到新位置
    Moved,
}

/// 首次启动时把旧缓存目录搬到新 Bundle ID 下（幂等）。
pub fn migrate_legacy_cache() -> Result<CacheMigration, String> {
    let root = caches_root().ok_or("HOME 未设置")?;
    migrate_dir(&root.join(LEGACY_BUNDLE_ID), &root.join(BUNDLE_ID))
}

/// 把 `from` 整体移动到 `to`：`from` 不存在或 `to` 已存在时不做任何事。
/// 同一卷内 rename 是原子的；缓存目录都在 `~/Library/Caches` 下，不会跨卷。
fn migrate_dir(from: &Path, to: &Path) -> Result<CacheMigration, String> {
    if !from.is_dir() {
        return Ok(CacheMigration::NothingToMigrate);
    }
    if to.exists() {
        return Ok(CacheMigration::AlreadyPresent);
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建缓存父目录失败：{e}"))?;
    }
    std::fs::rename(from, to).map_err(|e| {
        format!(
            "迁移缓存目录失败（{} → {}）：{e}",
            from.display(),
            to.display()
        )
    })?;
    Ok(CacheMigration::Moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mymac-paths-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn moves_legacy_dir_with_contents() {
        let root = scratch("move");
        let from = root.join(LEGACY_BUNDLE_ID);
        let to = root.join(BUNDLE_ID);
        std::fs::create_dir_all(from.join("pins")).unwrap();
        std::fs::write(from.join("pins/a.png"), b"png").unwrap();

        assert_eq!(migrate_dir(&from, &to).unwrap(), CacheMigration::Moved);
        assert!(!from.exists());
        assert_eq!(std::fs::read(to.join("pins/a.png")).unwrap(), b"png");
        // 再次调用幂等
        assert_eq!(
            migrate_dir(&from, &to).unwrap(),
            CacheMigration::NothingToMigrate
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keeps_both_when_target_exists() {
        let root = scratch("exists");
        let from = root.join(LEGACY_BUNDLE_ID);
        let to = root.join(BUNDLE_ID);
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join("old.txt"), b"old").unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(to.join("new.txt"), b"new").unwrap();

        assert_eq!(
            migrate_dir(&from, &to).unwrap(),
            CacheMigration::AlreadyPresent
        );
        assert!(from.join("old.txt").exists());
        assert!(to.join("new.txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nothing_when_legacy_missing() {
        let root = scratch("missing");
        assert_eq!(
            migrate_dir(&root.join(LEGACY_BUNDLE_ID), &root.join(BUNDLE_ID)).unwrap(),
            CacheMigration::NothingToMigrate
        );
        assert!(!root.join(BUNDLE_ID).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
