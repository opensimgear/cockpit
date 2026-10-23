//! Explicit fixture support, enabled only by native dev-dependencies. Exercises
//! real SQLITE_BUSY without exposing connections in the production service API.
use crate::{failure, Database};

pub struct WriteLock(rusqlite::Connection);
impl Drop for WriteLock {
    fn drop(&mut self) {
        let _ = self.0.execute_batch("ROLLBACK");
    }
}
pub fn hold_writer(database: &Database) -> Result<WriteLock, String> {
    let connection = database.connection()?;
    connection
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(failure)?;
    Ok(WriteLock(connection))
}
