use serde_json::Value;

pub(crate) fn owner(client: &str) -> u32 {
    client
        .bytes()
        .fold(2_166_136_261u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
        })
        .max(1)
}
fn tuples<'a>(
    config: &'a Value,
    key: &str,
    limit: usize,
    bounds: &[u64],
) -> Result<&'a Vec<Value>, String> {
    let values = config[key]
        .as_array()
        .filter(|v| v.len() <= limit)
        .ok_or_else(|| format!("Invalid {key} configuration."))?;
    for row in values {
        let row = row
            .as_array()
            .filter(|row| row.len() == bounds.len())
            .ok_or_else(|| format!("Invalid {key} mapping."))?;
        if row
            .iter()
            .zip(bounds)
            .any(|(v, max)| v.as_u64().is_none_or(|v| v > *max))
        {
            return Err(format!("A {key} mapping is outside its supported range."));
        }
    }
    Ok(values)
}
pub(crate) fn commands(config: &Value, owner: u32) -> Result<Vec<String>, String> {
    let name = config["name"]
        .as_str()
        .filter(|v| !v.trim().is_empty() && v.len() <= 63 && !v.chars().any(char::is_control))
        .ok_or("Device name needs 1–63 UTF-8 bytes without control characters.")?;
    let mask = config["expanderMask"]
        .as_u64()
        .filter(|v| *v <= 255)
        .ok_or("Expansion mask must be 0–255.")?;
    let buttons = tuples(config, "buttons", 128, &[151, 1, 255])?;
    let axes = tuples(config, "axes", 8, &[7, 56, 4094, 4095, 65535, 1, 32767])?;
    let hats = tuples(config, "hats", 4, &[151, 151, 151, 151])?;
    let encoders = tuples(config, "encoders", 8, &[23, 23])?;
    let selectors = tuples(config, "selectors", 8, &[151, 151])?;
    if buttons.len() + 2 * encoders.len() + 3 * selectors.len() > 128 {
        return Err(
            "Choose at most 128 logical buttons including encoder and selector buttons.".into(),
        );
    }
    for axis in axes {
        let axis = axis.as_array().unwrap();
        if axis[1].as_u64().unwrap() < 48 || axis[2].as_u64().unwrap() >= axis[3].as_u64().unwrap()
        {
            return Err(
                "Axis usage must be 48–56 and calibration minimum must be below maximum.".into(),
            );
        }
    }
    let encoded: String = name.bytes().map(|b| format!("{b:02x}")).collect();
    let mut result = vec![
        format!("discard {owner}"),
        format!(
            "counts {owner} {} {} {} {} {} {mask}",
            buttons.len(),
            axes.len(),
            hats.len(),
            encoders.len(),
            selectors.len()
        ),
        format!("name {owner} {encoded}"),
    ];
    for (key, rows) in [
        ("button", buttons),
        ("axis", axes),
        ("hat", hats),
        ("encoder", encoders),
        ("selector", selectors),
    ] {
        for (index, row) in rows.iter().enumerate() {
            let values = row
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap().to_string())
                .collect::<Vec<_>>()
                .join(" ");
            result.push(format!("{key} {owner} {index} {values}"));
        }
    }
    Ok(result)
}
pub(crate) fn validate(snapshot: &Value) -> Result<(), String> {
    if snapshot.to_string().len() > 4096
        || snapshot["generation"].as_u64().is_none()
        || snapshot["savedGeneration"].as_u64().is_none()
        || snapshot["owner"]
            .as_u64()
            .is_none_or(|v| v > u32::MAX as u64)
        || !snapshot["dirty"].is_boolean()
        || !snapshot["pendingApply"].is_boolean()
        || !matches!(snapshot["view"].as_str(), Some("draft" | "active"))
    {
        return Err("Module returned an invalid configuration snapshot.".into());
    }
    if snapshot["valid"] != false {
        commands(&snapshot["config"], 1)?;
    } else {
        if snapshot["view"] != "draft" {
            return Err("The active module configuration is invalid.".into());
        }
        let config = &snapshot["config"];
        if config["name"].as_str().is_none_or(|v| v.len() > 63)
            || config["expanderMask"].as_u64().is_none_or(|v| v > 255)
        {
            return Err("Module returned an invalid draft.".into());
        }
        for (key, limit, bounds) in [
            ("buttons", 128, vec![151, 1, 255]),
            ("axes", 8, vec![7, 56, 4095, 4095, 65535, 1, 32767]),
            ("hats", 4, vec![151; 4]),
            ("encoders", 8, vec![23; 2]),
            ("selectors", 8, vec![151; 2]),
        ] {
            tuples(config, key, limit, &bounds)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn builds_complete_mapping_commands_and_rejects_invalid_before_writing() {
        let config = json!({"name":"Pedals é","expanderMask":0,"buttons":[[0,0,0]],"axes":[[0,48,0,4095,0,1,200]],"hats":[],"encoders":[],"selectors":[]});
        let result = commands(&config, 12).unwrap();
        assert_eq!(result[1], "counts 12 1 1 0 0 0 0");
        assert_eq!(result.last().unwrap(), "axis 12 0 0 48 0 4095 0 1 200");
        let mut invalid = config.clone();
        invalid["axes"][0][2] = json!(4095);
        assert!(commands(&invalid, 12).is_err());
        invalid = config;
        invalid["buttons"][0][0] = json!(152);
        assert!(commands(&invalid, 12).is_err());
    }
    #[test]
    fn output_only_configuration_encodes_zero_counts_and_remains_a_valid_snapshot() {
        let config = json!({"name":"Rev lights","expanderMask":0,"buttons":[],"axes":[],"hats":[],"encoders":[],"selectors":[]});
        let encoded = commands(&config, 9).unwrap();
        assert_eq!(encoded[1], "counts 9 0 0 0 0 0 0");
        assert_eq!(encoded.len(), 3);
        let mut snapshot = json!({"generation":1,"savedGeneration":1,"owner":0,"dirty":false,"pendingApply":false,"view":"active","valid":true,"config":config});
        assert!(validate(&snapshot).is_ok());
        snapshot["config"]["axes"] = Value::Null;
        assert!(validate(&snapshot).is_err());
    }
}
