//! Pinned host-CA trust with ordinary rustls certificate-chain/SAN/time/signature verification.
use base64::{engine::general_purpose::STANDARD, Engine};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose,
};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use crate::persistence::{SealedSqliteStore,StateStorage};
use subtle::ConstantTimeEq;
use time::{Duration, OffsetDateTime};
use zeroize::Zeroize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeIdentity {
    pub host_id: String,
    pub ca_cert_der: Vec<u8>,
    ca_key_der: Vec<u8>,
    pub server_cert_der: Vec<u8>,
    server_key_der: Vec<u8>,
}
impl Drop for NativeIdentity {
    fn drop(&mut self) {
        self.ca_key_der.zeroize();
        self.server_key_der.zeroize();
    }
}
pub fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
pub fn ca_spki_pin(der: &[u8]) -> Result<String, String> {
    let (remaining, certificate) =
        x509_parser::parse_x509_certificate(der).map_err(|_| "Invalid host CA certificate")?;
    if !remaining.is_empty() || !certificate.is_ca() || !certificate.validity().is_valid() {
        return Err("Host trust certificate is not a currently valid CA".into());
    }
    Ok(sha256(certificate.public_key().raw))
}
pub fn verified_ca(base64: &str, expected_pin: &str) -> Result<CertificateDer<'static>, String> {
    if base64.len() > 32768 || expected_pin.len() != 71 {
        return Err("Invalid host CA trust material".into());
    }
    let der = STANDARD
        .decode(base64)
        .map_err(|_| "Invalid host CA encoding")?;
    let actual = ca_spki_pin(&der)?;
    if actual.as_bytes().ct_eq(expected_pin.as_bytes()).unwrap_u8() != 1 {
        return Err("Host CA fingerprint does not match the out-of-band pairing record".into());
    }
    Ok(CertificateDer::from(der))
}
pub fn pinned_client_config(ca_base64: &str, pin: &str) -> Result<Arc<ClientConfig>, String> {
    let ca = verified_ca(ca_base64, pin)?; // Pin is checked before any socket or pairing-secret write.
    let mut roots = RootCertStore::empty();
    roots
        .add(ca)
        .map_err(|error| format!("Invalid pinned host CA: {error}"))?;
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| error.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
    Ok(Arc::new(config))
}
impl NativeIdentity {
    pub fn generate(host_id: &str, names: Vec<String>) -> Result<Self, String> {
        let now = OffsetDateTime::now_utc();
        let ca_key = KeyPair::generate().map_err(|error| error.to_string())?;
        let mut params =
            CertificateParams::new(Vec::<String>::new()).map_err(|error| error.to_string())?;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.not_before = now - Duration::minutes(5);
        params.not_after = now + Duration::days(3650);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        params.distinguished_name = DistinguishedName::new();
        params.distinguished_name.push(
            DnType::CommonName,
            format!("OpenSimGear Companion CA {host_id}"),
        );
        let ca = params
            .self_signed(&ca_key)
            .map_err(|error| error.to_string())?;
        let mut identity = Self {
            host_id: host_id.into(),
            ca_cert_der: ca.der().to_vec(),
            ca_key_der: ca_key.serialize_der(),
            server_cert_der: Vec::new(),
            server_key_der: Vec::new(),
        };
        identity.renew_leaf(names)?;
        Ok(identity)
    }
    pub fn load_or_create(path: &Path, host_id: &str, names: Vec<String>) -> Result<Self, String> {
        let storage=SealedSqliteStore::new(path);
        let mut identity = if let Some(plain)=storage.read()? {
            let plain=zeroize::Zeroizing::new(plain);
            if plain.len()>131072{return Err("Stored TLS identity exceeds its budget".into());}
            let value=serde_json::from_slice::<Self>(&plain).map_err(|_|"Stored TLS identity is invalid".to_string())?;
            if value.host_id!=host_id{return Err("Stored TLS identity belongs to another host; explicit reset and re-pairing are required".into());}
            ca_spki_pin(&value.ca_cert_der)?;value
        } else {Self::generate(host_id,names.clone())?};
        identity.renew_leaf(names)?;
        let plain=zeroize::Zeroizing::new(serde_json::to_vec(&identity).map_err(|error|error.to_string())?);
        storage.write(&plain)?;
        Ok(identity)
    }
    pub fn renew_leaf(&mut self, names: Vec<String>) -> Result<(), String> {
        if names.is_empty() || names.len() > 32 {
            return Err("TLS server requires a bounded SAN list".into());
        }
        let key =
            KeyPair::try_from(self.ca_key_der.as_slice()).map_err(|error| error.to_string())?;
        let issuer = Issuer::from_ca_cert_der(&CertificateDer::from(self.ca_cert_der.clone()), key)
            .map_err(|error| error.to_string())?;
        let leaf_key = KeyPair::generate().map_err(|error| error.to_string())?;
        let mut params = CertificateParams::new(names).map_err(|error| error.to_string())?;
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::minutes(5);
        params.not_after = now + Duration::days(90);
        params.is_ca = IsCa::NoCa;
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params
            .distinguished_name
            .push(DnType::CommonName, "OpenSimGear Companion listener");
        let leaf = params
            .signed_by(&leaf_key, &issuer)
            .map_err(|error| error.to_string())?;
        self.server_cert_der = leaf.der().to_vec();
        self.server_key_der.zeroize();
        self.server_key_der = leaf_key.serialize_der();
        Ok(())
    }
    pub fn server_config(&self) -> Result<Arc<ServerConfig>, String> {
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|error| error.to_string())?
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(self.server_cert_der.clone())],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.server_key_der.clone())),
                )
                .map_err(|error| error.to_string())?;
        Ok(Arc::new(config))
    }
    pub fn ca_base64(&self) -> String {
        STANDARD.encode(&self.ca_cert_der)
    }
    pub fn pin(&self) -> Result<String, String> {
        ca_spki_pin(&self.ca_cert_der)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Duration as StdDuration;
    fn rejected(identity: NativeIdentity) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let config = identity.server_config().unwrap();
        let ca = identity.ca_base64();
        let pin = identity.pin().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(StdDuration::from_secs(2)))
                .unwrap();
            let mut connection = rustls::ServerConnection::new(config).unwrap();
            let mut stream = stream;
            let _ = connection.complete_io(&mut stream);
        });
        assert!(crate::client::connect(
            &format!("wss://127.0.0.1:{}/companion", address.port()),
            &ca,
            &pin
        )
        .is_err());
        server.join().unwrap();
    }
    #[test]
    fn normal_rustls_checks_reject_wrong_san_expired_leaf_and_tampered_signature() {
        rejected(NativeIdentity::generate("host", vec!["wrong-host.local".into()]).unwrap());
        let mut expired = NativeIdentity::generate("host", vec!["127.0.0.1".into()]).unwrap();
        let key = KeyPair::try_from(expired.ca_key_der.as_slice()).unwrap();
        let issuer =
            Issuer::from_ca_cert_der(&CertificateDer::from(expired.ca_cert_der.clone()), key)
                .unwrap();
        let leaf_key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec!["127.0.0.1".into()]).unwrap();
        params.not_before = OffsetDateTime::now_utc() - Duration::days(2);
        params.not_after = OffsetDateTime::now_utc() - Duration::days(1);
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        expired.server_cert_der = params.signed_by(&leaf_key, &issuer).unwrap().der().to_vec();
        expired.server_key_der = leaf_key.serialize_der();
        rejected(expired);
        let mut tampered = NativeIdentity::generate("host", vec!["127.0.0.1".into()]).unwrap();
        let index = tampered.server_cert_der.len() - 1;
        tampered.server_cert_der[index] ^= 1;
        rejected(tampered);
    }
    #[test]
    fn malformed_ca_and_oversized_trust_material_are_rejected_without_network() {
        assert!(ca_spki_pin(&[1, 2, 3]).is_err());
        assert!(verified_ca(&"a".repeat(32769), &"x".repeat(71)).is_err());
        assert!(verified_ca("a", "short").is_err());
        let mut identity = NativeIdentity::generate("host", vec!["localhost".into()]).unwrap();
        assert!(identity.renew_leaf(Vec::new()).is_err());
        assert!(identity.renew_leaf(vec!["x".into(); 33]).is_err());
        identity.ca_cert_der = vec![1, 2, 3];
        assert!(identity.renew_leaf(vec!["localhost".into()]).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn identity_reload_preserves_pin_and_refuses_foreign_or_corrupted_native_state() {
        let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let path = directory.path().join("identity.dpapi");
        let first =
            NativeIdentity::load_or_create(&path, "host", vec!["localhost".into()]).unwrap();
        let second =
            NativeIdentity::load_or_create(&path, "host", vec!["127.0.0.1".into()]).unwrap();
        assert_eq!(first.pin().unwrap(), second.pin().unwrap());
        assert!(NativeIdentity::load_or_create(&path, "other", vec!["localhost".into()]).is_err());
        let storage=SealedSqliteStore::new(&path);storage.read().unwrap();
        storage.write(b"corrupt").unwrap();
        assert!(NativeIdentity::load_or_create(&path, "host", vec!["localhost".into()]).is_err());
        storage.write(&vec![0;131073]).unwrap();
        assert!(NativeIdentity::load_or_create(&path, "host", vec!["localhost".into()]).is_err());
    }
}


// Certificate authentication for code bootstrap is supplied by SPAKE2, which
// authenticates the complete CA-bearing offer. This config is never used for auth.
#[derive(Debug)]
struct PairingBootstrap;
impl rustls::client::danger::ServerCertVerifier for PairingBootstrap {
    fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &rustls_pki_types::ServerName<'_>, _: &[u8], _: rustls_pki_types::UnixTime) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &rustls::crypto::ring::default_provider().signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &rustls::crypto::ring::default_provider().signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> { rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes() }
}
pub(crate) fn pairing_bootstrap_config() -> Arc<ClientConfig> {
    Arc::new(ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions().expect("TLS protocols")
        .dangerous().with_custom_certificate_verifier(Arc::new(PairingBootstrap)).with_no_client_auth())
}
