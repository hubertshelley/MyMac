//! 展示用的格式化辅助。

/// 字节数 → 人类可读（1024 进制）。
pub fn bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if value == 0 {
        return "0 B".to_string();
    }
    let i = ((value as f64).log2() / 10.0).floor() as usize;
    let i = i.min(UNITS.len() - 1);
    format!("{:.1} {}", value as f64 / 1024f64.powi(i as i32), UNITS[i])
}

#[cfg(test)]
mod tests {
    use super::bytes;

    #[test]
    fn formats_across_units() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(512), "512.0 B");
        assert_eq!(bytes(1024), "1.0 KB");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    }
}
