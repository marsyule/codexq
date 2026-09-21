//! CodexQ Pure Rust Core Engine.
//!
//! Provides SQLite WAL persistence, JWT token decoding, profile sandboxing,
//! `codex app-server` rate limit probing, atomic switching, and warmup scheduling.

pub mod auth;
pub mod config;
pub mod db;
pub mod paths;
pub mod probe;
pub mod process;
pub mod scheduler;
pub mod switch;
pub mod warmup;
pub mod doctor;
pub mod provider;

#[cfg(test)]
mod tests;
