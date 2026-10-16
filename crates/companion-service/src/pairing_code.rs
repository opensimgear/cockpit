//! Short-code pairing authenticates the CA-bearing offer with SPAKE2 before normal pinned WSS.
use base64::{engine::general_purpose::STANDARD, Engine};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use serde_json::{json, Value};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use zeroize::Zeroizing;

pub const CONTEXT: &[u8] = b"opensimgear-cockpit-pairing-v1";
pub fn new_code() -> Result<String, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| "Pairing randomness unavailable")?;
    Ok(bytes
        .iter()
        .map(|b| ALPHABET[(b & 31) as usize] as char)
        .collect())
}
pub fn normalize(code: &str) -> Result<String, String> {
    let value = code.trim().to_ascii_uppercase();
    if value.len() != 8
        || !value
            .bytes()
            .all(|b| b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(&b))
    {
        return Err("Enter the eight letters/numbers shown in Cockpit.".into());
    }
    Ok(value)
}
pub fn start(code: &str) -> (Spake2<Ed25519Group>, Vec<u8>) {
    Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.as_bytes()),
        &Identity::new(CONTEXT),
    )
}
fn key(bytes: &[u8]) -> Result<LessSafeKey, String> {
    UnboundKey::new(&AES_256_GCM, bytes)
        .map(LessSafeKey::new)
        .map_err(|_| "Invalid pairing key".into())
}
pub fn respond(code: &str, inbound: &str, payload: &Value) -> Result<Value, String> {
    let inbound = STANDARD
        .decode(inbound)
        .map_err(|_| "Invalid pairing exchange")?;
    if inbound.len() != 33 {
        return Err("Invalid pairing exchange".into());
    }
    let (state, message) = start(code);
    let shared = Zeroizing::new(
        state
            .finish(&inbound)
            .map_err(|_| "Invalid pairing exchange")?,
    );
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|_| "Pairing randomness unavailable")?;
    let mut encrypted = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
    key(&shared)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(CONTEXT),
            &mut encrypted,
        )
        .map_err(|_| "Pairing encryption failed")?;
    Ok(
        json!({"ok":true,"type":"pair-code-offer","message":STANDARD.encode(message),"nonce":STANDARD.encode(nonce),"encrypted":STANDARD.encode(encrypted)}),
    )
}
pub fn finish(state: Spake2<Ed25519Group>, reply: &Value) -> Result<Value, String> {
    let message = STANDARD
        .decode(reply["message"].as_str().ok_or("Missing pairing reply")?)
        .map_err(|_| "Invalid pairing reply")?;
    let shared = Zeroizing::new(
        state
            .finish(&message)
            .map_err(|_| "Invalid pairing exchange")?,
    );
    let nonce: [u8; 12] = STANDARD
        .decode(reply["nonce"].as_str().ok_or("Missing pairing nonce")?)
        .map_err(|_| "Invalid pairing nonce")?
        .try_into()
        .map_err(|_| "Invalid pairing nonce")?;
    let mut encrypted = Zeroizing::new(
        STANDARD
            .decode(reply["encrypted"].as_str().ok_or("Missing pairing offer")?)
            .map_err(|_| "Invalid pairing offer")?,
    );
    if encrypted.len() > 8192 {
        return Err("Pairing offer exceeds budget".into());
    }
    let clear = key(&shared)?
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(CONTEXT),
            &mut encrypted,
        )
        .map_err(|_| "Pairing code did not match. Start again in Cockpit.")?;
    serde_json::from_slice(clear).map_err(|_| "Invalid authenticated pairing offer".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_exchange_authenticates_offer_and_rejects_wrong_code_and_tampering() {
        let code = new_code().unwrap();
        assert_eq!(normalize(&code.to_lowercase()).unwrap(), code);
        let (state, message) = start(&code);
        let payload =
            json!({"endpoint":"wss://127.0.0.1:54437/companion","secret":"private","pin":"bound"});
        let reply = respond(&code, &STANDARD.encode(&message), &payload).unwrap();
        assert_eq!(finish(state, &reply).unwrap(), payload);
        let (wrong, message) = start("AAAAAAAA");
        assert!(finish(
            wrong,
            &respond("BBBBBBBB", &STANDARD.encode(message), &payload).unwrap()
        )
        .is_err());
        let (state, message) = start(&code);
        let mut reply = respond(&code, &STANDARD.encode(message), &payload).unwrap();
        reply["nonce"] = json!(STANDARD.encode([1u8; 12]));
        assert!(finish(state, &reply).is_err());
        assert!(normalize("123").is_err());
    }
}
