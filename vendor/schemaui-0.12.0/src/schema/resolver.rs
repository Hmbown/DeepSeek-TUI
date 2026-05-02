use anyhow::{Context, Result, bail};
use percent_encoding::percent_decode_str;
use serde_json::Value;

use super::dialect::RootDialectContext;
use super::model::{Schema, SchemaObject};

#[derive(Debug)]
pub struct SchemaResolver<'a> {
    raw: &'a Value,
}

impl<'a> SchemaResolver<'a> {
    pub fn new(raw: &'a Value) -> Self {
        Self { raw }
    }

    pub fn resolve_schema(&self, schema: &Schema) -> Result<SchemaObject> {
        match schema {
            Schema::Bool(_) => Ok(SchemaObject::default()),
            Schema::Object(object) => {
                if let Some(reference) = &object.reference {
                    let resolved = self.follow_reference(reference)?;
                    Ok(overlay_reference_annotations(resolved, object.as_ref()))
                } else {
                    Ok(object.as_ref().clone())
                }
            }
        }
    }

    pub fn root_dialect_context(&self) -> RootDialectContext {
        RootDialectContext::from_root(self.raw)
    }

    fn follow_reference(&self, reference: &str) -> Result<SchemaObject> {
        if let Some(fragment) = reference.strip_prefix('#') {
            let decoded = percent_decode_str(fragment)
                .decode_utf8()
                .context("invalid percent-encoding in $ref")?;
            let pointer = if decoded.is_empty() {
                String::new()
            } else if decoded.starts_with('/') {
                decoded.to_string()
            } else {
                format!("/{}", decoded)
            };
            let target = self
                .raw
                .pointer(&pointer)
                .with_context(|| format!("reference '{reference}' not found"))?;
            let schema: Schema = serde_json::from_value(target.clone())
                .with_context(|| format!("reference '{reference}' is not a valid schema"))?;
            return self.resolve_schema(&schema);
        }

        bail!("unsupported reference {reference}")
    }
}

pub fn schema_reference(schema: &Schema) -> Option<&str> {
    match schema {
        Schema::Object(object) => object.reference.as_deref(),
        Schema::Bool(_) => None,
    }
}

fn overlay_reference_annotations(mut target: SchemaObject, source: &SchemaObject) -> SchemaObject {
    if let Some(source_metadata) = source.metadata.as_deref() {
        let mut merged = target.metadata.as_deref().cloned().unwrap_or_default();
        if let Some(title) = source_metadata.title.clone() {
            merged.title = Some(title);
        }
        if let Some(description) = source_metadata.description.clone() {
            merged.description = Some(description);
        }
        if source_metadata.default.is_some() {
            merged.default = source_metadata.default.clone();
        }
        if source_metadata.deprecated {
            merged.deprecated = true;
        }
        if source_metadata.read_only {
            merged.read_only = true;
        }
        if source_metadata.write_only {
            merged.write_only = true;
        }
        if !source_metadata.examples.is_empty() {
            merged.examples = source_metadata.examples.clone();
        }
        target.metadata = Some(Box::new(merged));
    }

    if !source.extensions.is_empty() {
        for (key, value) in &source.extensions {
            if key.starts_with("x-") {
                target.extensions.insert(key.clone(), value.clone());
            }
        }
    }

    target
}
