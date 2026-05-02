use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use crate::io::{DocumentFormat, input::parse_document_str, input::schema_with_defaults};
use crate::precompile;
use crate::precompile::tui as ct_tui;
use crate::tui::model::form_schema_from_ui_ast;
use crate::tui::state::LayoutNavModel;
use crate::ui_ast::{build_ui_ast, index::collect_pointers, layout::build_ui_layout};

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("schemas")
        .join("test-comprehensive.schema.json")
}

fn ultra_complex_example_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("ultra-complex.schema.json")
}

fn defaults_value() -> Value {
    json!({
        "simpleTypes": {
            "text": "hello from defaults",
            "number": 7,
            "toggle": true,
            "dropdown": "option2"
        }
    })
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "schemaui_{prefix}_{}_{}",
        std::process::id(),
        stamp
    ))
}

fn write_defaults_file(prefix: &str, defaults: &Value) -> (PathBuf, PathBuf) {
    let out_dir = unique_temp_dir(prefix);
    fs::create_dir_all(&out_dir).expect("tmp dir creatable");
    let defaults_path = out_dir.join("defaults.json");
    fs::write(
        &defaults_path,
        serde_json::to_vec_pretty(defaults).expect("serialize defaults"),
    )
    .expect("write defaults file");
    (out_dir, defaults_path)
}

#[test]
fn generated_ui_ast_matches_runtime_for_comprehensive_schema() {
    let path = schema_path();

    let generated_ast =
        precompile::build_ui_ast_from_file(&path, DocumentFormat::Json).expect("build UiAst");

    let contents = std::fs::read_to_string(&path).expect("schema file readable");
    let schema_value: Value =
        parse_document_str(&contents, DocumentFormat::Json).expect("schema parses at runtime");
    let runtime_ast = build_ui_ast(&schema_value).expect("runtime UiAst");

    let generated_pointers = collect_pointers(&generated_ast);
    let runtime_pointers = collect_pointers(&runtime_ast);

    assert_eq!(runtime_pointers, generated_pointers);
}

#[test]
fn ultra_complex_example_builds_artifacts_and_keeps_recursive_tree_fields() {
    let path = ultra_complex_example_path();

    let bundle = precompile::build_ui_artifact_bundle_from_file(&path, DocumentFormat::Json, None)
        .expect("build UI artifact bundle for ultra-complex example");
    let pointers = collect_pointers(&bundle.ui.ui_ast);
    assert!(
        pointers.contains("/recursiveTree"),
        "pointers: {:?}",
        pointers
    );
    assert!(
        pointers.contains("/recursiveTree/name"),
        "recursive tree should still expose concrete fields"
    );
    assert!(
        pointers.contains("/recursiveTree/children"),
        "recursive tree should keep the children array boundary"
    );

    let recursive_root = bundle
        .tui
        .form_schema
        .roots
        .iter()
        .find(|root| root.id == "recursiveTree")
        .expect("recursive tree root");
    let section = recursive_root
        .sections
        .first()
        .expect("recursive tree section");
    let child_field = section
        .fields
        .iter()
        .find(|field| field.pointer == "/recursiveTree/children")
        .expect("recursive children field");
    assert!(
        matches!(child_field.kind, crate::tui::model::FieldKind::Array(_)),
        "recursive children should remain an array field"
    );
}

#[test]
fn ui_ast_json_roundtrip_preserves_structure() {
    let path = schema_path();

    let original =
        precompile::build_ui_ast_from_file(&path, DocumentFormat::Json).expect("build UiAst");
    let json = precompile::ui_ast_to_json(&original).expect("UiAst to JSON");
    let decoded = precompile::decode_ui_ast(&json).expect("decode UiAst from JSON");

    assert_eq!(original, decoded);
}

#[test]
fn ui_ast_bundle_json_roundtrip_preserves_structure() {
    let path = schema_path();

    let original = precompile::build_ui_ast_bundle_from_file(&path, DocumentFormat::Json)
        .expect("build UiAst bundle");
    let json = precompile::ui_ast_bundle_to_json(&original).expect("UiAst bundle to JSON");
    let decoded = precompile::decode_ui_ast_bundle(&json).expect("decode UiAst bundle from JSON");

    assert_eq!(original, decoded);
}

#[test]
fn ui_artifact_bundle_roundtrip_preserves_structure() {
    let path = schema_path();
    let defaults = defaults_value();
    let (out_dir, defaults_path) = write_defaults_file("ui_artifact_bundle_roundtrip", &defaults);

    let original = precompile::build_ui_artifact_bundle_from_file(
        &path,
        DocumentFormat::Json,
        Some(&defaults_path),
    )
    .expect("build UI artifact bundle");
    let json =
        precompile::ui_artifact_bundle_to_json(&original).expect("UI artifact bundle to JSON");
    let decoded =
        precompile::decode_ui_artifact_bundle(&json).expect("decode UI artifact bundle from JSON");

    assert_eq!(original, decoded);

    let _ = fs::remove_file(&defaults_path);
    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn ui_artifact_bundle_version_and_fingerprint_track_schema_and_defaults() {
    let path = schema_path();
    let defaults_a = json!({"simpleTypes": {"text": "a", "dropdown": "option1"}});
    let defaults_b = json!({"simpleTypes": {"text": "b", "dropdown": "option2"}});

    let contents = fs::read_to_string(&path).expect("schema file readable");
    let schema_value: Value =
        parse_document_str(&contents, DocumentFormat::Json).expect("schema parses at runtime");

    let bundle_a = precompile::build_ui_artifact_bundle(&schema_value, Some(&defaults_a))
        .expect("build bundle a");
    let bundle_a_again = precompile::build_ui_artifact_bundle(&schema_value, Some(&defaults_a))
        .expect("build bundle a again");
    let bundle_b = precompile::build_ui_artifact_bundle(&schema_value, Some(&defaults_b))
        .expect("build bundle b");

    assert_eq!(
        bundle_a.artifact_version,
        precompile::UI_ARTIFACT_BUNDLE_VERSION
    );
    assert_eq!(bundle_a.fingerprint, bundle_a_again.fingerprint);
    assert_eq!(
        bundle_a.fingerprint.schema_sha256,
        bundle_b.fingerprint.schema_sha256
    );
    assert_ne!(
        bundle_a.fingerprint.defaults_sha256,
        bundle_b.fingerprint.defaults_sha256
    );
    assert_ne!(
        bundle_a.fingerprint.input_sha256,
        bundle_b.fingerprint.input_sha256
    );
}

#[test]
fn generated_form_schema_matches_runtime_form_schema() {
    let path = schema_path();
    let defaults = defaults_value();
    let (out_dir, defaults_path) = write_defaults_file("tui_form_schema", &defaults);

    // 1) Generated-artifact build via precompile::tui helper.
    let (ct_ast, ct_form) =
        ct_tui::build_tui_form_schema_from_file(&path, DocumentFormat::Json, Some(&defaults_path))
            .expect("build FormSchema artifact");

    // 2) Runtime-style build from the same schema file.
    let contents = fs::read_to_string(&path).expect("schema file readable");
    let schema_value: Value =
        parse_document_str(&contents, DocumentFormat::Json).expect("schema parses at runtime");
    let enriched = schema_with_defaults(&schema_value, &defaults);
    let rt_ast = build_ui_ast(&enriched).expect("runtime UiAst");
    let rt_form = form_schema_from_ui_ast(&rt_ast);

    // 3) Assert UiAst and FormSchema equivalence (via JSON for FormSchema).
    if ct_ast != rt_ast {
        let ct_ptrs = collect_pointers(&ct_ast);
        let rt_ptrs = collect_pointers(&rt_ast);
        eprintln!(
            "UiAst mismatch: ct_pointers_len={} rt_pointers_len={}",
            ct_ptrs.len(),
            rt_ptrs.len()
        );
        let ct_sample: Vec<_> = ct_ptrs.iter().take(8).cloned().collect();
        let rt_sample: Vec<_> = rt_ptrs.iter().take(8).cloned().collect();
        eprintln!("  ct_pointers_sample={:?}", ct_sample);
        eprintln!("  rt_pointers_sample={:?}", rt_sample);

        // Persist full UiAst JSON to a temporary directory for manual diffing.
        let out_dir = std::env::var("CARGO_TARGET_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(
                    std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string()),
                )
                .join("tmp_tui_artifacts")
            });
        if let Err(err) = fs::create_dir_all(&out_dir) {
            eprintln!("  failed to create UiAst debug dir {:?}: {err}", out_dir);
        } else {
            let ct_json = serde_json::to_string_pretty(&ct_ast).unwrap();
            let rt_json = serde_json::to_string_pretty(&rt_ast).unwrap();
            let ct_path = out_dir.join("ct_ui_ast.json");
            let rt_path = out_dir.join("rt_ui_ast.json");
            if let Err(err) = fs::write(&ct_path, ct_json) {
                eprintln!("  failed to write {:?}: {err}", ct_path);
            }
            if let Err(err) = fs::write(&rt_path, rt_json) {
                eprintln!("  failed to write {:?}: {err}", rt_path);
            }
            eprintln!("  wrote UiAst JSON to {:?}", out_dir);
        }
    }
    assert_eq!(ct_ast, rt_ast, "generated and runtime UiAst must match");

    let ct_form_json = serde_json::to_value(&ct_form).expect("serialize ct FormSchema");
    let rt_form_json = serde_json::to_value(&rt_form).expect("serialize rt FormSchema");
    if ct_form_json != rt_form_json {
        eprintln!(
            "FormSchema JSON mismatch: ct_roots={} rt_roots={}",
            ct_form.roots.len(),
            rt_form.roots.len()
        );
        if let (Some(ct_root), Some(rt_root)) = (ct_form.roots.first(), rt_form.roots.first()) {
            eprintln!(
                " root[0]: ct_id={} ct_sections={} rt_id={} rt_sections={}",
                ct_root.id,
                ct_root.sections.len(),
                rt_root.id,
                rt_root.sections.len()
            );
        }
    }
    assert_eq!(
        ct_form_json, rt_form_json,
        "generated and runtime FormSchema JSON must match"
    );

    let _ = fs::remove_file(&defaults_path);
    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn generated_layout_nav_matches_runtime_layout_nav() {
    let path = schema_path();
    let defaults = defaults_value();
    let (out_dir, defaults_path) = write_defaults_file("tui_layout_nav", &defaults);

    // 1) Generated-artifact build via precompile::tui helper.
    let (ct_ast, ct_nav): (_, LayoutNavModel) =
        ct_tui::build_tui_layout_nav_from_file(&path, DocumentFormat::Json, Some(&defaults_path))
            .expect("build LayoutNavModel artifact");

    // 2) Runtime-style build from the same schema file.
    let contents = fs::read_to_string(&path).expect("schema file readable");
    let schema_value: Value =
        parse_document_str(&contents, DocumentFormat::Json).expect("schema parses at runtime");
    let enriched = schema_with_defaults(&schema_value, &defaults);
    let rt_ast = build_ui_ast(&enriched).expect("runtime UiAst");
    let rt_layout = build_ui_layout(&rt_ast);
    let rt_nav = LayoutNavModel::from_uilayout(&rt_layout);

    // 3) Assert UiAst and LayoutNavModel equivalence (via JSON for LayoutNavModel).
    if ct_ast != rt_ast {
        let ct_ptrs = collect_pointers(&ct_ast);
        let rt_ptrs = collect_pointers(&rt_ast);
        eprintln!(
            "UiAst mismatch: ct_pointers_len={} rt_pointers_len={}",
            ct_ptrs.len(),
            rt_ptrs.len()
        );
        let ct_sample: Vec<_> = ct_ptrs.iter().take(8).cloned().collect();
        let rt_sample: Vec<_> = rt_ptrs.iter().take(8).cloned().collect();
        eprintln!("  ct_pointers_sample={:?}", ct_sample);
        eprintln!("  rt_pointers_sample={:?}", rt_sample);
    }
    assert_eq!(ct_ast, rt_ast, "generated and runtime UiAst must match");

    let ct_nav_json = serde_json::to_value(&ct_nav).expect("serialize ct LayoutNavModel");
    let rt_nav_json = serde_json::to_value(&rt_nav).expect("serialize rt LayoutNavModel");
    if ct_nav_json != rt_nav_json {
        eprintln!(
            "LayoutNavModel JSON mismatch: ct_roots={} rt_roots={}",
            ct_nav.roots.len(),
            rt_nav.roots.len()
        );
        if let (Some(ct_root), Some(rt_root)) = (ct_nav.roots.first(), rt_nav.roots.first()) {
            eprintln!(
                " root[0]: ct_id={} ct_sections={} rt_id={} rt_sections={}",
                ct_root.id,
                ct_root.sections.len(),
                rt_root.id,
                rt_root.sections.len()
            );
            if let (Some(ct_section), Some(rt_section)) =
                (ct_root.sections.first(), rt_root.sections.first())
            {
                eprintln!(
                    " section[0]: ct_id={} ct_pointers_len={} rt_id={} rt_pointers_len={}",
                    ct_section.id,
                    ct_section.pointers.len(),
                    rt_section.id,
                    rt_section.pointers.len()
                );
                let ct_sample: Vec<_> = ct_section.pointers.iter().take(8).cloned().collect();
                let rt_sample: Vec<_> = rt_section.pointers.iter().take(8).cloned().collect();
                eprintln!("  ct_section_pointers_sample={:?}", ct_sample);
                eprintln!("  rt_section_pointers_sample={:?}", rt_sample);
            }
        }
    }
    assert_eq!(
        ct_nav_json, rt_nav_json,
        "generated and runtime LayoutNavModel JSON must match"
    );

    let _ = fs::remove_file(&defaults_path);
    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn generated_artifact_modules_compile_and_construct_artifacts() {
    let path = schema_path();
    let defaults = defaults_value();

    // 1) Generate modules into a temporary directory under target/.
    let (out_dir, defaults_path) = write_defaults_file("artifact_compile_smoke", &defaults);

    let tui_module = out_dir.join("tui_artifacts.rs");
    let form_module = out_dir.join("tui_form_schema.rs");
    let nav_module = out_dir.join("tui_layout_nav.rs");
    let bundle_module = out_dir.join("ui_artifact_bundle.rs");

    ct_tui::generate_tui_artifacts_module(
        &path,
        DocumentFormat::Json,
        Some(&defaults_path),
        &tui_module,
        "tui_artifacts",
    )
    .expect("generate TuiArtifacts module");
    ct_tui::generate_tui_form_schema_module(
        &path,
        DocumentFormat::Json,
        Some(&defaults_path),
        &form_module,
        "tui_form_schema",
    )
    .expect("generate FormSchema module");
    ct_tui::generate_tui_layout_nav_module(
        &path,
        DocumentFormat::Json,
        Some(&defaults_path),
        &nav_module,
        "tui_layout_nav",
    )
    .expect("generate LayoutNavModel module");
    precompile::generate_ui_artifact_bundle_module(
        &path,
        DocumentFormat::Json,
        Some(&defaults_path),
        &bundle_module,
        "ui_artifact_bundle",
    )
    .expect("generate UI artifact bundle module");

    // 2) Sanity-check that the generated files exist and contain the expected
    // function names.
    let tui_src = fs::read_to_string(&tui_module).expect("tui module readable");
    assert!(tui_src.contains("fn tui_artifacts"));

    let form_src = fs::read_to_string(&form_module).expect("form module readable");
    assert!(form_src.contains("fn tui_form_schema"));

    let nav_src = fs::read_to_string(&nav_module).expect("nav module readable");
    assert!(nav_src.contains("fn tui_layout_nav"));

    let bundle_src = fs::read_to_string(&bundle_module).expect("bundle module readable");
    assert!(bundle_src.contains("fn ui_artifact_bundle"));

    // 3) Compile the generated modules inside a temporary crate and construct
    // the generated artifacts at runtime.
    let smoke_crate_dir = out_dir.join("compile_smoke");
    fs::create_dir_all(smoke_crate_dir.join("src")).expect("smoke crate src creatable");
    fs::write(
        smoke_crate_dir.join("Cargo.toml"),
        smoke_crate_manifest(env!("CARGO_MANIFEST_DIR")),
    )
    .expect("write smoke crate manifest");
    fs::write(
        smoke_crate_dir.join("src/main.rs"),
        smoke_crate_main(&tui_module, &form_module, &nav_module, &bundle_module),
    )
    .expect("write smoke crate main");

    let target_dir = smoke_crate_dir.join("target");
    let output = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&smoke_crate_dir)
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("run smoke crate");
    if !output.status.success() {
        eprintln!("stdout:\n{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    assert!(
        output.status.success(),
        "smoke crate should compile and run"
    );

    let _ = fs::remove_dir_all(&out_dir);
}

fn smoke_crate_manifest(manifest_dir: &str) -> String {
    format!(
        r#"[package]
name = "schemaui-precompile-smoke"
version = "0.1.0"
edition = "2024"

[dependencies]
schemaui = {{ path = "{manifest_dir}", default-features = false, features = ["json", "tui", "precompile"] }}
serde_json = "1"
"#
    )
}

fn smoke_crate_main(
    tui_module: &Path,
    form_module: &Path,
    nav_module: &Path,
    bundle_module: &Path,
) -> String {
    format!(
        r##"include!(r#"{tui}"#);
include!(r#"{form}"#);
include!(r#"{nav}"#);
include!(r#"{bundle}"#);

fn main() {{
    let tui = tui_artifacts();
    let form = tui_form_schema();
    let nav = tui_layout_nav();
    let bundle = ui_artifact_bundle();

    assert!(!tui.form_schema.roots.is_empty());
    assert!(!tui.layout_nav.roots.is_empty());
    assert!(!form.roots.is_empty());
    assert!(!nav.roots.is_empty());
    assert!(!bundle.ui.ui_ast.roots.is_empty());
    assert_eq!(tui.form_schema, form);
    assert_eq!(tui.layout_nav, nav);
    assert_eq!(bundle.tui, tui);
    assert_eq!(
        bundle.artifact_version,
        schemaui::precompile::UI_ARTIFACT_BUNDLE_VERSION
    );
}}
"##,
        tui = tui_module.display(),
        form = form_module.display(),
        nav = nav_module.display(),
        bundle = bundle_module.display(),
    )
}
