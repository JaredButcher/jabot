//! Daily off-site backups: the bot copies its database with `VACUUM INTO`, then uploads the
//! copy with restic (encrypted and compressed) to the repository in `RESTIC_REPOSITORY`.

mod snapshot;

pub use snapshot::{SnapshotError, check_integrity, snapshot};
