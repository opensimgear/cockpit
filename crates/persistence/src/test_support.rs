//! Explicit fixture support, enabled only by native dev-dependencies. Exercises
//! real SQLITE_BUSY without exposing connections in the production service API.
use crate::{failure, Database};

/// Deliberately bypass domain decoding and bounds to qualify corrupt/stale
/// recovery evidence without adding a production escape hatch.
pub fn read_evidence(anchor:&std::path::Path,domain:&str)->Result<Vec<u8>,String>{
    let key=crate::namespace::evidence_key(anchor)?;
    Database::for_namespace(anchor)?.read(domain,&key)?.map(|r|r.payload).ok_or("No recovery evidence.".into())
}
pub fn write_evidence(anchor:&std::path::Path,domain:&str,payload:Option<&[u8]>)->Result<(),String>{
    let key=crate::namespace::evidence_key(anchor)?;
    let database=Database::for_namespace(anchor)?;
    database.commit(&[crate::Change{domain,key:&key,expected_revision:database.revision(domain,&key)?,payload}])?;
    Ok(())
}

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
