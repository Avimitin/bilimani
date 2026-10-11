//! IIDX game service; platform and viewer policy lives in the page.
pub mod aliases;
pub mod application;
pub mod catalog;
pub mod config;
pub mod game;
pub mod host;
pub mod iidx;
#[cfg(windows)]
mod runtime;
pub mod service;
pub mod versions;
