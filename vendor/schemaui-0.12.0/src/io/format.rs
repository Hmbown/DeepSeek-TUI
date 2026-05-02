use std::{fmt, path::Path};

#[cfg(not(any(feature = "json", feature = "yaml", feature = "toml")))]
compile_error!("schemaui requires at least one document format feature: json, yaml, or toml");

/// Supported data formats for input/output layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormat {
    #[cfg(feature = "json")]
    Json,
    #[cfg(feature = "yaml")]
    Yaml,
    #[cfg(feature = "toml")]
    Toml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormatProbe {
    Known(DocumentFormat),
    UnsupportedFeature {
        format_name: &'static str,
        feature_flag: &'static str,
    },
    Unknown,
}

impl Default for DocumentFormat {
    fn default() -> Self {
        #[cfg(feature = "json")]
        {
            Self::Json
        }
        #[cfg(all(not(feature = "json"), feature = "yaml"))]
        {
            Self::Yaml
        }
        #[cfg(all(not(feature = "json"), not(feature = "yaml"), feature = "toml"))]
        {
            Self::Toml
        }
    }
}

impl fmt::Display for DocumentFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "json")]
            DocumentFormat::Json => write!(f, "json"),
            #[cfg(feature = "yaml")]
            DocumentFormat::Yaml => write!(f, "yaml"),
            #[cfg(feature = "toml")]
            DocumentFormat::Toml => write!(f, "toml"),
        }
    }
}

impl DocumentFormat {
    /// Parse a format keyword (json/yaml/toml) into a `DocumentFormat`.
    pub fn from_keyword(keyword: &str) -> Result<Self, String> {
        match Self::probe_keyword(keyword) {
            DocumentFormatProbe::Known(format) => Ok(format),
            DocumentFormatProbe::UnsupportedFeature {
                format_name,
                feature_flag,
            } => Err(format!(
                "format '{format_name}' requires the '{feature_flag}' feature"
            )),
            DocumentFormatProbe::Unknown => Err(format!(
                "unsupported format '{}', available: {}",
                keyword,
                Self::keyword_list().join(", ")
            )),
        }
    }

    /// Try to infer a format from a file extension.
    pub fn from_extension(path: &Path) -> Option<Self> {
        match Self::probe_extension(path) {
            DocumentFormatProbe::Known(format) => Some(format),
            DocumentFormatProbe::UnsupportedFeature { .. } | DocumentFormatProbe::Unknown => None,
        }
    }

    pub fn probe_keyword(keyword: &str) -> DocumentFormatProbe {
        match keyword.to_ascii_lowercase().as_str() {
            #[cfg(feature = "json")]
            "json" => DocumentFormatProbe::Known(DocumentFormat::Json),
            #[cfg(not(feature = "json"))]
            "json" => DocumentFormatProbe::UnsupportedFeature {
                format_name: "json",
                feature_flag: "json",
            },
            #[cfg(feature = "yaml")]
            "yaml" | "yml" => DocumentFormatProbe::Known(DocumentFormat::Yaml),
            #[cfg(not(feature = "yaml"))]
            "yaml" | "yml" => DocumentFormatProbe::UnsupportedFeature {
                format_name: "yaml",
                feature_flag: "yaml",
            },
            #[cfg(feature = "toml")]
            "toml" => DocumentFormatProbe::Known(DocumentFormat::Toml),
            #[cfg(not(feature = "toml"))]
            "toml" => DocumentFormatProbe::UnsupportedFeature {
                format_name: "toml",
                feature_flag: "toml",
            },
            _ => DocumentFormatProbe::Unknown,
        }
    }

    pub fn probe_extension(path: &Path) -> DocumentFormatProbe {
        let Some(ext) = path.extension() else {
            return DocumentFormatProbe::Unknown;
        };
        let ext = ext.to_string_lossy().to_ascii_lowercase();
        match ext.as_str() {
            #[cfg(feature = "json")]
            "json" => DocumentFormatProbe::Known(DocumentFormat::Json),
            #[cfg(not(feature = "json"))]
            "json" => DocumentFormatProbe::UnsupportedFeature {
                format_name: "json",
                feature_flag: "json",
            },
            #[cfg(feature = "yaml")]
            "yaml" | "yml" => DocumentFormatProbe::Known(DocumentFormat::Yaml),
            #[cfg(not(feature = "yaml"))]
            "yaml" | "yml" => DocumentFormatProbe::UnsupportedFeature {
                format_name: "yaml",
                feature_flag: "yaml",
            },
            #[cfg(feature = "toml")]
            "toml" => DocumentFormatProbe::Known(DocumentFormat::Toml),
            #[cfg(not(feature = "toml"))]
            "toml" => DocumentFormatProbe::UnsupportedFeature {
                format_name: "toml",
                feature_flag: "toml",
            },
            _ => DocumentFormatProbe::Unknown,
        }
    }

    pub fn keyword_list() -> Vec<&'static str> {
        vec![
            #[cfg(feature = "json")]
            "json",
            #[cfg(feature = "yaml")]
            "yaml",
            #[cfg(feature = "toml")]
            "toml",
        ]
    }

    pub fn available_formats() -> Vec<DocumentFormat> {
        vec![
            #[cfg(feature = "json")]
            DocumentFormat::Json,
            #[cfg(feature = "yaml")]
            DocumentFormat::Yaml,
            #[cfg(feature = "toml")]
            DocumentFormat::Toml,
        ]
    }

    pub fn format_list() -> &'static str {
        #[cfg(all(feature = "json", feature = "yaml", feature = "toml"))]
        {
            "JSON/YAML/TOML"
        }
        #[cfg(all(feature = "json", feature = "yaml", not(feature = "toml")))]
        {
            "JSON/YAML"
        }
        #[cfg(all(feature = "json", not(feature = "yaml"), feature = "toml"))]
        {
            "JSON/TOML"
        }
        #[cfg(all(not(feature = "json"), feature = "yaml", feature = "toml"))]
        {
            "YAML/TOML"
        }
        #[cfg(all(feature = "json", not(feature = "yaml"), not(feature = "toml")))]
        {
            "JSON"
        }
        #[cfg(all(not(feature = "json"), feature = "yaml", not(feature = "toml")))]
        {
            "YAML"
        }
        #[cfg(all(not(feature = "json"), not(feature = "yaml"), feature = "toml"))]
        {
            "TOML"
        }
    }
}
