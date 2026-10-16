pub mod feedback;
pub use feedback::*;
mod hub;
pub mod pairing_code;
pub mod persistence;
mod publication;
pub mod sources;
pub use hub::*;
pub use publication::*;
pub use sources::*;
#[cfg(test)]
mod hub_tests;

pub fn qrcode_matrix(payload: &str) -> Result<Vec<Vec<bool>>, String> {
    if payload.len() > 4096 {
        return Err("Pairing offer exceeds QR budget.".into());
    }
    let code = qrcode::QrCode::new(payload.as_bytes())
        .map_err(|_| "Pairing offer does not fit a QR code.")?;
    Ok(code
        .to_colors()
        .chunks(code.width())
        .map(|row| {
            row.iter()
                .map(|color| *color == qrcode::Color::Dark)
                .collect()
        })
        .collect())
}

pub mod browser;
pub mod client;
pub mod server;
pub mod tls;

#[cfg(test)]
mod transport_tests;
