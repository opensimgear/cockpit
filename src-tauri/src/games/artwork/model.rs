use serde::{Deserialize,Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Crop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Crop {
    pub(super) fn validate(&self) -> Result<(), String> {
        if ![self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            || self.x < 0.0
            || self.y < 0.0
            || self.width <= 0.0
            || self.height <= 0.0
            || self.x + self.width > 1.0 + 1e-9
            || self.y + self.height > 1.0 + 1e-9
        {
            return Err("Keep the entire tile crop inside the image bounds.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Background {
    pub image: String,
    pub position_x: f64,
    pub position_y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,
}
impl Background {
    pub(crate) fn validate(&self) -> Result<(), String> {
        asset_name(&self.image)?;
        if let Some(crop) = self.crop {
            crop.validate()?;
        }
        if !self.position_x.is_finite()
            || !self.position_y.is_finite()
            || !(0.0..=100.0).contains(&self.position_x)
            || !(0.0..=100.0).contains(&self.position_y)
        {
            return Err("Background position must be between 0 and 100 percent.".into());
        }
        Ok(())
    }
}
pub(super) fn asset_name(reference: &str) -> Result<&str, String> {
    let name = reference
        .strip_prefix("artwork:")
        .ok_or("Unknown managed game background.")?;
    let hash = name
        .strip_suffix(".jpg")
        .ok_or("Unknown managed game background format.")?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Invalid managed game background identity.".into());
    }
    Ok(name)
}
