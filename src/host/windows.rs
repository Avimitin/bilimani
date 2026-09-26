use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use windows_sys::Win32::{
    Foundation::HMODULE,
    System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
};

#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn module_path(module: HMODULE) -> Result<PathBuf> {
    let mut buf = vec![0u16; 32768];
    let len = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    ensure!(len > 0 && len < buf.len(), "Cannot determine module path");
    use std::os::windows::ffi::OsStringExt;
    Ok(std::ffi::OsString::from_wide(&buf[..len]).into())
}
pub struct ModuleImage {
    pub base: usize,
    pub sha256: String,
}
impl ModuleImage {
    pub fn load(name: &str) -> Result<Self> {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let module = unsafe { GetModuleHandleW(wide.as_ptr()) };
        ensure!(!module.is_null(), "Game DLL not loaded: {name}");
        let file = std::fs::read(module_path(module)?).context("Cannot fingerprint game DLL")?;
        Ok(Self {
            base: module as usize,
            sha256: hex::encode(Sha256::digest(file)),
        })
    }
}
