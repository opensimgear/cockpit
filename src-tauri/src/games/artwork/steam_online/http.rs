use super::super::*;
use std::time::Duration;

const METADATA_LIMIT: usize = 1024 * 1024;
const DOWNLOAD_LIMIT: usize = 4 * 1024 * 1024;

/// Store metadata supplies current hashed capsule URLs. No account or API key is used.
pub(super) fn download(app_id: u32) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(6))
        .timeout_connect(Duration::from_secs(2))
        .timeout_read(Duration::from_secs(3))
        .redirects(0)
        .build();
    download_with(app_id, |url, limit| get(&agent, url, limit))
}

fn download_with(
    app_id: u32,
    mut get: impl FnMut(&str, usize) -> Result<Vec<u8>, String>,
) -> Result<String, String> {
    let metadata = get(
        &format!("https://store.steampowered.com/api/appdetails?appids={app_id}&filters=basic&l=english"),
        METADATA_LIMIT,
    )?;
    let url = image_url(app_id, &metadata)?;
    let bytes = get(&url, DOWNLOAD_LIMIT)?;
    let original = decoded(&bytes)?;
    // The store may return its generated 460px header. Probe only the matching
    // 2x header beside a confirmed same-app header, keeping that exact URL as fallback.
    if original.width() < 920 {
        if let Some(larger) = larger_header_url(&url) {
            if let Ok(bytes) = get(&larger, DOWNLOAD_LIMIT) {
                if decoded(&bytes).is_ok_and(|image| {
                    image.width() > original.width() && image.height() > original.height()
                }) {
                    if let Ok(preview) = encode(&bytes) {
                        return Ok(preview);
                    }
                }
            }
        }
    }
    encode(&bytes)
}

fn larger_header_url(url: &str) -> Option<String> {
    let mut url = tauri::Url::parse(url).ok()?;
    let path = url.path().strip_suffix("/header.jpg")?;
    url.set_path(&format!("{path}/header_2x.jpg"));
    Some(url.into())
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;

fn get(agent: &ureq::Agent, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let response = agent
        .get(url)
        .set("User-Agent", "OpenSimGear/0.1 artwork")
        .call()
        .map_err(|error| format!("Steam artwork request failed: {error}"))?;
    read_bounded(response.into_reader(), limit)
}

pub(super) fn read_bounded(reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("Steam artwork response exceeds its size limit.".into());
    }
    Ok(bytes)
}

pub(super) fn image_url(app_id: u32, bytes: &[u8]) -> Result<String, String> {
    let metadata: Value =
        serde_json::from_slice(bytes).map_err(|_| "Invalid Steam artwork metadata.")?;
    let game = &metadata[app_id.to_string()];
    let url = game["data"]["header_image"]
        .as_str()
        .filter(|_| game["success"] == true)
        .ok_or("Steam has no artwork for this app ID.")?;
    let parsed = tauri::Url::parse(url).map_err(|_| "Invalid Steam artwork URL.")?;
    let allowed_host = matches!(
        parsed.host_str(),
        Some(
            "shared.akamai.steamstatic.com"
                | "shared.fastly.steamstatic.com"
                | "cdn.akamai.steamstatic.com"
                | "cdn.cloudflare.steamstatic.com"
                | "cdn.fastly.steamstatic.com"
                | "steamcdn-a.akamaihd.net"
        )
    );
    if url.len() > 4096
        || parsed.scheme() != "https"
        || !allowed_host
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port_or_known_default() != Some(443)
        || ![
            format!("/store_item_assets/steam/apps/{app_id}/"),
            format!("/steam/apps/{app_id}/"),
        ]
        .iter()
        .any(|prefix| parsed.path().starts_with(prefix))
    {
        return Err("Steam artwork must come from its app's official image host.".into());
    }
    Ok(parsed.to_string())
}

pub(super) fn encode(bytes: &[u8]) -> Result<String, String> {
    discovery_bytes(bytes)
}
