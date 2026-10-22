pub mod v33;
pub mod v34;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    V33_20260812,
    V34_010_0916,
    V34_010_0917,
}
impl Build {
    pub fn major_version(self) -> u16 {
        match self {
            Self::V33_20260812 => 33,
            Self::V34_010_0916 | Self::V34_010_0917 => 34,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::V33_20260812 => "20260812",
            Self::V34_010_0916 => "010-0916",
            Self::V34_010_0917 => "010-0917",
        }
    }
}
#[derive(Debug, thiserror::Error)]
#[error("Unsupported IIDX binary fingerprint: {0}")]
pub struct UnsupportedBuild(pub String);
pub fn resolve(fingerprint: &str) -> Result<Build, UnsupportedBuild> {
    match fingerprint {
        v33::b20260812::SUPPORTED_SHA256 => Ok(Build::V33_20260812),
        v34::b010_0916::SUPPORTED_SHA256 => Ok(Build::V34_010_0916),
        v34::b010_0917::SUPPORTED_SHA256 => Ok(Build::V34_010_0917),
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
        Build::V34_010_0916 => v34::b010_0916::attach(image, controls),
        Build::V34_010_0917 => v34::b010_0917::attach(image, controls),
    }
}
