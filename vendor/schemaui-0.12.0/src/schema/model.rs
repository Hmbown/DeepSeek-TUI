use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstanceType {
    Null,
    Boolean,
    Object,
    Array,
    Number,
    String,
    Integer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SingleOrVec<T> {
    Single(Box<T>),
    Vec(Vec<T>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Schema {
    Bool(bool),
    Object(Box<SchemaObject>),
}

impl Schema {
    pub fn into_object(self) -> SchemaObject {
        match self {
            Self::Bool(_) => SchemaObject::default(),
            Self::Object(object) => *object,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, rename = "default", skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deprecated: bool,
    #[serde(default, rename = "readOnly", skip_serializing_if = "is_false")]
    pub read_only: bool,
    #[serde(default, rename = "writeOnly", skip_serializing_if = "is_false")]
    pub write_only: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Value>,
}

impl Metadata {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.description.is_none()
            && self.default.is_none()
            && !self.deprecated
            && !self.read_only
            && !self.write_only
            && self.examples.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SubschemaValidation {
    #[serde(
        default,
        rename = "allOf",
        skip_serializing_if = "option_vec_is_none_or_empty"
    )]
    pub all_of: Option<Vec<Schema>>,
    #[serde(
        default,
        rename = "anyOf",
        skip_serializing_if = "option_vec_is_none_or_empty"
    )]
    pub any_of: Option<Vec<Schema>>,
    #[serde(
        default,
        rename = "oneOf",
        skip_serializing_if = "option_vec_is_none_or_empty"
    )]
    pub one_of: Option<Vec<Schema>>,
}

impl SubschemaValidation {
    pub fn is_empty(&self) -> bool {
        option_vec_is_none_or_empty(&self.all_of)
            && option_vec_is_none_or_empty(&self.any_of)
            && option_vec_is_none_or_empty(&self.one_of)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ArrayValidation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<SingleOrVec<Schema>>,
    #[serde(default, rename = "minItems", skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    #[serde(default, rename = "maxItems", skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
}

impl ArrayValidation {
    pub fn is_empty(&self) -> bool {
        self.items.is_none() && self.min_items.is_none() && self.max_items.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ObjectValidation {
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub properties: IndexMap<String, Schema>,
    #[serde(
        default,
        rename = "patternProperties",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub pattern_properties: IndexMap<String, Schema>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(
        default,
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<Box<Schema>>,
    #[serde(
        default,
        rename = "propertyNames",
        skip_serializing_if = "Option::is_none"
    )]
    pub property_names: Option<Box<Schema>>,
}

impl ObjectValidation {
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
            && self.pattern_properties.is_empty()
            && self.required.is_empty()
            && self.additional_properties.is_none()
            && self.property_names.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SchemaObject {
    pub metadata: Option<Box<Metadata>>,
    pub instance_type: Option<SingleOrVec<InstanceType>>,
    pub format: Option<String>,
    pub enum_values: Option<Vec<Value>>,
    pub const_value: Option<Value>,
    pub subschemas: Option<Box<SubschemaValidation>>,
    pub array: Option<Box<ArrayValidation>>,
    pub object: Option<Box<ObjectValidation>>,
    pub reference: Option<String>,
    pub extensions: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
struct SchemaObjectRepr {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, rename = "default", skip_serializing_if = "Option::is_none")]
    default_value: Option<Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    deprecated: bool,
    #[serde(default, rename = "readOnly", skip_serializing_if = "is_false")]
    read_only: bool,
    #[serde(default, rename = "writeOnly", skip_serializing_if = "is_false")]
    write_only: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    examples: Vec<Value>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    instance_type: Option<SingleOrVec<InstanceType>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(
        default,
        rename = "enum",
        skip_serializing_if = "option_vec_is_none_or_empty"
    )]
    enum_values: Option<Vec<Value>>,
    #[serde(default, rename = "const", skip_serializing_if = "Option::is_none")]
    const_value: Option<Value>,
    #[serde(flatten)]
    subschemas: SubschemaValidation,
    #[serde(flatten)]
    array: ArrayValidation,
    #[serde(flatten)]
    object: ObjectValidation,
    #[serde(default, rename = "$ref", skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "Map::is_empty")]
    extensions: Map<String, Value>,
}

impl<'de> Deserialize<'de> for SchemaObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let repr = SchemaObjectRepr::deserialize(deserializer)?;
        let metadata = Metadata {
            title: repr.title,
            description: repr.description,
            default: repr.default_value,
            deprecated: repr.deprecated,
            read_only: repr.read_only,
            write_only: repr.write_only,
            examples: repr.examples,
        };

        Ok(Self {
            metadata: (!metadata.is_empty()).then_some(Box::new(metadata)),
            instance_type: repr.instance_type,
            format: repr.format,
            enum_values: empty_vec_to_none(repr.enum_values),
            const_value: repr.const_value,
            subschemas: (!repr.subschemas.is_empty()).then_some(Box::new(repr.subschemas)),
            array: (!repr.array.is_empty()).then_some(Box::new(repr.array)),
            object: (!repr.object.is_empty()).then_some(Box::new(repr.object)),
            reference: repr.reference,
            extensions: repr.extensions,
        })
    }
}

impl Serialize for SchemaObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let metadata = self.metadata.as_deref().cloned().unwrap_or_default();
        let repr = SchemaObjectRepr {
            title: metadata.title,
            description: metadata.description,
            default_value: metadata.default,
            deprecated: metadata.deprecated,
            read_only: metadata.read_only,
            write_only: metadata.write_only,
            examples: metadata.examples,
            instance_type: self.instance_type.clone(),
            format: self.format.clone(),
            enum_values: empty_vec_to_none(self.enum_values.clone()),
            const_value: self.const_value.clone(),
            subschemas: self.subschemas.as_deref().cloned().unwrap_or_default(),
            array: self.array.as_deref().cloned().unwrap_or_default(),
            object: self.object.as_deref().cloned().unwrap_or_default(),
            reference: self.reference.clone(),
            extensions: self.extensions.clone(),
        };
        repr.serialize(serializer)
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn option_vec_is_none_or_empty<T>(value: &Option<Vec<T>>) -> bool {
    match value {
        Some(inner) => inner.is_empty(),
        None => true,
    }
}

fn empty_vec_to_none<T>(value: Option<Vec<T>>) -> Option<Vec<T>> {
    match value {
        Some(inner) if inner.is_empty() => None,
        other => other,
    }
}
