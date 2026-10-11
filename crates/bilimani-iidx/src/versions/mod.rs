pub mod v33;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    V33_20260812,
}
#[derive(Debug, thiserror::Error)]
#[error("Unsupported IIDX binary fingerprint: {0}")]
pub struct UnsupportedBuild(pub String);
pub fn resolve(fingerprint: &str) -> Result<Build, UnsupportedBuild> {
    match fingerprint {
        v33::b20260812::SUPPORTED_SHA256 => Ok(Build::V33_20260812),
        other => Err(UnsupportedBuild(other.into())),
    }
}
#[cfg(windows)]
pub fn attach(
    module: &str,
    controls: &crate::config::Controls,
) -> anyhow::Result<Box<dyn crate::game::GameAdapter>> {
    let image = crate::host::windows::ModuleImage::load(module)?;
    match resolve(&image.sha256)? {
        Build::V33_20260812 => v33::b20260812::attach(image, controls),
    }
}
