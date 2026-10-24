use super::*;

#[test]
fn rejected_upload_metadata_never_initializes_services_or_retains_an_upload() {
    let runtime = runtime();
    for (document, options, expected) in [
        (json!({"canonicalDocument":"{}","publicationHash":"unused","assets":[],"unexpected":true}), json!({}), "Unexpected panel upload fields."),
        (json!({"canonicalDocument":"{}","publicationHash":"unused","assets":[]}), json!({"saveAsLatest":"yes"}), "saveAsLatest must be a boolean."),
    ] {
        let bytes = serde_json::to_vec(&document).unwrap();
        let id = call(&runtime, "companion_publish_begin", json!({"size":bytes.len()})).unwrap()["uploadId"].clone();
        call(&runtime, "companion_publish_chunk", json!({"uploadId":id,"offset":0,"bytes":bytes})).unwrap();
        let mut commit = options;
        commit["uploadId"] = id;
        assert_eq!(call(&runtime, "companion_publish_commit", commit).unwrap_err(), expected);
        assert!(runtime.upload.lock().unwrap().is_none());
        assert!(runtime.services.get().is_none());
    }
}
