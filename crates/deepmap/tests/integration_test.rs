// Integration tests for DeepMap engine.

#[test]
fn test_scan_own_crate() {
    let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = crate_dir.parent().unwrap().parent().unwrap(); // workspace root

    let mut engine = deepmap::engine::RepoMapEngine::new(project_root);
    engine.scan(200, 60.0); // scan up to 200 files, 60s timeout

    assert!(engine.is_scanned(), "Engine should be in scanned state");
    assert!(
        !engine.graph.symbols.is_empty(),
        "Should have found symbols"
    );
    assert!(
        !engine.graph.file_symbols.is_empty(),
        "Should have file-symbol mappings"
    );

    // Verify we found some Rust files.
    let rust_files: Vec<_> = engine
        .graph
        .file_symbols
        .keys()
        .filter(|f| f.ends_with(".rs"))
        .collect();
    assert!(!rust_files.is_empty(), "Should have scanned Rust files");

    // Entry points should include main.rs.
    let entries = engine.entry_points();
    let has_main = entries.iter().any(|e| e.contains("main.rs"));
    assert!(has_main, "Should find main.rs as entry point");

    // Query a known symbol.
    let results = engine.query_symbol("main");
    assert!(!results.is_empty(), "Should find 'main' symbol");

    // Overview report should be non-empty.
    let report = deepmap::renderer::render_overview_report(&engine, 8000);
    assert!(!report.is_empty(), "Overview report should not be empty");
    assert!(report.contains("Project Map"), "Report should have title");
}

#[test]
fn test_call_chain() {
    let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = crate_dir.parent().unwrap().parent().unwrap();

    let mut engine = deepmap::engine::RepoMapEngine::new(project_root);
    engine.scan(200, 60.0);

    // Query a symbol that should exist.
    let results = engine.query_symbol("scan");
    if !results.is_empty() {
        let sym = &results[0];
        let chains = engine.call_chain(&sym.id, "both", 2);
        assert!(
            chains.contains_key("callers") || chains.contains_key("callees"),
            "Call chain should return at least one direction"
        );
    }
}

#[test]
fn test_hotspots_and_reading_order() {
    let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = crate_dir.parent().unwrap().parent().unwrap();

    let mut engine = deepmap::engine::RepoMapEngine::new(project_root);
    engine.scan(200, 60.0);

    let hotspots = engine.hotspots(5);
    assert!(!hotspots.is_empty(), "Should have hotspots");

    let reading = engine.suggested_reading_order(5);
    assert!(!reading.is_empty(), "Should have reading order");

    let modules = engine.module_summary(3);
    assert!(!modules.is_empty(), "Should have module summary");
}

#[test]
fn test_query_report() {
    let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = crate_dir.parent().unwrap().parent().unwrap();

    let mut engine = deepmap::engine::RepoMapEngine::new(project_root);
    engine.scan(200, 60.0);

    let report = deepmap::renderer::render_query_report(&engine, "scan symbols", 15, 8000);
    assert!(!report.is_empty());
    eprintln!("=== Query Report (first 500 chars) ===");
    eprintln!("{}", &report[..report.len().min(500)]);
}

#[test]
fn test_renderer() {
    let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_root = crate_dir.parent().unwrap().parent().unwrap();

    let mut engine = deepmap::engine::RepoMapEngine::new(project_root);
    engine.scan(200, 60.0);

    let report = deepmap::renderer::render_overview_report(&engine, 16000);
    assert!(!report.is_empty());
    eprintln!("=== Overview Report (first 500 chars) ===");
    eprintln!("{}", &report[..report.len().min(500)]);

    // File detail for a Rust file.
    if let Some(file) = engine
        .graph
        .file_symbols
        .keys()
        .find(|f| f.ends_with(".rs"))
    {
        let detail = deepmap::renderer::render_file_detail_report(&engine, file, 10, 4000);
        assert!(!detail.is_empty());
        eprintln!("=== File Detail (first 300 chars) ===");
        eprintln!("{}", &detail[..detail.len().min(300)]);
    }

    // Call chain report.
    let results = engine.query_symbol("scan");
    if !results.is_empty() {
        let report = deepmap::renderer::render_call_chain_report(&engine, "scan", 2);
        assert!(!report.is_empty());
        eprintln!("=== Call Chain (first 300 chars) ===");
        eprintln!("{}", &report[..report.len().min(300)]);
    }
}
