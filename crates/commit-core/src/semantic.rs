use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SemanticAnalysis {
    pub files: Vec<FileSemantic>,
    pub unsupported: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileSemantic {
    pub path: String,
    pub language: Language,
    pub symbols: Vec<SymbolChange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SymbolChange {
    pub kind: SymbolKind,
    pub name: String,
    pub visibility: Visibility,
    pub change: ChangeKind,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Union,
    Trait,
    Impl,
    Mod,
    Const,
    Static,
    TypeAlias,
    Macro,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    PubCrate,
    Private,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    BodyModified,
    SignatureChanged,
}

pub fn detect_language(path: &str) -> Option<Language> {
    if path.ends_with(".rs") {
        Some(Language::Rust)
    } else {
        None
    }
}

#[cfg(feature = "ast-rust")]
mod rust_impl {
    use super::*;
    use tree_sitter::{Node, Parser};

    #[derive(Debug)]
    pub(super) struct Symbol {
        pub kind: SymbolKind,
        pub name: String,
        pub visibility: Visibility,
        pub signature_text: String,
        pub full_text: String,
    }

    pub fn analyze(
        path: &str,
        old_source: Option<&str>,
        new_source: Option<&str>,
    ) -> Option<FileSemantic> {
        let language = detect_language(path)?;
        let old_symbols = extract_or_empty(old_source);
        let new_symbols = extract_or_empty(new_source);
        let changes = diff_symbols(&old_symbols, &new_symbols);
        if changes.is_empty() {
            None
        } else {
            Some(FileSemantic {
                path: path.to_string(),
                language,
                symbols: changes,
            })
        }
    }

    fn extract_or_empty(source: Option<&str>) -> Vec<Symbol> {
        source.and_then(extract_rust_symbols).unwrap_or_default()
    }

    pub(super) fn extract_rust_symbols(source: &str) -> Option<Vec<Symbol>> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .ok()?;
        let tree = parser.parse(source, None)?;
        let mut symbols = Vec::new();
        walk(tree.root_node(), source, &mut symbols, None);
        Some(symbols)
    }

    fn walk(node: Node, source: &str, symbols: &mut Vec<Symbol>, qualifier: Option<&str>) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "function_item" => {
                    if let Some(symbol) = build_function(child, source, qualifier) {
                        symbols.push(symbol);
                    }
                }
                "struct_item" => push_named(symbols, child, source, SymbolKind::Struct, qualifier),
                "enum_item" => push_named(symbols, child, source, SymbolKind::Enum, qualifier),
                "union_item" => push_named(symbols, child, source, SymbolKind::Union, qualifier),
                "trait_item" => push_named(symbols, child, source, SymbolKind::Trait, qualifier),
                "type_item" => {
                    push_named(symbols, child, source, SymbolKind::TypeAlias, qualifier);
                }
                "const_item" => push_named(symbols, child, source, SymbolKind::Const, qualifier),
                "static_item" => push_named(symbols, child, source, SymbolKind::Static, qualifier),
                "macro_definition" => {
                    push_named(symbols, child, source, SymbolKind::Macro, qualifier);
                }
                "mod_item" => {
                    push_named(symbols, child, source, SymbolKind::Mod, qualifier);
                }
                "impl_item" => {
                    let impl_qualifier = impl_qualifier(child, source);
                    if let Some(symbol) =
                        build_impl_summary(child, source, qualifier, &impl_qualifier)
                    {
                        symbols.push(symbol);
                    }
                    if let Some(body) = child.child_by_field_name("body") {
                        walk(body, source, symbols, Some(&impl_qualifier));
                    }
                }
                _ => {}
            }
        }
    }

    fn build_function(node: Node, source: &str, qualifier: Option<&str>) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = slice(source, name_node)?;
        let visibility = visibility_of(node, source);
        let body_node = node.child_by_field_name("body");
        let signature_end = body_node
            .map(|body| body.start_byte())
            .unwrap_or_else(|| node.end_byte());
        let signature_text = source.get(node.start_byte()..signature_end)?.to_string();
        let full_text = slice(source, node)?.to_string();
        Some(Symbol {
            kind: SymbolKind::Function,
            name: qualified(qualifier, name),
            visibility,
            signature_text,
            full_text,
        })
    }

    fn push_named(
        symbols: &mut Vec<Symbol>,
        node: Node,
        source: &str,
        kind: SymbolKind,
        qualifier: Option<&str>,
    ) {
        if let Some(name_node) = node.child_by_field_name("name") {
            let Some(name) = slice(source, name_node) else {
                return;
            };
            let visibility = visibility_of(node, source);
            let Some(full_text) = slice(source, node) else {
                return;
            };
            symbols.push(Symbol {
                kind,
                name: qualified(qualifier, name),
                visibility,
                signature_text: full_text.to_string(),
                full_text: full_text.to_string(),
            });
        }
    }

    fn build_impl_summary(
        node: Node,
        source: &str,
        outer_qualifier: Option<&str>,
        impl_qualifier: &str,
    ) -> Option<Symbol> {
        let header_end = node
            .child_by_field_name("body")
            .map(|body| body.start_byte())
            .unwrap_or_else(|| node.end_byte());
        let signature_text = source.get(node.start_byte()..header_end)?.to_string();
        let full_text = slice(source, node)?.to_string();
        Some(Symbol {
            kind: SymbolKind::Impl,
            name: qualified(outer_qualifier, impl_qualifier),
            visibility: Visibility::Private,
            signature_text,
            full_text,
        })
    }

    fn impl_qualifier(node: Node, source: &str) -> String {
        let type_text = node
            .child_by_field_name("type")
            .and_then(|node| slice(source, node))
            .unwrap_or("<unknown>")
            .trim()
            .to_string();
        if let Some(trait_node) = node.child_by_field_name("trait") {
            let trait_text = slice(source, trait_node).unwrap_or("<unknown>").trim();
            format!("{trait_text} for {type_text}")
        } else {
            type_text
        }
    }

    fn visibility_of(node: Node, source: &str) -> Visibility {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "visibility_modifier" {
                let text = slice(source, child).unwrap_or("").trim();
                if text.starts_with("pub(crate)") {
                    return Visibility::PubCrate;
                } else if text.starts_with("pub") {
                    return Visibility::Public;
                }
            }
        }
        Visibility::Private
    }

    fn slice<'a>(source: &'a str, node: Node) -> Option<&'a str> {
        source.get(node.start_byte()..node.end_byte())
    }

    fn qualified(qualifier: Option<&str>, name: &str) -> String {
        match qualifier {
            Some(q) => format!("{q}::{name}"),
            None => name.to_string(),
        }
    }

    pub(super) fn diff_symbols(old: &[Symbol], new: &[Symbol]) -> Vec<SymbolChange> {
        use std::collections::HashMap;

        let mut old_index: HashMap<(SymbolKind, &str), &Symbol> = HashMap::new();
        for symbol in old {
            old_index.insert((symbol.kind, symbol.name.as_str()), symbol);
        }

        let mut changes = Vec::new();
        let mut seen_in_old = std::collections::HashSet::new();

        for new_symbol in new {
            let key = (new_symbol.kind, new_symbol.name.as_str());
            if let Some(old_symbol) = old_index.get(&key) {
                seen_in_old.insert(key);
                if new_symbol.full_text == old_symbol.full_text {
                    continue;
                }
                let change = if matches!(new_symbol.kind, SymbolKind::Function)
                    && new_symbol.signature_text != old_symbol.signature_text
                {
                    ChangeKind::SignatureChanged
                } else {
                    ChangeKind::BodyModified
                };
                changes.push(SymbolChange {
                    kind: new_symbol.kind,
                    name: new_symbol.name.clone(),
                    visibility: new_symbol.visibility,
                    change,
                });
            } else {
                changes.push(SymbolChange {
                    kind: new_symbol.kind,
                    name: new_symbol.name.clone(),
                    visibility: new_symbol.visibility,
                    change: ChangeKind::Added,
                });
            }
        }

        for old_symbol in old {
            let key = (old_symbol.kind, old_symbol.name.as_str());
            if !seen_in_old.contains(&key) && !new.iter().any(|s| (s.kind, s.name.as_str()) == key)
            {
                changes.push(SymbolChange {
                    kind: old_symbol.kind,
                    name: old_symbol.name.clone(),
                    visibility: old_symbol.visibility,
                    change: ChangeKind::Removed,
                });
            }
        }

        changes
    }
}

#[cfg(feature = "ast-rust")]
pub fn analyze_file(
    path: &str,
    old_source: Option<&str>,
    new_source: Option<&str>,
) -> Option<FileSemantic> {
    rust_impl::analyze(path, old_source, new_source)
}

#[cfg(not(feature = "ast-rust"))]
pub fn analyze_file(
    _path: &str,
    _old_source: Option<&str>,
    _new_source: Option<&str>,
) -> Option<FileSemantic> {
    None
}

#[cfg(all(test, feature = "ast-rust"))]
mod tests {
    use super::*;
    use rust_impl::{diff_symbols, extract_rust_symbols};

    fn analyze(old: &str, new: &str) -> Vec<SymbolChange> {
        let old_symbols = extract_rust_symbols(old).expect("parse old");
        let new_symbols = extract_rust_symbols(new).expect("parse new");
        diff_symbols(&old_symbols, &new_symbols)
    }

    fn find<'a>(changes: &'a [SymbolChange], name: &str) -> &'a SymbolChange {
        changes
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("expected change for {name}: {changes:#?}"))
    }

    #[test]
    fn extracts_public_function_visibility() {
        let symbols = extract_rust_symbols("pub fn foo() {}\nfn bar() {}").expect("parse");
        let foo = symbols.iter().find(|s| s.name == "foo").expect("foo");
        let bar = symbols.iter().find(|s| s.name == "bar").expect("bar");
        assert_eq!(foo.visibility, Visibility::Public);
        assert_eq!(bar.visibility, Visibility::Private);
    }

    #[test]
    fn extracts_pub_crate_visibility() {
        let symbols = extract_rust_symbols("pub(crate) fn foo() {}").expect("parse");
        let foo = symbols.iter().find(|s| s.name == "foo").expect("foo");
        assert_eq!(foo.visibility, Visibility::PubCrate);
    }

    #[test]
    fn detects_added_function() {
        let old = "fn keep() {}\n";
        let new = "fn keep() {}\npub fn fresh() {}\n";
        let changes = analyze(old, new);
        let fresh = find(&changes, "fresh");
        assert_eq!(fresh.change, ChangeKind::Added);
        assert_eq!(fresh.kind, SymbolKind::Function);
        assert_eq!(fresh.visibility, Visibility::Public);
    }

    #[test]
    fn detects_removed_function() {
        let old = "fn keep() {}\npub fn gone() {}\n";
        let new = "fn keep() {}\n";
        let changes = analyze(old, new);
        let gone = find(&changes, "gone");
        assert_eq!(gone.change, ChangeKind::Removed);
        assert_eq!(gone.visibility, Visibility::Public);
    }

    #[test]
    fn detects_body_modified_function() {
        let old = "fn touched() { 1 }\n";
        let new = "fn touched() { 2 }\n";
        let changes = analyze(old, new);
        let touched = find(&changes, "touched");
        assert_eq!(touched.change, ChangeKind::BodyModified);
    }

    #[test]
    fn detects_signature_changed_function() {
        let old = "fn moved(x: u32) -> bool { true }\n";
        let new = "fn moved(x: u32, y: u32) -> bool { true }\n";
        let changes = analyze(old, new);
        let moved = find(&changes, "moved");
        assert_eq!(moved.change, ChangeKind::SignatureChanged);
    }

    #[test]
    fn detects_struct_body_modified() {
        let old = "pub struct Config { pub name: String }\n";
        let new = "pub struct Config { pub name: String, pub count: u32 }\n";
        let changes = analyze(old, new);
        let config = find(&changes, "Config");
        assert_eq!(config.kind, SymbolKind::Struct);
        assert_eq!(config.change, ChangeKind::BodyModified);
        assert_eq!(config.visibility, Visibility::Public);
    }

    #[test]
    fn qualifies_methods_with_impl_type() {
        let new = r#"
            pub struct Foo;
            impl Foo {
                pub fn new() -> Self { Foo }
            }
        "#;
        let symbols = extract_rust_symbols(new).expect("parse");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"));
        assert!(
            names.contains(&"Foo::new"),
            "expected Foo::new in {names:?}"
        );
    }

    #[test]
    fn unchanged_files_yield_no_changes() {
        let source = "pub fn stable() {}\npub struct Stable;\n";
        let changes = analyze(source, source);
        assert!(changes.is_empty(), "expected no changes, got {changes:#?}");
    }

    #[test]
    fn analyze_file_returns_none_for_non_rust_paths() {
        assert!(analyze_file("README.md", Some(""), Some("hi")).is_none());
    }

    #[test]
    fn malformed_rust_does_not_panic_and_extracts_what_it_can() {
        let new = "pub fn ok() {}\nfn broken( {\n";
        let symbols = extract_rust_symbols(new).expect("parser tolerates broken source");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"ok"),
            "should still extract `ok` from partially valid source: {names:?}"
        );
    }
}
