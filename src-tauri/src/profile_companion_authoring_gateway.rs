//! Read-only profile authoring boundary. Browser resource/publication choices
//! are metadata selectors; paired instance authority comes from native cache.
use super::*;

enum Request<'a> {
    Catalogue,
    Inspect {
        resource: &'a str,
        publication: &'a str,
        offset: usize,
        limit: usize,
    },
}
fn parse(value: &Value) -> Result<Request<'_>, String> {
    let command = value["command"]
        .as_str()
        .ok_or("Choose a companion authoring operation.")?;
    let fields: &[&str] = match command {
        "profile_companion_catalogue" => &[],
        "profile_companion_inspect" => &["resourceId", "publicationHash", "offset", "limit"],
        _ => return Err("Unknown companion authoring operation.".into()),
    };
    if value
        .as_object()
        .ok_or("Companion authoring requires an object.")?
        .keys()
        .any(|key| key != "command" && !fields.contains(&key.as_str()))
    {
        return Err("Unknown companion authoring field.".into());
    }
    if command == "profile_companion_catalogue" {
        return Ok(Request::Catalogue);
    }
    let resource = value["resourceId"]
        .as_str()
        .filter(|s| uuid::Uuid::parse_str(s).is_ok_and(|id| id.to_string() == *s))
        .ok_or("Choose an exact native resource UUID.")?;
    let publication = value["publicationHash"]
        .as_str()
        .filter(|s| {
            s.strip_prefix("sha256:").is_some_and(|h| {
                h.len() == 64
                    && h.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        })
        .ok_or("Choose an exact retained publication hash.")?;
    let number = |key: &str, default: usize, max: usize| -> Result<usize, String> {
        match value.get(key) {
            None => Ok(default),
            Some(value) => value
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .filter(|v| *v <= max)
                .ok_or_else(|| format!("{key} exceeds the bounded authoring page.")),
        }
    };
    let offset = number("offset", 0, 1024)?;
    let limit = number("limit", 32, 64)?;
    if limit == 0 {
        return Err("Choose 1–64 ports per page.".into());
    }
    Ok(Request::Inspect {
        resource,
        publication,
        offset,
        limit,
    })
}
impl GatewayManager {
    pub(super) fn companion_authoring_request(&self, request: &Value) -> Result<Value, String> {
        match parse(request)? {
            Request::Catalogue => {
                serde_json::to_value(self.companion_host.profile_authoring_catalogue()?)
                    .map_err(|e| e.to_string())
            }
            Request::Inspect {
                resource,
                publication,
                offset,
                limit,
            } => {
                let (canonical, instance) = self
                    .profile_resources
                    .companion_authoring_identity(resource)?;
                let page = self.companion_host.profile_authoring_inspect(
                    &canonical,
                    &instance,
                    publication,
                    offset,
                    limit,
                )?;
                // Pairing/identity may change while the retained panel is read.
                if self
                    .profile_resources
                    .companion_authoring_identity(resource)?
                    != (canonical, instance)
                {
                    return Err(
                        "The native companion identity changed while reading its ports.".into(),
                    );
                }
                serde_json::to_value(page).map_err(|e| e.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authoring_requests_accept_only_bounded_metadata_selectors() {
        assert!(matches!(
            parse(&json!({"command":"profile_companion_catalogue"})).unwrap(),
            Request::Catalogue
        ));
        let valid = json!({"command":"profile_companion_inspect","resourceId":"11111111-1111-4111-8111-111111111111","publicationHash":format!("sha256:{}","a".repeat(64))});
        assert!(matches!(
            parse(&valid).unwrap(),
            Request::Inspect {
                offset: 0,
                limit: 32,
                ..
            }
        ));
        for (field, value) in [
            ("instanceId", json!("forged")),
            ("token", json!({})),
            ("offset", json!(-1)),
            ("offset", json!(1025)),
            ("limit", json!(0)),
            ("limit", json!(65)),
            ("limit", json!(1.2)),
            ("publicationHash", json!("sha256:BAD")),
            ("resourceId", json!("device name")),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(parse(&invalid).is_err(), "{invalid}");
        }
        assert!(
            parse(&json!({"command":"profile_companion_catalogue","instanceId":"forged"})).is_err()
        );
    }
}
