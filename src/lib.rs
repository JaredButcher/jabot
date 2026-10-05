//! Shared plumbing (`framework`) and the bot's capabilities (`features`).

// serenity::Error is large, and it flows through every Discord call (and every mock of one).
#![allow(clippy::result_large_err)]

pub mod backup;
pub mod features;
pub mod framework;
