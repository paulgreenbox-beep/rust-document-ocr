//! JSON export of the OCR document result.

use std::path::Path;

use crate::dto::DocumentResult;

/// Serialize the entire [`DocumentResult`] to pretty-printed JSON and
/// write it to `output`.
pub fn export_json(
    result: &DocumentResult,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(result)?;
    std::fs::write(output, json)?;
    Ok(())
}
