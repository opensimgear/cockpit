use super::{exact, Inner};
use serde::Serialize;
use serde_json::Value;

const MAX_CSS_DIMENSION: f64 = 16_384.0;
const MIN_PIXEL_RATIO: f64 = 0.25;
const MAX_PIXEL_RATIO: f64 = 8.0;
const MAX_DISPLAY_DIMENSION: f64 = 65_536.0;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DisplayReport {
    publication_hash: Option<String>,
    page_id: Option<String>,
    panel_width: Option<u64>,
    panel_height: Option<u64>,
    display_width: u64,
    display_height: u64,
    warning: String,
}

/// Parse bounded, authenticated presentation telemetry. References that raced
/// an assignment change are ignored; malformed values are rejected.
pub(super) fn validate_report(
    value: Option<&Value>,
    inner: &Inner,
    companion: &str,
) -> Result<Option<DisplayReport>, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    exact(
        value,
        &["cssWidth", "cssHeight", "pixelRatio"],
        &["publicationHash", "pageId"],
    )?;
    let publication_hash = value
        .get("publicationHash")
        .filter(|value| !value.is_null());
    let page_id = value.get("pageId").filter(|value| !value.is_null());
    if publication_hash.is_some() != page_id.is_some() {
        return Err("Display publication and page references must be supplied together.".into());
    }
    let (publication_hash, page_id) =
        if let (Some(publication_hash), Some(page_id)) = (publication_hash, page_id) {
            let publication_hash = publication_hash
                .as_str()
                .filter(|hash| {
                    hash.len() == 71
                        && hash.starts_with("sha256:")
                        && hash[7..]
                            .bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                })
                .ok_or_else(|| "Invalid display publication hash.".to_owned())?;
            let page_id = page_id
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 64 && !id.chars().any(char::is_control))
                .ok_or_else(|| "Invalid display page ID.".to_owned())?;
            (Some(publication_hash), Some(page_id))
        } else {
            (None, None)
        };
    let css_width = bounded_number(
        &value["cssWidth"],
        0.0,
        MAX_CSS_DIMENSION,
        false,
        "CSS width",
    )?;
    let css_height = bounded_number(
        &value["cssHeight"],
        0.0,
        MAX_CSS_DIMENSION,
        false,
        "CSS height",
    )?;
    let pixel_ratio = bounded_number(
        &value["pixelRatio"],
        MIN_PIXEL_RATIO,
        MAX_PIXEL_RATIO,
        true,
        "pixel ratio",
    )?;
    let display_width = (css_width * pixel_ratio - 0.01).ceil();
    let display_height = (css_height * pixel_ratio - 0.01).ceil();
    if display_width <= 0.0
        || display_height <= 0.0
        || display_width > MAX_DISPLAY_DIMENSION
        || display_height > MAX_DISPLAY_DIMENSION
    {
        return Err("Display dimensions are outside the supported range.".into());
    }

    let display_width = display_width as u64;
    let display_height = display_height as u64;
    let (panel_width, panel_height, warning) = if let (Some(publication_hash), Some(page_id)) =
        (publication_hash, page_id)
    {
        let current_assignment = inner.record.instances.iter().any(|assignment| {
            assignment.companion_id == companion && assignment.publication_hash == publication_hash
        });
        if !current_assignment {
            return Ok(None);
        }
        let Some(publication) = inner.publications.get(publication_hash) else {
            return Ok(None);
        };
        let Ok(document) = serde_json::from_str::<Value>(&publication.canonical_document) else {
            return Ok(None);
        };
        let viewport = document["presentation"]["viewports"]
            .as_array()
            .and_then(|viewports| {
                viewports
                    .iter()
                    .find(|viewport| viewport["pageId"] == page_id)
            });
        let Some(viewport) = viewport else {
            return Ok(None);
        };
        let (Some(panel_width), Some(panel_height)) =
            (viewport["width"].as_u64(), viewport["height"].as_u64())
        else {
            return Ok(None);
        };
        let warning = if panel_width >= display_width && panel_height >= display_height {
            String::new()
        } else {
            format!("The selected panel is too small for this display: {panel_width} × {panel_height} px, displayed at {display_width} × {display_height} px. Widgets may look blurry. Increase the panel size in Cockpit to at least {display_width} × {display_height} px and recompile, or use a smaller browser window.")
        };
        (Some(panel_width), Some(panel_height), warning)
    } else {
        (None, None, String::new())
    };
    Ok(Some(DisplayReport {
        publication_hash: publication_hash.map(str::to_owned),
        page_id: page_id.map(str::to_owned),
        panel_width,
        panel_height,
        display_width,
        display_height,
        warning,
    }))
}

fn bounded_number(
    value: &Value,
    min: f64,
    max: f64,
    inclusive_min: bool,
    name: &str,
) -> Result<f64, String> {
    let number = value
        .as_f64()
        .ok_or_else(|| format!("Invalid display {name}."))?;
    let lower_ok = if inclusive_min {
        number >= min
    } else {
        number > min
    };
    if !number.is_finite() || !lower_ok || number > max {
        return Err(format!("Display {name} is outside the supported range."));
    }
    Ok(number)
}

pub(super) fn client_reports(inner: &Inner, companion: &str) -> Vec<DisplayReport> {
    inner
        .connections
        .iter()
        .filter(|(_, owner)| owner.as_str() == companion)
        .filter_map(|(connection, _)| inner.display_reports.get(connection))
        .filter(|report| report_is_current(inner, companion, report))
        .cloned()
        .collect()
}

pub(super) fn clear_companion(inner: &mut Inner, companion: &str) {
    let connections = inner
        .connections
        .iter()
        .filter(|(_, owner)| owner.as_str() == companion)
        .map(|(connection, _)| connection.clone())
        .collect::<std::collections::BTreeSet<_>>();
    inner
        .display_reports
        .retain(|connection, _| !connections.contains(connection));
}

fn report_is_current(inner: &Inner, companion: &str, report: &DisplayReport) -> bool {
    let (Some(publication_hash), Some(page_id)) = (&report.publication_hash, &report.page_id)
    else {
        return true;
    };
    inner.record.instances.iter().any(|assignment| {
        assignment.companion_id == companion
            && assignment.publication_hash == *publication_hash
            && inner
                .publications
                .get(publication_hash)
                .and_then(|publication| {
                    serde_json::from_str::<Value>(&publication.canonical_document).ok()
                })
                .is_some_and(|document| {
                    document["presentation"]["viewports"]
                        .as_array()
                        .is_some_and(|viewports| {
                            viewports
                                .iter()
                                .any(|viewport| viewport["pageId"] == page_id.as_str())
                        })
                })
    })
}
