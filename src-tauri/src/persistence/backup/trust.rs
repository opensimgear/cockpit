//! Retain validated public content while issuing an empty protected host identity.
use super::*;
pub(super) fn fresh_host(
    snapshot: &Database,
    seal: impl Fn(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<Option<Vec<u8>>, String> {
    let Some(content) = snapshot.read("companion", "publications")? else {
        return Ok(None);
    };
    let plain = osg_companion_service::Hub::fresh_restore_record(&content.payload)?;
    seal(&plain).map(Some)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_archive_requires_a_new_empty_host_and_never_copies_credentials() {
        let directory = crate::test_directory();
        let database = Database::open(directory.path().join("cockpit.sqlite3")).unwrap();
        assert!(fresh_host(&database, |_| panic!(
            "No first-use identity without public content"
        ))
        .unwrap()
        .is_none());
        let payload = b"{\"publications\":[],\"savedHeads\":{}}";
        database
            .commit(&[Change {
                domain: "companion",
                key: "publications",
                expected_revision: 0,
                payload: Some(payload),
            }])
            .unwrap();
        let first = fresh_host(&database, |bytes| Ok(bytes.to_vec()))
            .unwrap()
            .unwrap();
        let second = fresh_host(&database, |bytes| Ok(bytes.to_vec()))
            .unwrap()
            .unwrap();
        let first: Value = serde_json::from_slice(&first).unwrap();
        let second: Value = serde_json::from_slice(&second).unwrap();
        assert_ne!(first["hostId"], second["hostId"]);
        assert_eq!(first["credentials"], json!([]));
        assert_eq!(first["instances"], json!([]));
        assert_eq!(first.as_object().unwrap().len(), 3);
        assert!(fresh_host(&database, |_| Err("Protected store locked".into())).is_err());
        database
            .commit(&[Change {
                domain: "companion",
                key: "publications",
                expected_revision: 1,
                payload: Some(b"invalid archive"),
            }])
            .unwrap();
        assert!(fresh_host(&database, |_| panic!("Validate content before sealing")).is_err());
    }
}
