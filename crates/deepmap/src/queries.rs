// Placeholder for tree-sitter queries (language-specific S-expression patterns).

/// Tree-sitter query strings for each language.
/// Key: language name -> query type -> S-expression query string.
pub fn queries() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // Python
        (
            "python",
            "function",
            concat!(
                "(function_definition name: (identifier) @name) @definition.function\n",
                "(decorated_definition (function_definition name: (identifier) @name)) @definition.function\n",
                "(class_definition body: (block (function_definition name: (identifier) @name))) @definition.method\n",
                "(assignment left: (identifier) @name right: (lambda)) @definition.lambda",
            ),
        ),
        (
            "python",
            "class",
            concat!(
                "(class_definition name: (identifier) @name) @definition.class\n",
                "(decorated_definition (class_definition name: (identifier) @name)) @definition.class",
            ),
        ),
        (
            "python",
            "import",
            concat!(
                "(import_statement name: (dotted_name) @name)\n",
                "(import_statement name: (aliased_import name: (dotted_name) @name))\n",
                "(import_from_statement module_name: (dotted_name) @name)\n",
                "(import_from_statement module_name: (relative_import) @name)",
            ),
        ),
        (
            "python",
            "call",
            concat!(
                "(call function: (identifier) @name) @reference.call\n",
                "(call function: (attribute attribute: (identifier) @name)) @reference.call",
            ),
        ),
        // JavaScript
        (
            "javascript",
            "function",
            concat!(
                "(function_declaration name: (identifier) @name) @definition.function\n",
                "(variable_declarator name: (identifier) @name value: (arrow_function)) @definition.function\n",
                "(variable_declarator name: (identifier) @name value: (function_expression)) @definition.function\n",
                "(method_definition name: (property_identifier) @name) @definition.method",
            ),
        ),
        (
            "javascript",
            "anonymous_function",
            concat!(
                "(arrow_function) @definition.anonymous_function\n",
                "(function_expression) @definition.anonymous_function",
            ),
        ),
        (
            "javascript",
            "class",
            "(class_declaration name: (identifier) @name) @definition.class",
        ),
        (
            "javascript",
            "import",
            concat!(
                "(import_statement source: (string) @source)\n",
                "(import_specifier name: (identifier) @name)\n",
                "(import_clause (identifier) @name)",
            ),
        ),
        (
            "javascript",
            "call",
            concat!(
                "(call_expression function: (identifier) @name) @reference.call\n",
                "(call_expression function: (member_expression property: (property_identifier) @name)) @reference.call",
            ),
        ),
        // TypeScript
        (
            "typescript",
            "function",
            concat!(
                "(function_declaration name: (identifier) @name) @definition.function\n",
                "(variable_declarator name: (identifier) @name value: (arrow_function)) @definition.function\n",
                "(method_definition name: (property_identifier) @name) @definition.method",
            ),
        ),
        (
            "typescript",
            "anonymous_function",
            concat!(
                "(arrow_function) @definition.anonymous_function\n",
                "(function_expression) @definition.anonymous_function",
            ),
        ),
        (
            "typescript",
            "class",
            "(class_declaration name: (_) @name) @definition.class",
        ),
        (
            "typescript",
            "import",
            concat!(
                "(import_statement source: (string) @source)\n",
                "(import_specifier name: (identifier) @name)\n",
                "(import_clause (identifier) @name)",
            ),
        ),
        (
            "typescript",
            "call",
            concat!(
                "(call_expression function: (identifier) @name) @reference.call\n",
                "(call_expression function: (member_expression property: (property_identifier) @name)) @reference.call",
            ),
        ),
        // Go
        (
            "go",
            "function",
            concat!(
                "(function_declaration name: (identifier) @name) @definition.function\n",
                "(method_declaration name: (field_identifier) @name) @definition.method",
            ),
        ),
        (
            "go",
            "class",
            concat!(
                "(type_spec name: (type_identifier) @name type: (struct_type)) @definition.struct\n",
                "(type_spec name: (type_identifier) @name type: (interface_type)) @definition.interface",
            ),
        ),
        (
            "go",
            "import",
            "(import_spec path: (interpreted_string_literal) @path)",
        ),
        (
            "go",
            "call",
            concat!(
                "(call_expression function: (identifier) @name) @reference.call\n",
                "(call_expression function: (selector_expression field: (field_identifier) @name)) @reference.call",
            ),
        ),
        // Rust
        (
            "rust",
            "function",
            concat!(
                "(function_item name: (identifier) @name) @definition.function\n",
                "(function_signature_item name: (identifier) @name) @definition.trait_method",
            ),
        ),
        (
            "rust",
            "class",
            concat!(
                "(struct_item name: (type_identifier) @name) @definition.struct\n",
                "(enum_item name: (type_identifier) @name) @definition.enum\n",
                "(trait_item name: (type_identifier) @name) @definition.trait\n",
                "(impl_item type: (type_identifier) @name) @definition.impl\n",
                "(type_item name: (type_identifier) @name) @definition.type\n",
                "(mod_item name: (identifier) @name) @definition.module",
            ),
        ),
        (
            "rust",
            "import",
            concat!(
                "(use_declaration argument: (scoped_identifier path: (identifier) @path name: (identifier) @name))\n",
                "(use_declaration argument: (scoped_use_list path: (identifier) @path))\n",
                "(extern_crate_declaration name: (identifier) @name)\n",
                "(use_declaration argument: (identifier) @name)",
            ),
        ),
        (
            "rust",
            "call",
            concat!(
                "(call_expression function: (identifier) @name) @reference.call\n",
                "(call_expression function: (field_expression field: (field_identifier) @name)) @reference.call\n",
                "(call_expression function: (scoped_identifier name: (identifier) @name)) @reference.call",
            ),
        ),
    ]
}
