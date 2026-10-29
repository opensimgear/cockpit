use super::*;

fn assert_self_signed_root(certificate_der: &[u8], leaf_der: &[u8]) {
    let (_, root) = x509_parser::parse_x509_certificate(certificate_der).unwrap();
    let (_, leaf) = x509_parser::parse_x509_certificate(leaf_der).unwrap();
    assert!(root.is_ca());
    assert_eq!(root.tbs_certificate.subject, root.tbs_certificate.issuer);
    root.verify_signature(None).unwrap();
    assert!(!leaf.is_ca());
    assert_ne!(certificate_der, leaf_der);
    assert_eq!(leaf.tbs_certificate.issuer, root.tbs_certificate.subject);
    leaf.verify_signature(Some(root.public_key())).unwrap();
}

#[test]
fn exported_trust_material_is_the_self_signed_ca_and_not_the_leaf() {
    let identity = NativeIdentity::generate("host", vec!["localhost".into()]).unwrap();
    let exported = STANDARD.decode(identity.ca_base64()).unwrap();
    assert_eq!(exported, identity.ca_cert_der);
    assert_self_signed_root(&exported, &identity.server_cert_der);
    assert_ne!(exported, identity.ca_key_der);
}

#[test]
fn non_self_signed_ca_and_ca_key_mismatch_are_rejected() {
    let mut identity = NativeIdentity::generate("host", vec!["localhost".into()]).unwrap();
    let other = NativeIdentity::generate("other", vec!["localhost".into()]).unwrap();
    identity.ca_cert_der = other.ca_cert_der.clone();
    assert!(ca_spki_pin(&identity.ca_cert_der).is_ok());
    assert!(identity
        .renew_leaf(vec!["localhost".into()])
        .unwrap_err()
        .contains("key does not match"));

    let mut invalid_root = NativeIdentity::generate("host", vec!["localhost".into()]).unwrap();
    let last = invalid_root.ca_cert_der.len() - 1;
    invalid_root.ca_cert_der[last] ^= 1;
    assert!(ca_spki_pin(&invalid_root.ca_cert_der).is_err());
}

#[cfg(windows)]
#[test]
fn protected_reload_and_listener_changes_preserve_the_exact_ca_and_key() {
    use crate::{
        server::{ServerConfig, ServerRuntime},
        Hub, SourceChange, SourceDefinition, SourceSink, SourceToken,
    };
    use std::sync::Arc;

    #[derive(Default)]
    struct EmptySink;
    impl SourceSink for EmptySink {
        fn register(&self, _: SourceDefinition) -> Result<(), String> {
            Ok(())
        }
        fn baseline(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> {
            Ok(())
        }
        fn changes(&self, _: &SourceToken, _: u64, _: &[SourceChange]) -> Result<(), String> {
            Ok(())
        }
        fn loss(&self, _: &SourceToken) -> Result<(), String> {
            Ok(())
        }
    }

    let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = directory.path().join("companion-identity.dpapi");
    let first = NativeIdentity::load_or_create(&path, "host", vec!["localhost".into()]).unwrap();
    let ca_der = first.ca_cert_der.clone();
    let ca_key = first.ca_key_der.clone();
    let isolated_path = directory.path().join("isolated/companion-identity.dpapi");
    std::fs::create_dir_all(isolated_path.parent().unwrap()).unwrap();
    let isolated =
        NativeIdentity::load_or_create(&isolated_path, "host", vec!["localhost".into()]).unwrap();
    assert_ne!(isolated.ca_cert_der, ca_der);
    assert_ne!(isolated.ca_key_der, ca_key);
    let second =
        NativeIdentity::load_or_create(&path, "host", vec!["127.0.0.1".into(), "localhost".into()])
            .unwrap();
    assert_eq!(second.ca_cert_der, ca_der);
    assert_eq!(second.ca_key_der, ca_key);
    assert_self_signed_root(&second.ca_cert_der, &second.server_cert_der);

    let sink = Arc::new(EmptySink);
    let hub = Hub::new("host".into(), sink);
    let runtime = ServerRuntime::new(hub.clone(), directory.path().to_path_buf()).unwrap();
    let exported_first = runtime.browser_certificate().unwrap();
    let first_der = pem_der(&exported_first);
    runtime
        .start(ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertised_host: "127.0.0.1".into(),
            discovery: false,
        })
        .unwrap();
    runtime.stop();
    runtime
        .start(ServerConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertised_host: "localhost".into(),
            discovery: false,
        })
        .unwrap();
    let exported_after_listener_change = runtime.browser_certificate().unwrap();
    assert_eq!(pem_der(&exported_after_listener_change), first_der);
    runtime.stop();
    drop(runtime);

    let restarted =
        NativeIdentity::load_or_create(&path, "host", vec!["localhost".into(), "127.0.0.1".into()])
            .unwrap();
    assert_eq!(restarted.ca_cert_der, ca_der);
    assert_eq!(restarted.ca_key_der, ca_key);
}

#[cfg(windows)]
#[test]
fn damaged_protected_identity_fails_without_replacing_the_saved_record() {
    let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let path = directory.path().join("companion-identity.dpapi");
    NativeIdentity::load_or_create(&path, "host", vec!["localhost".into()]).unwrap();
    let storage = SealedSqliteStore::new(&path);
    storage.write(b"damaged identity record").unwrap();
    let damaged = storage.read().unwrap().unwrap();
    assert!(NativeIdentity::load_or_create(&path, "host", vec!["localhost".into()]).is_err());
    assert_eq!(storage.read().unwrap().as_deref(), Some(damaged.as_slice()));
}

#[cfg(windows)]
fn pem_der(pem: &str) -> Vec<u8> {
    let base64 = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect::<String>();
    STANDARD.decode(base64).unwrap()
}
