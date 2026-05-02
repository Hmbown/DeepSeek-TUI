use std::path::Path;

use crate::{DocumentFormat, DocumentFormatProbe};

#[test]
fn default_format_matches_first_available_format() {
    let available = DocumentFormat::available_formats();
    assert!(!available.is_empty());
    assert_eq!(DocumentFormat::default(), available[0]);
}

#[test]
fn enabled_keywords_round_trip() {
    for format in DocumentFormat::available_formats() {
        let keyword = format.to_string();
        assert_eq!(DocumentFormat::from_keyword(&keyword), Ok(format));
    }
}

#[test]
fn enabled_extensions_round_trip() {
    #[cfg(feature = "json")]
    assert_eq!(
        DocumentFormat::probe_extension(Path::new("config.json")),
        DocumentFormatProbe::Known(DocumentFormat::Json)
    );

    #[cfg(feature = "yaml")]
    {
        assert_eq!(
            DocumentFormat::probe_extension(Path::new("config.yaml")),
            DocumentFormatProbe::Known(DocumentFormat::Yaml)
        );
        assert_eq!(
            DocumentFormat::probe_extension(Path::new("config.yml")),
            DocumentFormatProbe::Known(DocumentFormat::Yaml)
        );
    }

    #[cfg(feature = "toml")]
    assert_eq!(
        DocumentFormat::probe_extension(Path::new("config.toml")),
        DocumentFormatProbe::Known(DocumentFormat::Toml)
    );
}

#[cfg(not(feature = "json"))]
#[test]
fn json_keyword_reports_missing_feature() {
    assert_eq!(
        DocumentFormat::from_keyword("json"),
        Err("format 'json' requires the 'json' feature".to_string())
    );
    assert_eq!(
        DocumentFormat::probe_extension(Path::new("config.json")),
        DocumentFormatProbe::UnsupportedFeature {
            format_name: "json",
            feature_flag: "json",
        }
    );
}

#[cfg(not(feature = "yaml"))]
#[test]
fn yaml_keyword_reports_missing_feature() {
    assert_eq!(
        DocumentFormat::from_keyword("yaml"),
        Err("format 'yaml' requires the 'yaml' feature".to_string())
    );
    assert_eq!(
        DocumentFormat::probe_extension(Path::new("config.yaml")),
        DocumentFormatProbe::UnsupportedFeature {
            format_name: "yaml",
            feature_flag: "yaml",
        }
    );
}

#[cfg(not(feature = "toml"))]
#[test]
fn toml_keyword_reports_missing_feature() {
    assert_eq!(
        DocumentFormat::from_keyword("toml"),
        Err("format 'toml' requires the 'toml' feature".to_string())
    );
    assert_eq!(
        DocumentFormat::probe_extension(Path::new("config.toml")),
        DocumentFormatProbe::UnsupportedFeature {
            format_name: "toml",
            feature_flag: "toml",
        }
    );
}
