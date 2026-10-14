//! DDR WORLD game integration. Platform connections live in the page.
pub mod aliases;
pub mod application;
pub mod catalog;
pub mod config;
pub mod ddr;
pub mod game;
pub mod host;
#[cfg(windows)]
mod runtime;
pub mod service;
pub mod versions;
