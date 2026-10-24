//! Public offline content uses the existing native client's transport checks.
//! It never enters the host's authoring/publication admission path.
use serde_json::{json,Value};
pub(crate) fn validate(publication:&Value)->Result<(),String>{
    let object=publication.as_object().ok_or("Invalid cached publication.")?;
    if object.len()!=4 || !["document","assets","publicationHash","capabilityFingerprint"].iter().all(|key|object.contains_key(*key)) {
        return Err("Unexpected cached publication fields.".into());
    }
    let fingerprint=publication["capabilityFingerprint"].as_str().ok_or("Missing cache capability identity.")?;
    if !crate::publication::digest_id(fingerprint){return Err("Invalid cache capability identity.".into());}
    let canonical=serde_json_canonicalizer::to_string(&publication["document"]).map_err(|_|"Invalid cached document.")?;
    super::verify_publication(&json!({"canonicalDocument":canonical,"publicationHash":publication["publicationHash"],"assets":publication["assets"]}))?;
    Ok(())
}
