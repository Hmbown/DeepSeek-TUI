use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaDialect {
    Draft7,
    Draft202012,
    Unknown,
}

impl SchemaDialect {
    pub fn detect(schema: &Value) -> Self {
        let Some(uri) = schema
            .as_object()
            .and_then(|obj| obj.get("$schema"))
            .and_then(Value::as_str)
        else {
            return Self::Unknown;
        };

        if uri.contains("draft-07") {
            Self::Draft7
        } else if uri.contains("2020-12") {
            Self::Draft202012
        } else {
            Self::Unknown
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RootDialectContext {
    pub dialect: SchemaDialect,
    pub schema_keyword: Option<Value>,
    pub dollar_defs: Option<Value>,
    pub definitions: Option<Value>,
}

impl RootDialectContext {
    pub fn from_root(schema: &Value) -> Self {
        let obj = schema.as_object();
        Self {
            dialect: SchemaDialect::detect(schema),
            schema_keyword: obj.and_then(|map| map.get("$schema").cloned()),
            dollar_defs: obj.and_then(|map| map.get("$defs").cloned()),
            definitions: obj.and_then(|map| map.get("definitions").cloned()),
        }
    }

    pub fn apply_to_overlay(&self, map: &mut Map<String, Value>) {
        if let Some(schema_keyword) = &self.schema_keyword {
            map.entry("$schema".to_string())
                .or_insert_with(|| schema_keyword.clone());
        }
        if let Some(definitions) = &self.definitions {
            map.entry("definitions".to_string())
                .or_insert_with(|| definitions.clone());
        }
        if let Some(dollar_defs) = &self.dollar_defs {
            map.entry("$defs".to_string())
                .or_insert_with(|| dollar_defs.clone());
        }
    }
}
