use crate::config::{Config, LogLevel, Logging};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub struct Logger {
    path: PathBuf,
    options: Logging,
    secrets: Vec<String>,
}
impl Logger {
    pub fn reconfigure(&mut self, root: &Path, config: &Config) {
        let mut replacement = Self::new(root, config);
        replacement.secrets.extend(self.secrets.iter().cloned());
        replacement
            .secrets
            .sort_by_key(|s| std::cmp::Reverse(s.len()));
        replacement.secrets.dedup();
        *self = replacement;
    }
    pub fn new(root: &Path, config: &Config) -> Self {
        let mut secrets = config.source().redaction_secrets();
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        // Callers use Debug formatting for quoted chat fields; also redact its escaped form.
        let escaped: Vec<_> = secrets
            .iter()
            .map(|s| s.escape_debug().to_string())
            .collect();
        secrets.extend(escaped);
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        secrets.dedup();
        Self {
            path: root.join("chart-requester.log"),
            options: config.logging.clone(),
            secrets,
        }
    }
    pub fn info(&self, category: &str, message: &str) {
        if self.options.level != LogLevel::Off {
            self.write("INFO", category, message);
        }
    }
    pub fn debug(&self, category: &str, message: &str) {
        if self.options.level == LogLevel::Debug {
            self.write("DEBUG", category, message);
        }
    }
    pub fn danmu(&self, message: &str) {
        if self.options.danmu {
            self.debug("danmu", message);
        }
    }
    fn write(&self, level: &str, category: &str, message: &str) {
        // No log failure may interrupt game or chat processing.
        let _ = self.append(level, category, message);
    }
    fn append(&self, level: &str, category: &str, message: &str) -> std::io::Result<()> {
        let text = sanitize(message, &self.secrets);
        let line = format!("[{}] {level} [{category}] {text}\n", timestamp());
        if fs::metadata(&self.path)
            .is_ok_and(|m| m.len() + line.len() as u64 > self.options.max_file_mb * 1024 * 1024)
        {
            let backup = |n| self.path.with_file_name(format!("chart-requester.log.{n}"));
            let oldest = backup(self.options.backups);
            if oldest.exists() {
                fs::remove_file(oldest)?;
            }
            for n in (1..self.options.backups).rev() {
                let from = backup(n);
                if from.exists() {
                    fs::rename(from, backup(n + 1))?;
                }
            }
            fs::rename(&self.path, backup(1))?;
        }
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?
            .write_all(line.as_bytes())
    }
}

fn sanitize(message: &str, secrets: &[String]) -> String {
    let mut text = message.to_owned();
    for secret in secrets {
        text = text.replace(secret, "[REDACTED]");
    }
    let mut out = String::new();
    for c in text.chars().take(4096) {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    if text.chars().count() > 4096 {
        out.push_str(" [truncated]");
    }
    out
}

#[cfg(windows)]
fn timestamp() -> String {
    let mut t = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}
#[cfg(not(windows))]
fn timestamp() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03} UTC", t.as_secs(), t.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logging_redacts_credentials_escapes_lines_and_rotates() {
        let root = std::env::temp_dir().join(format!("requester-log-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let mut cfg = Config::default();
        cfg.bilibili.auth_code = "private-identity-code".into();
        cfg.bilibili.sessdata = "private-cookie".into();
        cfg.logging.max_file_mb = 1;
        cfg.logging.backups = 2;
        let logger = Logger::new(&root, &cfg);
        logger.danmu("点歌 AA private-identity-code\nFAKE LOG private-cookie");
        let text = fs::read_to_string(&logger.path).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("点歌 AA [REDACTED]\\nFAKE LOG [REDACTED]"));
        assert!(!text.contains("private-"));
        assert!(text.contains("DEBUG [danmu]"));
        for n in 1..=3 {
            fs::write(&logger.path, vec![b'x'; 1024 * 1024]).unwrap();
            logger.info("rotation", &format!("pass {n}"));
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
        assert!(fs::read_to_string(&logger.path).unwrap().contains("pass 3"));
        for entry in fs::read_dir(&root).unwrap() {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn logging_options_suppress_chat_content_and_debug_output() {
        let root = std::env::temp_dir().join(format!("requester-log-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let mut cfg = Config::default();
        cfg.logging.danmu = false;
        let logger = Logger::new(&root, &cfg);
        logger.danmu("hidden chat");
        assert!(!logger.path.exists());
        cfg.logging.level = LogLevel::Info;
        let logger = Logger::new(&root, &cfg);
        logger.debug("transport", "hidden debug");
        logger.info("connection", "connected");
        let text = fs::read_to_string(&logger.path).unwrap();
        assert!(!text.contains("hidden"));
        assert!(text.contains("connected"));
        fs::remove_file(&logger.path).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
