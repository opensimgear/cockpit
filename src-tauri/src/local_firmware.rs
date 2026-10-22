//! Verify the identity of the bytes staged for development embedding.
use serde_json::Value;
use sha2::{Digest,Sha256};

pub fn bundle_version(manifest:&[u8],board:&str,segments:&[(u32,&[u8])])->Result<String,String>{
    let manifest:Value=serde_json::from_slice(manifest).map_err(|_|"Invalid local firmware manifest.")?;
    let (chip,boot)=match board{"sunton-3248s035r"=>("esp32",0x1000),"waveshare-lcd7-s3"=>("esp32s3",0),_=>return Err("Unknown local firmware board.".into())};
    let version=manifest["version"].as_str().filter(|version|!version.is_empty()&&version.len()<=64&&version.bytes().all(|byte|byte.is_ascii_digit()||byte==b'.')).ok_or("Invalid local firmware version.")?;
    if manifest["schemaVersion"]!=1||manifest["component"]!="esp32-companion"||manifest["board"]!=board||manifest["chip"]!=chip{return Err("Local firmware manifest targets another board.".into());}
    let records=manifest["segments"].as_array().filter(|records|records.len()==3).ok_or("Local firmware manifest is incomplete.")?;
    if segments.len()!=3{return Err("Local firmware segments are incomplete.".into());}
    for ((offset,name),(address,bytes)) in [(boot,"bootloader.bin"),(0x8000,"partition-table.bin"),(0x10000,"application.bin")].into_iter().zip(segments){
        let record=records.iter().find(|record|record["file"]==name).ok_or("Local firmware segment is missing.")?;
        if *address!=offset||record["offset"]!=format!("0x{offset:x}")||record["sha256"]!=format!("{:x}",Sha256::digest(bytes)){return Err("Local firmware bytes do not match their staged manifest. Rebuild and stage the board firmware.".into());}
    }
    let mut identity=vec![0];identity.extend_from_slice(version.as_bytes());identity.push(0);
    if !segments[2].1.windows(identity.len()).any(|bytes|bytes==identity){return Err("Local firmware version does not match the application image. Rebuild and stage the board firmware.".into());}
    Ok(version.to_owned())
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn fixture()->(Value,Vec<(u32,Vec<u8>)>){
        let segments=vec![(0x1000,vec![1,2]),(0x8000,vec![3,4]),(0x10000,b"app\02026.10.21.1\0".to_vec())];
        let records:Vec<_>=segments.iter().zip(["bootloader.bin","partition-table.bin","application.bin"]).map(|((offset,bytes),name)|json!({"file":name,"offset":format!("0x{offset:x}"),"sha256":format!("{:x}",Sha256::digest(bytes))})).collect();
        (json!({"schemaVersion":1,"component":"esp32-companion","board":"sunton-3248s035r","chip":"esp32","version":"2026.10.21.1","segments":records}),segments)
    }
    fn check(manifest:&Value,segments:&[(u32,Vec<u8>)])->Result<String,String>{bundle_version(&serde_json::to_vec(manifest).unwrap(),"sunton-3248s035r",&segments.iter().map(|(offset,bytes)|(*offset,bytes.as_slice())).collect::<Vec<_>>())}
    #[test]fn staged_version_is_independent_of_the_newer_source_header(){let(m,s)=fixture();assert_eq!(check(&m,&s).unwrap(),"2026.10.21.1");}
    #[test]fn refuses_relabelling_old_images_as_new_firmware(){let(mut m,s)=fixture();m["version"]=json!("2026.10.22.3");assert!(check(&m,&s).unwrap_err().contains("version does not match"));}
    #[test]fn refuses_changed_or_partial_segments(){let(mut m,mut s)=fixture();s[0].1.push(9);assert!(check(&m,&s).is_err());s.pop();assert!(check(&m,&s).is_err());m["segments"]=json!([]);assert!(check(&m,&s).is_err());let(mut m,s)=fixture();m["segments"][0]["file"]=json!("missing.bin");assert!(check(&m,&s).is_err());}
    #[test]fn refuses_invalid_manifest_identity(){let(m,s)=fixture();for(key,value)in[("board",json!("other")),("chip",json!("esp32s3")),("version",json!("")),("version",json!("next")),("schemaVersion",json!(2))]{let mut broken=m.clone();broken[key]=value;assert!(check(&broken,&s).is_err());}assert!(bundle_version(b"broken","sunton-3248s035r",&[]).is_err());assert!(bundle_version(b"{}","unknown",&[]).is_err());}
    #[test]fn accepts_the_waveshare_board_with_its_own_boot_offset(){let(mut m,mut s)=fixture();m["board"]=json!("waveshare-lcd7-s3");m["chip"]=json!("esp32s3");s[0].0=0;m["segments"][0]["offset"]=json!("0x0");assert_eq!(bundle_version(&serde_json::to_vec(&m).unwrap(),"waveshare-lcd7-s3",&s.iter().map(|(offset,bytes)|(*offset,bytes.as_slice())).collect::<Vec<_>>()).unwrap(),"2026.10.21.1");}
}
