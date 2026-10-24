//! A restore changes the database incarnation even when revisions match.
use super::*;
pub(crate) fn read(connection: &Connection) -> Result<String, String> {
    let value: String = connection
        .query_row(
            "SELECT epoch FROM incarnation WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(|_| {
            "SQLite layout differs from this development build. Start with a fresh database."
                .to_string()
        })?;
    if value.len() != 32
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Invalid database incarnation. Preserve the database for inspection.".into());
    }
    Ok(value)
}
