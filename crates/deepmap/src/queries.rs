// Tree-sitter query patterns for all supported languages.
//
// Each entry is (language, query_type, s-expression).
// query_type is one of: "function", "class", "import", "call", "http_route".
// Multiple patterns per query are concatenated into one s-expression string.

/// Return all tree-sitter query patterns for every supported language.
pub fn queries() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut result = Vec::with_capacity(84);

    // =========================================================================
    // Python
    // =========================================================================
    result.push((
        "python",
        "function",
        concat!(
            "(function_definition\n",
            "  name: (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "python",
        "class",
        concat!(
            "(class_definition\n",
            "  name: (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "python",
        "import",
        concat!(
            "(import_statement\n",
            "  (dotted_name) @name\n",
            ") @def\n",
            "(import_from_statement\n",
            "  module_name: (dotted_name) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "python",
        "call",
        concat!(
            "(call\n",
            "  function: (identifier) @name\n",
            ") @def\n",
            "(call\n",
            "  function: (attribute\n",
            "    attribute: (identifier) @name)\n",
            ") @def\n",
        ),
    ));
    result.push((
        "python",
        "http_route",
        concat!(
            "(decorated_definition\n",
            "  decorator: (decorator\n",
            "    (call\n",
            "      function: (attribute\n",
            "        attribute: (identifier) @route_method)\n",
            "      arguments: (argument_list\n",
            "        (string) @route_path)))\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // JavaScript
    // =========================================================================
    result.push((
        "javascript",
        "function",
        concat!(
            "(function_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(generator_function_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(method_definition\n",
            "  name: (property_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "javascript",
        "class",
        concat!(
            "(class_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "javascript",
        "import",
        concat!(
            "(import_statement\n",
            "  source: (string) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "javascript",
        "call",
        concat!(
            "(call_expression\n",
            "  function: (identifier) @name\n",
            ") @def\n",
            "(call_expression\n",
            "  function: (member_expression\n",
            "    property: (property_identifier) @name)\n",
            ") @def\n",
        ),
    ));
    result.push((
        "javascript",
        "http_route",
        concat!(
            "(call_expression\n",
            "  function: (member_expression\n",
            "    property: (property_identifier) @route_method)\n",
            "  arguments: (argument_list\n",
            "    . (string) @route_path)\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // TypeScript
    // =========================================================================
    result.push((
        "typescript",
        "function",
        concat!(
            "(function_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(generator_function_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(method_definition\n",
            "  name: (property_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "typescript",
        "class",
        concat!(
            "(class_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(interface_declaration\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
            "(type_alias_declaration\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
            "(enum_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "typescript",
        "import",
        concat!(
            "(import_statement\n",
            "  source: (string) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "typescript",
        "call",
        concat!(
            "(call_expression\n",
            "  function: (identifier) @name\n",
            ") @def\n",
            "(call_expression\n",
            "  function: (member_expression\n",
            "    property: (property_identifier) @name)\n",
            ") @def\n",
        ),
    ));
    result.push((
        "typescript",
        "http_route",
        concat!(
            "(call_expression\n",
            "  function: (member_expression\n",
            "    property: (property_identifier) @route_method)\n",
            "  arguments: (argument_list\n",
            "    . (string) @route_path)\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // Go
    // =========================================================================
    result.push((
        "go",
        "function",
        concat!(
            "(function_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(method_declaration\n",
            "  name: (field_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "go",
        "class",
        concat!(
            "(type_declaration\n",
            "  (type_spec\n",
            "    name: (type_identifier) @name)\n",
            ") @def\n",
        ),
    ));
    result.push((
        "go",
        "import",
        concat!(
            "(import_declaration\n",
            "  (import_spec\n",
            "    path: (interpreted_string_literal) @name)\n",
            ") @def\n",
        ),
    ));
    result.push((
        "go",
        "call",
        concat!(
            "(call_expression\n",
            "  function: (identifier) @name\n",
            ") @def\n",
            "(call_expression\n",
            "  function: (selector_expression\n",
            "    field: (field_identifier) @name)\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // Rust
    // =========================================================================
    result.push((
        "rust",
        "function",
        concat!(
            "(function_item\n",
            "  name: (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "rust",
        "class",
        concat!(
            "(struct_item\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
            "(enum_item\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
            "(trait_item\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
            "(type_item\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
            "(union_item\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "rust",
        "import",
        concat!(
            "(use_declaration\n",
            "  argument: (scoped_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "rust",
        "call",
        concat!(
            "(call_expression\n",
            "  function: (identifier) @name\n",
            ") @def\n",
            "(call_expression\n",
            "  function: (scoped_identifier\n",
            "    name: (identifier) @name)\n",
            ") @def\n",
            "(call_expression\n",
            "  function: (field_expression\n",
            "    field: (field_identifier) @name)\n",
            ") @def\n",
        ),
    ));
    result.push((
        "rust",
        "http_route",
        concat!(
            "(call_expression\n",
            "  function: (field_expression\n",
            "    field: (field_identifier) @route_method\n",
            "    argument: (scoped_identifier) @route_path)\n",
            ") @def\n",
            "(call_expression\n",
            "  function: (field_expression)\n",
            "  arguments: (arguments\n",
            "    (string_literal) @route_path)\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // HTML
    // =========================================================================
    result.push((
        "html",
        "function",
        concat!(
            "(element\n",
            "  (start_tag\n",
            "    (tag_name) @name)\n",
            ") @def\n",
        ),
    ));
    result.push((
        "html",
        "class",
        concat!(
            "(element\n",
            "  (start_tag\n",
            "    (tag_name) @name)\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // CSS
    // =========================================================================
    result.push((
        "css",
        "function",
        concat!(
            "(rule_set\n",
            "  (selectors\n",
            "    (class_selector\n",
            "      (class_name) @name))\n",
            ") @def\n",
            "(rule_set\n",
            "  (selectors\n",
            "    (id_selector\n",
            "      (id_name) @name))\n",
            ") @def\n",
        ),
    ));
    result.push((
        "css",
        "class",
        concat!(
            "(rule_set\n",
            "  (selectors\n",
            "    (class_selector\n",
            "      (class_name) @name))\n",
            ") @def\n",
            "(rule_set\n",
            "  (selectors\n",
            "    (type_selector\n",
            "      (tag_name) @name))\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // JSON
    // =========================================================================
    result.push((
        "json",
        "function",
        concat!(
            "(pair\n",
            "  key: (string) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "json",
        "class",
        concat!(
            "(pair\n",
            "  key: (string) @name\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // Java (NEW)
    // =========================================================================
    result.push((
        "java",
        "function",
        concat!(
            "(function_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(method_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "java",
        "class",
        concat!(
            "(class_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(interface_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "java",
        "import",
        concat!(
            "(import_declaration\n",
            "  (scoped_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "java",
        "call",
        concat!(
            "(method_invocation\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(object_creation\n",
            "  type: (type_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "java",
        "http_route",
        concat!(
            "(annotation\n",
            "  name: (identifier) @route_annotation\n",
            "  (annotation_argument_list\n",
            "    (string_literal) @route_path)\n",
            ") @def\n",
            "(annotation\n",
            "  name: (scoped_identifier) @route_annotation\n",
            "  (annotation_argument_list\n",
            "    (string_literal) @route_path)\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // Kotlin (NEW)
    // =========================================================================
    result.push((
        "kotlin",
        "function",
        concat!(
            "(function_declaration\n",
            "  name: (simple_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "kotlin",
        "class",
        concat!(
            "(class_declaration\n",
            "  name: (simple_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "kotlin",
        "import",
        concat!(
            "(import_header\n",
            "  (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "kotlin",
        "call",
        concat!(
            "(call_expression\n",
            "  function: (simple_identifier) @name\n",
            ") @def\n",
            "(call_expression\n",
            "  function: (navigation_expression\n",
            "    (simple_identifier) @name)\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // Swift (NEW)
    // =========================================================================
    result.push((
        "swift",
        "function",
        concat!(
            "(function_declaration\n",
            "  name: (simple_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "swift",
        "class",
        concat!(
            "(class_declaration\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
            "(struct_declaration\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
            "(enum_declaration\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
            "(protocol_declaration\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "swift",
        "import",
        concat!(
            "(import_statement\n",
            "  (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "swift",
        "call",
        concat!(
            "(call_expression\n",
            "  function: (simple_identifier) @name\n",
            ") @def\n",
            "(call_expression\n",
            "  function: (directly_identified_expression\n",
            "    (simple_identifier) @name)\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // C++ (NEW)
    // =========================================================================
    result.push((
        "cpp",
        "function",
        concat!(
            "(function_definition\n",
            "  declarator: (function_declarator\n",
            "    declarator: (identifier) @name)\n",
            ") @def\n",
            "(template_declaration\n",
            "  (function_definition\n",
            "    declarator: (function_declarator\n",
            "      declarator: (identifier) @name))\n",
            ") @def\n",
        ),
    ));
    result.push((
        "cpp",
        "class",
        concat!(
            "(class_specifier\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
            "(struct_specifier\n",
            "  name: (type_identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "cpp",
        "import",
        concat!(
            "(preproc_include\n",
            "  path: (string_literal) @name\n",
            ") @def\n",
            "(preproc_include\n",
            "  path: (system_lib_string) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "cpp",
        "call",
        concat!(
            "(call_expression\n",
            "  function: (identifier) @name\n",
            ") @def\n",
            "(call_expression\n",
            "  function: (field_expression\n",
            "    field: (field_identifier) @name)\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // C# (NEW)
    // =========================================================================
    result.push((
        "csharp",
        "function",
        concat!(
            "(method_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(local_function_statement\n",
            "  name: (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "csharp",
        "class",
        concat!(
            "(class_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(interface_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
            "(struct_declaration\n",
            "  name: (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "csharp",
        "import",
        concat!(
            "(using_directive\n",
            "  (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "csharp",
        "call",
        concat!(
            "(invocation_expression\n",
            "  function: (identifier) @name\n",
            ") @def\n",
            "(invocation_expression\n",
            "  function: (member_access_expression\n",
            "    name: (identifier) @name)\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // PHP (NEW)
    // =========================================================================
    result.push((
        "php",
        "function",
        concat!(
            "(function_definition\n",
            "  name: (name) @name\n",
            ") @def\n",
            "(method_declaration\n",
            "  name: (name) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "php",
        "class",
        concat!(
            "(class_declaration\n",
            "  name: (name) @name\n",
            ") @def\n",
            "(interface_declaration\n",
            "  name: (name) @name\n",
            ") @def\n",
            "(trait_declaration\n",
            "  name: (name) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "php",
        "import",
        concat!(
            "(require_once_expression\n",
            "  (string) @name\n",
            ") @def\n",
            "(require_expression\n",
            "  (string) @name\n",
            ") @def\n",
            "(include_expression\n",
            "  (string) @name\n",
            ") @def\n",
            "(include_once_expression\n",
            "  (string) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "php",
        "call",
        concat!(
            "(function_call_expression\n",
            "  function: (name) @name\n",
            ") @def\n",
            "(member_call_expression\n",
            "  name: (name) @name\n",
            ") @def\n",
        ),
    ));

    // =========================================================================
    // Ruby (NEW)
    // =========================================================================
    result.push((
        "ruby",
        "function",
        concat!(
            "(method\n",
            "  name: (identifier) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "ruby",
        "class",
        concat!(
            "(class\n",
            "  name: (constant) @name\n",
            ") @def\n",
            "(module\n",
            "  name: (constant) @name\n",
            ") @def\n",
        ),
    ));
    result.push((
        "ruby",
        "import",
        concat!(
            "(call\n",
            "  method: (identifier) @name\n",
            "  (argument_list\n",
            "    (string) @path)\n",
            ") @def\n",
        ),
    ));
    result.push((
        "ruby",
        "call",
        concat!(
            "(call\n",
            "  method: (identifier) @name\n",
            ") @def\n",
        ),
    ));

    result
}
