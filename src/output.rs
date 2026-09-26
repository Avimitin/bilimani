use anyhow::{Context, Result, ensure};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub struct TextFile {
    path: PathBuf,
    previous: Option<String>,
}
impl TextFile {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            previous: None,
        }
    }
    pub fn write(&mut self, text: &str) -> Result<()> {
        if self.previous.as_deref() == Some(text) {
            return Ok(());
        }
        atomic_write(&self.path, text.as_bytes())?;
        self.previous = Some(text.into());
        Ok(())
    }
}
pub fn resolved_output(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().context("Output path has no parent")?;
    std::fs::create_dir_all(parent)?;
    if path.exists() {
        ensure!(path.is_file(), "Output is not a file");
        return Ok(path.canonicalize()?);
    }
    Ok(parent
        .canonicalize()?
        .join(path.file_name().context("Missing output filename")?))
}
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().context("Output path has no parent")?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".chart-requester-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(contents)?;
        file.flush()?;
        drop(file);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};
            let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        #[cfg(not(windows))]
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.with_context(|| format!("Cannot update OBS file {}", path.display()))
}
