pub mod v33;
pub mod v34;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    V33_20260812,
    V34_20260917,
}
impl Build {
    pub fn major_version(self) -> u16 {
        match self {
            Self::V33_20260812 => 33,
            Self::V34_20260917 => 34,
        }
    }
    pub fn date(self) -> &'static str {
        match self {
            Self::V33_20260812 => "20260812",
            Self::V34_20260917 => "20260917",
        }
    }
}
#[derive(Debug, thiserror::Error)]
#[error("Unsupported IIDX binary fingerprint: {0}")]
pub struct UnsupportedBuild(pub String);
pub fn resolve(fingerprint: &str) -> Result<Build, UnsupportedBuild> {
    match fingerprint {
        v33::b20260812::SUPPORTED_SHA256 => Ok(Build::V33_20260812),
        v34::b20260917::SUPPORTED_SHA256 => Ok(Build::V34_20260917),
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
        Build::V34_20260917 => v34::b20260917::attach(image, controls),
    }
}
