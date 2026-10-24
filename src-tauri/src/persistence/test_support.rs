//! Raw records for corruption and failure fixtures. Production callers use typed stores.
pub(crate) fn record(path:&std::path::Path,domain:&str,key:&str,payload:&[u8]) {
    let db=osg_persistence::Database::for_namespace(path).unwrap();
    db.commit(&[osg_persistence::Change{domain,key,expected_revision:db.revision(domain,key).unwrap(),payload:Some(payload)}]).unwrap();
}
