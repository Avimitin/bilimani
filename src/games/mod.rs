//! Composition root: only this registry knows which game/version modules exist.
pub mod iidx;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Iidx33,
}
pub fn resolve(fingerprint: &str) -> Option<Profile> {
    match fingerprint {
        iidx::v33::SUPPORTED_SHA256 => Some(Profile::Iidx33),
        _ => None,
    }
}

#[cfg(windows)]
pub fn attach(
    config: &crate::config::Game,
    controls: &crate::config::Controls,
) -> anyhow::Result<Box<dyn crate::game::GameAdapter>> {
    let image = crate::host::windows::ModuleImage::load(&config.module)?;
    match resolve(&image.sha256) {
        Some(Profile::Iidx33) => iidx::v33::attach(image, controls.clone()),
        // Keep the existing unsupported-build error for existing installations.
        None => {
            anyhow::bail!("Unsupported bm2dx.dll build; hook disabled (see docs/game-analysis.md)")
        }
    }
}
