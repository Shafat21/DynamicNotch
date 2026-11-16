//! Plain-text notes (spec §23.1): the row type and the body rule shared by the commands.

use serde::Serialize;

/// Room for a long call's transcript (~1,000 chars per minute).
pub const MAX_BODY: usize = 200_000;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: i64,
    pub body: String,
    pub pinned: bool,
    pub updated_at: i64,
    /// Written by the notetaker.
    pub call: bool,
}

/// Trimmed body; empty or over `MAX_BODY` chars is an error.
pub fn clean_body(body: &str) -> Result<&str, String> {
    let b = body.trim();
    if b.is_empty() || b.chars().count() > MAX_BODY {
        return Err(format!("A note must be 1-{MAX_BODY} characters"));
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_is_trimmed_and_limited() {
        assert_eq!(clean_body("  hi \n"), Ok("hi"));
        assert!(clean_body("  \n ").is_err());
        assert!(clean_body(&"é".repeat(MAX_BODY)).is_ok()); // chars, not bytes
        assert!(clean_body(&"x".repeat(MAX_BODY + 1)).is_err());
    }
}
