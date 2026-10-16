//! Native Android Keystore bridge. No credential API is granted to JavaScript.
use base64::{engine::general_purpose::STANDARD, Engine};
pub const MAX_CREDENTIAL_BYTES: usize = 65_536;

fn encode(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() || bytes.len() > MAX_CREDENTIAL_BYTES {
        return Err("Invalid native credential payload size".into());
    }
    Ok(STANDARD.encode(bytes))
}
fn decode(encoded: Option<String>) -> Result<Option<Vec<u8>>, String> {
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    if encoded.len() > MAX_CREDENTIAL_BYTES.div_ceil(3) * 4 {
        return Err("Stored native credential exceeds budget".into());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "Invalid protected credential response")?;
    if bytes.is_empty() || bytes.len() > MAX_CREDENTIAL_BYTES {
        return Err("Invalid stored native credential size".into());
    }
    Ok(Some(bytes))
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use osg_companion_service::persistence::StateStorage;
    use serde::Deserialize;
    use serde_json::json;
    use tauri::{
        plugin::{PluginHandle, TauriPlugin},
        Manager, Runtime,
    };

    pub struct PlatformStore<R: Runtime>(PluginHandle<R>);
    impl<R: Runtime> Clone for PlatformStore<R> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }
    #[derive(Deserialize)]
    struct ReadResponse {
        data: Option<String>,
    }
    impl<R: Runtime> PlatformStore<R> {
        fn call<T: serde::de::DeserializeOwned>(
            &self,
            command: &str,
            payload: serde_json::Value,
        ) -> Result<T, String> {
            self.0
                .run_mobile_plugin(command, payload)
                .map_err(|error| format!("Android protected storage: {error}"))
        }
    }
    impl<R: Runtime> StateStorage for PlatformStore<R> {
        fn available(&self) -> Result<(), String> {
            let _: serde_json::Value = self.call("probe", json!({}))?;
            Ok(())
        }
        fn read(&self) -> Result<Option<Vec<u8>>, String> {
            let reply: ReadResponse = self.call("read", json!({}))?;
            decode(reply.data)
        }
        fn write(&self, bytes: &[u8]) -> Result<(), String> {
            let _: serde_json::Value = self.call("write", json!({"data": encode(bytes)?}))?;
            Ok(())
        }
        fn delete(&self) -> Result<(), String> {
            let _: serde_json::Value = self.call("delete", json!({}))?;
            Ok(())
        }
    }
    pub fn init<R: Runtime>() -> TauriPlugin<R> {
        tauri::plugin::Builder::new("companion-platform")
            .setup(|app, api| {
                let handle = api.register_android_plugin(
                    "org.opensimgear.companion.platform",
                    "CompanionPlatformPlugin",
                )?;
                app.manage(PlatformStore(handle));
                Ok(())
            })
            .build()
    }
}
#[cfg(target_os = "android")]
pub use android::{init, PlatformStore};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_bounds_and_validates_credential_payloads() {
        assert_eq!(decode(None).unwrap(), None);
        for bytes in [vec![1], vec![0, 255, 128], vec![42; MAX_CREDENTIAL_BYTES]] {
            assert_eq!(decode(Some(encode(&bytes).unwrap())).unwrap(), Some(bytes));
        }
        assert!(encode(&[]).is_err());
        assert!(encode(&vec![0; MAX_CREDENTIAL_BYTES + 1]).is_err());
        assert!(decode(Some("".into())).is_err());
        assert!(decode(Some("invalid==".into())).is_err());
        assert!(decode(Some(STANDARD.encode(vec![0; MAX_CREDENTIAL_BYTES + 1]))).is_err());
        assert!(decode(Some("A".repeat(MAX_CREDENTIAL_BYTES * 2))).is_err());
    }
}
