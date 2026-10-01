//! tell: users get a token with `/tell`, then `POST <base path>/tell` with it to DM themselves,
//! e.g. when a long-running task finishes.

mod model;
mod repo;

pub use repo::{SqliteTellRepo, TellRepo};
