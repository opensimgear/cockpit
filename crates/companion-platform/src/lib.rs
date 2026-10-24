//! Native Android Keystore cryptography. SQLite owns all durable ciphertext.
use base64::{engine::general_purpose::STANDARD,Engine};
pub const MAX_CREDENTIAL_BYTES:usize=65_536;
const MAX_CIPHER_BYTES:usize=MAX_CREDENTIAL_BYTES+29;
fn encode(bytes:&[u8],limit:usize)->Result<String,String>{
    if bytes.is_empty() || bytes.len()>limit{return Err("Invalid native protection payload size".into());}
    Ok(STANDARD.encode(bytes))
}
fn decode(encoded:String,limit:usize)->Result<Vec<u8>,String>{
    if encoded.len()>limit.div_ceil(3)*4{return Err("Native protection response exceeds budget".into());}
    let bytes=STANDARD.decode(encoded).map_err(|_|"Invalid native protection response")?;
    if bytes.is_empty() || bytes.len()>limit{return Err("Invalid native protection response size".into());}
    Ok(bytes)
}
#[cfg(target_os="android")]
mod android {
    use super::*;
    use osg_companion_service::persistence::NativeSealer;
    use serde::Deserialize;
    use serde_json::json;
    use tauri::{plugin::{PluginHandle,TauriPlugin},Manager,Runtime};
    pub struct PlatformStore<R:Runtime>(PluginHandle<R>);
    impl<R:Runtime> Clone for PlatformStore<R>{fn clone(&self)->Self{Self(self.0.clone())}}
    #[derive(Deserialize)]
    struct Response{data:String}
    impl<R:Runtime> PlatformStore<R>{
        fn call<T:serde::de::DeserializeOwned>(&self,command:&str,payload:serde_json::Value)->Result<T,String>{
            self.0.run_mobile_plugin(command,payload).map_err(|_|"Android protection is unavailable, locked or corrupt. Unlock the device or Forget host and pair again.".into())
        }
    }
    impl<R:Runtime> NativeSealer for PlatformStore<R>{
        fn available(&self)->Result<(),String>{let _:serde_json::Value=self.call("probe",json!({}))?;Ok(())}
        fn seal(&self,bytes:&[u8])->Result<Vec<u8>,String>{
            let response:Response=self.call("seal",json!({"data":encode(bytes,MAX_CREDENTIAL_BYTES)?}))?;
            decode(response.data,MAX_CIPHER_BYTES)
        }
        fn unseal(&self,bytes:&[u8])->Result<Vec<u8>,String>{
            let response:Response=self.call("unseal",json!({"data":encode(bytes,MAX_CIPHER_BYTES)?}))?;
            decode(response.data,MAX_CREDENTIAL_BYTES)
        }
    }
    pub fn init<R:Runtime>()->TauriPlugin<R>{
        tauri::plugin::Builder::new("companion-platform").setup(|app,api|{
            let handle=api.register_android_plugin("org.opensimgear.companion.platform","CompanionPlatformPlugin")?;
            app.manage(PlatformStore(handle));Ok(())
        }).build()
    }
}
#[cfg(target_os="android")]
pub use android::{init,PlatformStore};
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_bounds_and_validates_plaintext_and_ciphertext_without_durable_storage(){
        for limit in [MAX_CREDENTIAL_BYTES,MAX_CIPHER_BYTES]{
            for bytes in [vec![1],vec![0,255,128],vec![42;limit]]{assert_eq!(decode(encode(&bytes,limit).unwrap(),limit).unwrap(),bytes);}
            assert!(encode(&[],limit).is_err());assert!(encode(&vec![0;limit+1],limit).is_err());
            assert!(decode("".into(),limit).is_err());assert!(decode("invalid==".into(),limit).is_err());
            assert!(decode(STANDARD.encode(vec![0;limit+1]),limit).is_err());assert!(decode("A".repeat(limit*2),limit).is_err());
        }
    }
}
