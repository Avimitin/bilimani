//! Worker command boundary: IO here, state transitions in Service.
use crate::service::Service;
use bilimani_api::{ApiError, Command};
use serde_json::{Value, json};

/// Filesystem boundary shared by runtime and development preview. Parsing and
/// replacing aliases remains transactional inside Catalog.
pub fn execute(
    service: &mut Service,
    command: Command,
    now: u64,
    aliases_path: &std::path::Path,
) -> Result<Value, ApiError> {
    match command {
        Command::ReloadAliases => {
            let catalog = service.catalog.as_mut().ok_or(ApiError::NotReady)?;
            let bytes = crate::aliases::read(aliases_path)
                .map_err(|error| ApiError::AliasConfiguration(error.to_string()))?;
            let count = catalog
                .reload_aliases(&bytes)
                .map_err(|error| ApiError::AliasConfiguration(error.to_string()))?;
            Ok(json!({"count": count}))
        }
        command => service.command(command, now),
    }
}
