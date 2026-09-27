use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use windows_sys::Win32::{
    Foundation::HMODULE,
    System::{
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleExW, GetModuleHandleW,
        },
        Memory::{
            MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE, PAGE_EXECUTE_READ,
            PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, VirtualQuery,
        },
    },
};

pub fn executable_address(address: usize) -> bool {
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    (unsafe {
        VirtualQuery(
            address as *const _,
            &mut info,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    }) != 0
        && info.State == MEM_COMMIT
        && info.Protect & PAGE_GUARD == 0
        && matches!(
            info.Protect & 0xff,
            PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
        )
}

/// Resolve only already-loaded modules, without loading or pinning another DLL.
pub fn code_module(address: usize) -> Option<(usize, String)> {
    let mut module = std::ptr::null_mut();
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address as *const u16,
            &mut module,
        )
    } == 0
    {
        return None;
    }
    let path = module_path(module).ok()?;
    Some((
        module as usize,
        path.file_name()?.to_string_lossy().into_owned(),
    ))
}

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
