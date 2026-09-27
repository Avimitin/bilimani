//! Host services; no game-specific IDs, offsets or layouts belong here.
#[cfg(windows)]
pub mod menu;
#[cfg(windows)]
pub mod spice;
#[cfg(windows)]
pub mod windows;
