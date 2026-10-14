use super::*;
use crate::fixtures::profile;
fn call(
    service: &Documents,
    now: Instant,
    owner: &str,
    command: &str,
    mut request: Value,
) -> Result<Value, String> {
    request["clientId"] = json!(owner);
    request["command"] = json!(command);
    service.request_at(&request, now)
}
fn begin(service: &Documents, now: Instant, owner: &str, size: usize) -> String {
    call(
        service,
        now,
        owner,
        "mapping_import_begin",
        json!({"size":size}),
    )
    .unwrap()["uploadId"]
        .as_str()
        .unwrap()
        .into()
}
fn upload(service: &Documents, now: Instant, bytes: &[u8]) -> String {
    let id = begin(service, now, "client", bytes.len());
    for (index, chunk) in bytes.chunks(CHUNK_LIMIT).enumerate() {
        call(
            service,
            now,
            "client",
            "mapping_import_chunk",
            json!({"uploadId":id,"offset":index * CHUNK_LIMIT,"bytes":chunk}),
        )
        .unwrap();
    }
    id
}
#[test]
fn full_document_upload_roundtrips_without_creating_or_starting_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let service = Documents::new(Some(temp.path().into()));
    let now = Instant::now();
    let mut bytes = profile().export().unwrap();
    bytes.resize(DOCUMENT_LIMIT, b' ');
    let id = upload(&service, now, &bytes);
    assert!(service.store.as_ref().unwrap().ids().unwrap().is_empty());
    let result = call(
        &service,
        now,
        "client",
        "mapping_import_commit",
        json!({"uploadId":id}),
    )
    .unwrap();
    assert_eq!(result["saved"], true);
    assert!(result.to_string().len() < DOCUMENT_LIMIT + 1024);
    let exported = call(
        &service,
        now,
        "client",
        "mapping_export",
        json!({"profileId":profile().id}),
    )
    .unwrap();
    assert_eq!(result["document"], exported["document"]);
    assert!(exported["document"].is_object());
    let page = call(
        &service,
        now,
        "client",
        "mapping_profiles",
        json!({"offset":0}),
    )
    .unwrap();
    assert_eq!(page["ids"], json!([profile().id]));
    assert_eq!(page["next"], Value::Null);
    assert!(call(
        &service,
        now,
        "client",
        "mapping_profiles",
        json!({"offset":2})
    )
    .is_err());
    assert!(call(
        &service,
        now,
        "client",
        "mapping_import_commit",
        json!({"uploadId":id})
    )
    .is_err());
    let validation = Documents::new(None);
    let id = upload(&validation, now, &bytes);
    let validated = call(
        &validation,
        now,
        "client",
        "mapping_validate_commit",
        json!({"uploadId":id}),
    )
    .unwrap();
    assert_eq!(validated["saved"], false);
    assert_eq!(validated["document"], exported["document"]);
}
#[test]
fn uploads_are_owned_ordered_atomic_bounded_and_expire() {
    let service = Documents::new(None);
    let now = Instant::now();
    for size in [0, DOCUMENT_LIMIT + 1] {
        assert!(call(
            &service,
            now,
            "client",
            "mapping_import_begin",
            json!({"size":size})
        )
        .is_err());
    }
    let id = begin(&service, now, "client", 3);
    assert!(call(
        &service,
        now,
        "client",
        "mapping_import_begin",
        json!({"size":3})
    )
    .is_err());
    for command in [
        "mapping_import_commit",
        "mapping_import_abort",
        "mapping_import_chunk",
    ] {
        assert!(call(
            &service,
            now,
            "other",
            command,
            json!({"uploadId":id,"offset":0,"bytes":[1]})
        )
        .is_err());
    }
    for (offset, bytes) in [
        (1, json!([1])),
        (0, json!([])),
        (0, json!([1, 2, 3, 4])),
        (0, json!([1, 256])),
        (0, json!([1, -1])),
        (0, json!([1, 1.5])),
        (0, json!([1, true])),
    ] {
        assert!(call(
            &service,
            now,
            "client",
            "mapping_import_chunk",
            json!({"uploadId":id,"offset":offset,"bytes":bytes})
        )
        .is_err());
    }
    assert_eq!(service.uploads.lock().unwrap()[&id].bytes.len(), 0);
    call(
        &service,
        now,
        "client",
        "mapping_import_chunk",
        json!({"uploadId":id,"offset":0,"bytes":[1]}),
    )
    .unwrap();
    assert!(call(
        &service,
        now,
        "client",
        "mapping_import_commit",
        json!({"uploadId":id})
    )
    .is_err());
    for owner in ["two", "three", "four"] {
        begin(&service, now, owner, DOCUMENT_LIMIT);
    }
    assert!(call(
        &service,
        now,
        "five",
        "mapping_import_begin",
        json!({"size":1})
    )
    .is_err());
    assert!(call(
        &service,
        now + TTL,
        "client",
        "mapping_import_abort",
        json!({"uploadId":id})
    )
    .is_err());
    let large = begin(&service, now + TTL, "client", DOCUMENT_LIMIT);
    assert!(call(
        &service,
        now + TTL,
        "client",
        "mapping_import_chunk",
        json!({"uploadId":large,"offset":0,"bytes":vec![0; CHUNK_LIMIT+1]})
    )
    .is_err());
    call(
        &service,
        now + TTL,
        "client",
        "mapping_import_abort",
        json!({"uploadId":large}),
    )
    .unwrap();
    assert!(service.uploads.lock().unwrap().is_empty());
    begin(&service, now + TTL, "client", 1);
    service.clear();
    assert!(service.uploads.lock().unwrap().is_empty());
}
#[test]
fn malformed_documents_requests_and_disabled_storage_fail_closed() {
    let service = Documents::new(None);
    let now = Instant::now();
    assert!(service.request(&Value::Null).is_err());
    assert!(service
        .request(&json!({"padding":"x".repeat(DOCUMENT_LIMIT)}))
        .is_err());
    assert!(service.request(&json!({})).is_err());
    for request in [json!({"size":-1}), json!({"size":"1"}), json!({"size":1.5})] {
        assert!(call(&service, now, "client", "mapping_import_begin", request).is_err());
    }
    for command in ["mapping_profiles", "mapping_export", "unknown"] {
        assert!(call(
            &service,
            now,
            "client",
            command,
            json!({"offset":0,"profileId":profile().id})
        )
        .is_err());
    }
    let id = begin(&service, now, "client", 1);
    assert!(call(
        &service,
        now,
        "client",
        "mapping_import_chunk",
        json!({"uploadId":id,"offset":0,"bytes":"!"})
    )
    .is_err());
    call(
        &service,
        now,
        "client",
        "mapping_import_chunk",
        json!({"uploadId":id,"offset":0,"bytes":[255]}),
    )
    .unwrap();
    assert!(call(
        &service,
        now,
        "client",
        "mapping_validate_commit",
        json!({"uploadId":id})
    )
    .is_err());
    let id = upload(&service, now, &profile().export().unwrap());
    assert!(call(
        &service,
        now,
        "client",
        "mapping_import_commit",
        json!({"uploadId":id})
    )
    .is_err());
    let temp = tempfile::tempdir().unwrap();
    let saved = Documents::new(Some(temp.path().into()));
    for i in 0..33 {
        let mut p = profile();
        p.id = uuid::Uuid::from_u128(i + 1).to_string();
        saved.store.as_ref().unwrap().save(&p).unwrap();
    }
    let first = call(
        &saved,
        now,
        "client",
        "mapping_profiles",
        json!({"offset":0}),
    )
    .unwrap();
    assert_eq!(first["ids"].as_array().unwrap().len(), 32);
    assert_eq!(first["next"], 32);
    let last = call(
        &saved,
        now,
        "client",
        "mapping_profiles",
        json!({"offset":32}),
    )
    .unwrap();
    assert_eq!(last["ids"].as_array().unwrap().len(), 1);
}
