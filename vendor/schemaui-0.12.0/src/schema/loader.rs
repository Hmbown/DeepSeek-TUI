use anyhow::{Context, Result};
use serde_json::Value;

use super::model::{Schema, SchemaObject};

/// Deserialize a JSON value into an internal `SchemaObject`.
pub fn load_root_schema(value: &Value) -> Result<SchemaObject> {
    serde_json::from_value::<Schema>(value.clone())
        .map(Schema::into_object)
        .context("schema is not a valid JSON Schema document")
}
