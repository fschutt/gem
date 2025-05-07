use ignore::WalkBuilder;
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use syn::{
    visit::{self, Visit},
    ItemConst, ItemEnum, ItemFn, ItemImpl, ItemMacro, ItemMod, ItemStatic, ItemStruct, ItemTrait,
    ItemType,
};

/// A visitor that collects Rust symbols during parsing
pub struct SymbolCollector {
    crate_name: String,
    base_path: PathBuf,
    current_path: Vec<String>,
    symbols: HashMap<String, SymbolInfo>,
    current_file: PathBuf,
}

impl SymbolCollector {
    pub fn new(crate_name: &str, base_path: &Path) -> Self {
        Self {
            crate_name: crate_name.to_string(),
            base_path: base_path.to_path_buf(),
            current_path: Vec::new(),
            symbols: HashMap::new(),
            current_file: PathBuf::new(),
        }
    }

    pub fn get_symbols(&self) -> &HashMap<String, SymbolInfo> {
        &self.symbols
    }

    fn current_module_path(&self) -> Vec<String> {
        let rel_path = match self.current_file.strip_prefix(&self.base_path) {
            Ok(p) => p.to_path_buf(),
            Err(_) => return vec![self.crate_name.clone()],
        };

        let mut module_path = vec![self.crate_name.clone()];

        // Process path components
        let components: Vec<_> = rel_path.components().collect();
        for (i, comp) in components.iter().enumerate() {
            if let std::path::Component::Normal(name) = comp {
                let name_str = name.to_string_lossy();

                // Skip src directory
                if i == 0 && name_str == "src" {
                    continue;
                }

                // Handle special files
                if i == components.len() - 1 {
                    let file_name = name_str.to_string();
                    if file_name == "mod.rs" {
                        // mod.rs doesn't add to path
                        continue;
                    } else if file_name == "lib.rs" || file_name == "main.rs" {
                        // lib.rs and main.rs are the crate root
                        module_path = vec![self.crate_name.clone()];
                        continue;
                    } else if file_name.ends_with(".rs") {
                        // Regular .rs file, add without extension
                        let module_name = file_name.trim_end_matches(".rs");
                        module_path.push(module_name.to_string());
                    }
                } else {
                    // Directory name becomes a module
                    module_path.push(name_str.to_string());
                }
            }
        }

        module_path
    }

    fn add_symbol(&mut self, name: &str, symbol_type: SymbolType, doc: String) {
        // Base path from file location
        let mut path = self.current_module_path();

        // Add current traversal path for nested items
        for part in &self.current_path {
            path.push(part.clone());
        }

        // Add the item name
        path.push(name.to_string());
        let full_path = path.join("::");

        self.symbols.insert(
            full_path.clone(),
            SymbolInfo {
                identifier: name.to_string(),
                symbol_type,
                hover_text: doc,
                source_vertex_id: format!("{:?}", self.current_file),
            },
        );
    }

    fn enter_module(&mut self, module_name: &str) {
        self.current_path.push(module_name.to_string());
    }

    fn exit_module(&mut self) {
        self.current_path.pop();
    }

    fn extract_doc(&self, attrs: &[syn::Attribute]) -> String {
        let mut doc = String::new();
        for attr in attrs {
            if attr.path().is_ident("doc") {
                if let Ok(meta) = attr.meta.require_name_value() {
                    if let syn::Expr::Lit(expr_lit) = &meta.value {
                        if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                            let doc_line = lit_str.value();

                            // If docstring has a '#', only include content up to that
                            if let Some(idx) = doc_line.find('#') {
                                if !doc.is_empty() {
                                    doc.push('\n');
                                }
                                doc.push_str(doc_line[..idx].trim());
                                continue;
                            }

                            if !doc.is_empty() {
                                doc.push('\n');
                            }
                            doc.push_str(doc_line.trim());
                        }
                    }
                }
            }
        }
        doc
    }

    fn has_async_attr(&self, attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| attr.path().is_ident("async"))
    }
}

impl<'ast> Visit<'ast> for SymbolCollector {
    fn visit_item_struct(&mut self, i: &'ast ItemStruct) {
        let doc = self.extract_doc(&i.attrs);
        self.add_symbol(&i.ident.to_string(), SymbolType::Struct, doc);

        // Add fields
        if let syn::Fields::Named(fields) = &i.fields {
            for field in &fields.named {
                if let Some(ident) = &field.ident {
                    let field_doc = self.extract_doc(&field.attrs);

                    // Create proper module path for field
                    let mut path = self.current_module_path();
                    for part in &self.current_path {
                        path.push(part.clone());
                    }
                    path.push(i.ident.to_string());
                    path.push(ident.to_string());
                    let full_field_path = path.join("::");

                    // Format field type if available
                    let mut hover_text = field_doc;
                    if let syn::Type::Path(type_path) = &field.ty {
                        if let Some(segment) = type_path.path.segments.last() {
                            hover_text = format!("{} (type: {})", hover_text, segment.ident);
                        }
                    }

                    self.symbols.insert(
                        full_field_path,
                        SymbolInfo {
                            identifier: ident.to_string(),
                            symbol_type: SymbolType::Field,
                            hover_text,
                            source_vertex_id: format!("{:?}", self.current_file),
                        },
                    );
                }
            }
        }

        visit::visit_item_struct(self, i);
    }

    fn visit_item_enum(&mut self, i: &'ast ItemEnum) {
        let doc = self.extract_doc(&i.attrs);
        self.add_symbol(&i.ident.to_string(), SymbolType::Enum, doc);

        // Add variants
        for variant in &i.variants {
            let variant_doc = self.extract_doc(&variant.attrs);

            // Create proper module path for variant
            let mut path = self.current_module_path();
            for part in &self.current_path {
                path.push(part.clone());
            }
            path.push(i.ident.to_string());
            path.push(variant.ident.to_string());
            let full_variant_path = path.join("::");

            self.symbols.insert(
                full_variant_path,
                SymbolInfo {
                    identifier: variant.ident.to_string(),
                    symbol_type: SymbolType::Variant,
                    hover_text: variant_doc,
                    source_vertex_id: format!("{:?}", self.current_file),
                },
            );
        }

        visit::visit_item_enum(self, i);
    }

    fn visit_item_fn(&mut self, i: &'ast ItemFn) {
        let doc = self.extract_doc(&i.attrs);

        // Determine if this is a method or a function
        let symbol_type = if !self.current_path.is_empty()
            && self.current_path.last().unwrap().starts_with("impl ")
        {
            SymbolType::Method
        } else {
            SymbolType::Function
        };

        // Capture function signature with arguments
        let mut signature = String::new();

        // Check if function is async
        let is_async = i.sig.asyncness.is_some();
        if is_async {
            signature.push_str("async ");
        }

        signature.push_str("fn ");
        signature.push_str(&i.sig.ident.to_string());
        signature.push('(');

        // Add function arguments
        for (idx, input) in i.sig.inputs.iter().enumerate() {
            if idx > 0 {
                signature.push_str(", ");
            }

            match input {
                syn::FnArg::Receiver(r) => {
                    if r.reference.is_some() {
                        signature.push('&');
                        if let Some(lt) = &r.lifetime() {
                            signature.push('\'');
                            signature.push_str(&lt.ident.to_string());
                            signature.push(' ');
                        }
                        if r.mutability.is_some() {
                            signature.push_str("mut ");
                        }
                    } else if r.mutability.is_some() {
                        signature.push_str("mut ");
                    }
                    signature.push_str("self");
                }
                syn::FnArg::Typed(pat_type) => {
                    if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                        signature.push_str(&pat_ident.ident.to_string());
                        signature.push_str(": ");

                        // Add type information
                        let type_str = match &*pat_type.ty {
                            syn::Type::Path(type_path) => {
                                if let Some(segment) = type_path.path.segments.last() {
                                    segment.ident.to_string()
                                } else {
                                    "unknown".to_string()
                                }
                            }
                            syn::Type::Reference(type_ref) => {
                                let mut ref_str = String::from("&");
                                if type_ref.mutability.is_some() {
                                    ref_str.push_str("mut ");
                                }
                                if let syn::Type::Path(type_path) = &*type_ref.elem {
                                    if let Some(segment) = type_path.path.segments.last() {
                                        ref_str.push_str(&segment.ident.to_string());
                                    } else {
                                        ref_str.push_str("unknown");
                                    }
                                } else {
                                    ref_str.push_str("unknown");
                                }
                                ref_str
                            }
                            _ => "unknown".to_string(),
                        };

                        signature.push_str(&type_str);
                    }
                }
            }
        }

        signature.push(')');

        // Add return type if not unit
        if let syn::ReturnType::Type(_, return_type) = &i.sig.output {
            signature.push_str(" -> ");
            let return_str = match &**return_type {
                syn::Type::Path(type_path) => {
                    if let Some(segment) = type_path.path.segments.last() {
                        segment.ident.to_string()
                    } else {
                        "unknown".to_string()
                    }
                }
                _ => "unknown".to_string(),
            };
            signature.push_str(&return_str);
        }

        // Add attributes to hover text
        let mut hover_text = format!("{}\nSignature: {}", doc, signature);

        self.add_symbol(&i.sig.ident.to_string(), symbol_type, hover_text);
        visit::visit_item_fn(self, i);
    }

    fn visit_item_trait(&mut self, i: &'ast ItemTrait) {
        let doc = self.extract_doc(&i.attrs);
        self.add_symbol(&i.ident.to_string(), SymbolType::Trait, doc);

        self.enter_module(&i.ident.to_string());
        visit::visit_item_trait(self, i);
        self.exit_module();
    }

    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        let impl_name = if let Some((_, path, _)) = &i.trait_ {
            format!("impl {} for", path.segments.last().unwrap().ident)
        } else {
            "impl".to_string()
        };

        let type_name = match &*i.self_ty {
            syn::Type::Path(type_path) => {
                if let Some(segment) = type_path.path.segments.last() {
                    segment.ident.to_string()
                } else {
                    "Unknown".to_string()
                }
            }
            _ => "Unknown".to_string(),
        };

        let impl_full_name = format!("{} {}", impl_name, type_name);

        self.enter_module(&impl_full_name);
        visit::visit_item_impl(self, i);
        self.exit_module();
    }

    fn visit_item_const(&mut self, i: &'ast ItemConst) {
        let doc = self.extract_doc(&i.attrs);
        self.add_symbol(&i.ident.to_string(), SymbolType::Const, doc);
        visit::visit_item_const(self, i);
    }

    fn visit_item_static(&mut self, i: &'ast ItemStatic) {
        let doc = self.extract_doc(&i.attrs);
        self.add_symbol(&i.ident.to_string(), SymbolType::Static, doc);
        visit::visit_item_static(self, i);
    }

    fn visit_item_mod(&mut self, i: &'ast ItemMod) {
        let doc = self.extract_doc(&i.attrs);
        self.add_symbol(&i.ident.to_string(), SymbolType::Module, doc);

        if let Some(content) = &i.content {
            self.enter_module(&i.ident.to_string());

            for item in &content.1 {
                visit::visit_item(self, item);
            }

            self.exit_module();
        }
    }

    fn visit_item_type(&mut self, i: &'ast ItemType) {
        let doc = self.extract_doc(&i.attrs);
        self.add_symbol(&i.ident.to_string(), SymbolType::TypeAlias, doc);
        visit::visit_item_type(self, i);
    }

    fn visit_item_macro(&mut self, i: &'ast ItemMacro) {
        let doc = self.extract_doc(&i.attrs);
        let name = i
            .ident
            .as_ref()
            .map_or("unnamed_macro".to_string(), |id| id.to_string());
        self.add_symbol(&name, SymbolType::Macro, doc);
        visit::visit_item_macro(self, i);
    }
}

/// Determine crate name from Cargo.toml
fn get_crate_name(project_root: &Path) -> String {
    let cargo_path = project_root.join("Cargo.toml");
    if let Ok(content) = fs::read_to_string(cargo_path) {
        if let Some(name_line) = content.lines().find(|line| line.trim().starts_with("name")) {
            if let Some(equals_pos) = name_line.find('=') {
                let name = name_line[equals_pos + 1..]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'');
                return name.to_string();
            }
        }
    }
    // Default name
    "crate".to_string()
}

/// Symbol types we can identify
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum SymbolType {
    Struct,
    Enum,
    Function,
    Trait,
    Impl,
    Const,
    Static,
    Module,
    Field,
    Variant,
    Method,
    TypeAlias,
    Macro,
    Crate,
    Unknown(String),
}

/// Information about a symbol
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct SymbolInfo {
    pub identifier: String,
    pub symbol_type: SymbolType,
    pub hover_text: String,
    pub source_vertex_id: String,
}

/// A hierarchical representation of symbols
#[derive(Debug, Clone)]
pub enum SymbolNode {
    EnumVariant {
        name: String,
        doc: String,
    },
    Field {
        name: String,
        doc: String,
    },
    Function {
        name: String,
        signature: String,
        doc: String,
    },
    Leaf {
        name: String,
        symbol_type: SymbolType,
        doc: String,
    },
}

/// Symbol hierarchy
#[derive(Debug, Default)]
pub struct SymbolHierarchy {
    symbols: BTreeMap<String, BTreeMap<String, SymbolNode>>,
}

impl SymbolHierarchy {
    pub fn new() -> Self {
        Self {
            symbols: BTreeMap::new(),
        }
    }

    pub fn add_symbol(&mut self, path: &str, symbol_type: &SymbolType, doc: &str) {
        let parts: Vec<&str> = path.rsplitn(2, "::").collect();

        // Extract name and parent path
        let (name, parent) = match parts.len() {
            1 => (parts[0], ""),
            _ => (parts[0], parts[1]),
        };

        // For enum variants, extract the parent enum path
        if matches!(symbol_type, SymbolType::Variant) {
            if let Some(enum_path) = extract_parent_enum(path, name) {
                let doc = extract_doc_comment(doc);
                self.symbols
                    .entry(format!("ENUMS"))
                    .or_default()
                    .entry(enum_path.to_string())
                    .or_insert_with(|| SymbolNode::Leaf {
                        name: enum_path.to_string(),
                        symbol_type: SymbolType::Enum,
                        doc: String::new(),
                    });

                // Add variant to enum
                return;
            }
        }

        // For other symbols
        let category = format!("{:?}S", symbol_type).to_uppercase();

        match symbol_type {
            SymbolType::Function => {
                let signature = extract_function_signature(doc);
                let doc = extract_doc_comment(doc);
                self.symbols
                    .entry(category)
                    .or_default()
                    .entry(path.to_string())
                    .or_insert_with(|| SymbolNode::Function {
                        name: name.to_string(),
                        signature,
                        doc,
                    });
            }
            SymbolType::Field => {
                if let Some(struct_path) = extract_parent_struct(path, name) {
                    let doc = extract_doc_comment(doc);
                    // Add field to struct
                }
            }
            _ => {
                let doc = extract_doc_comment(doc);
                self.symbols
                    .entry(category)
                    .or_default()
                    .entry(path.to_string())
                    .or_insert_with(|| SymbolNode::Leaf {
                        name: name.to_string(),
                        symbol_type: symbol_type.clone(),
                        doc,
                    });
            }
        }
    }

    pub fn from_symbol_map(symbols: &HashMap<String, SymbolInfo>) -> Self {
        let mut hierarchy = Self::new();

        // First pass: add all base symbols
        for (path, info) in symbols {
            hierarchy.add_symbol(path, &info.symbol_type, &info.hover_text);
        }

        // Second pass: organize variants and fields
        for (path, info) in symbols {
            match &info.symbol_type {
                SymbolType::Variant => {
                    if let Some(enum_path) = extract_parent_path(path) {
                        let doc = extract_doc_comment(&info.hover_text);
                        hierarchy.add_enum_variant(&enum_path, path, doc);
                    }
                }
                SymbolType::Field => {
                    if let Some(struct_path) = extract_parent_path(path) {
                        let doc = extract_doc_comment(&info.hover_text);
                        hierarchy.add_struct_field(&struct_path, path, doc);
                    }
                }
                _ => {}
            }
        }

        hierarchy
    }

    fn add_enum_variant(&mut self, enum_path: &str, variant_path: &str, doc: String) {
        // Extract variant name from path
        let name = variant_path.rsplit("::").next().unwrap_or(variant_path);

        // Find the enum in the hierarchy
        if let Some(enums) = self.symbols.get_mut("ENUMS") {
            if let Some(enum_node) = enums.get_mut(enum_path) {
                // Add the variant as a child of the enum
                if let SymbolNode::Leaf { ref mut doc, .. } = enum_node {
                    // If this is the first variant, initialize the enum as a parent
                    let mut variants = BTreeMap::new();
                    variants.insert(
                        name.to_string(),
                        SymbolNode::EnumVariant {
                            name: name.to_string(),
                            doc: doc.to_string(),
                        },
                    );

                    // Replace the enum leaf with a parent node
                    // (In a real implementation, you'd have a proper parent node type)
                }
            }
        }
    }

    fn add_struct_field(&mut self, struct_path: &str, field_path: &str, doc: String) {
        // Similar implementation to add_enum_variant
    }

    pub fn format(&self) -> String {
        let mut result = String::new();
        result.push_str("crate lsp: Found symbols\n");

        for (category, symbols) in &self.symbols {
            if symbols.is_empty() {
                continue;
            }

            result.push_str(&format!("{}:\n", category));

            for (path, node) in symbols {
                match node {
                    SymbolNode::Leaf { doc, .. } => {
                        result.push_str(&format!("  {}", path));
                        if !doc.is_empty() {
                            result.push_str(&format!(" /* {} */", doc));
                        }
                        result.push('\n');
                    }
                    SymbolNode::Function { signature, doc, .. } => {
                        result.push_str(&format!("  {}: {}", path, signature));
                        if !doc.is_empty() {
                            result.push_str(&format!(" /* {} */", doc));
                        }
                        result.push('\n');
                    }
                    // Handle other node types
                    _ => {}
                }
            }

            result.push('\n');
        }

        result
    }
}

fn extract_parent_path(path: &str) -> Option<String> {
    path.rsplitn(2, "::").nth(1).map(|s| s.to_string())
}

fn extract_parent_enum(path: &str, variant_name: &str) -> Option<String> {
    // Logic to extract parent enum path from a variant path
    path.rsplitn(2, "::").nth(1).map(|s| s.to_string())
}

fn extract_parent_struct(path: &str, field_name: &str) -> Option<String> {
    // Logic to extract parent struct path from a field path
    path.rsplitn(2, "::").nth(1).map(|s| s.to_string())
}

/// Parse a single Rust file and collect symbols
fn parse_file(path: &Path, collector: &mut SymbolCollector) -> Result<(), String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file {}: {}", path.display(), e))?;

    collector.current_file = path.to_path_buf();

    let syntax = syn::parse_file(&content)
        .map_err(|e| format!("Failed to parse file {}: {}", path.display(), e))?;

    collector.visit_file(&syntax);

    Ok(())
}

/// Walk through a directory and parse all Rust files
pub fn parse_directory(dir_path: &Path) -> Result<HashMap<String, SymbolInfo>, String> {
    let crate_name = get_crate_name(dir_path);
    let mut collector = SymbolCollector::new(&crate_name, dir_path);

    let walker = WalkBuilder::new(dir_path)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .build();

    for result in walker {
        match result {
            Ok(entry) => {
                let path = entry.path();

                if path.extension().map_or(false, |ext| ext == "rs") {
                    if let Err(e) = parse_file(path, &mut collector) {
                        eprintln!("Error processing {}: {}", path.display(), e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error walking directory: {}", e);
            }
        }
    }

    Ok(collector.get_symbols().clone())
}

/// Main function to gather symbols from a project
pub fn gather_project_symbols(project_root: &Path) -> Result<SymbolHierarchy, String> {
    let symbols = parse_directory(project_root)?;
    Ok(SymbolHierarchy::from_symbol_map(&symbols))
}

/// Format symbols in a human-readable way
pub fn format_symbols(symbols: &HashMap<String, SymbolInfo>) -> String {
    organize_symbols(symbols)
}

/// Get symbols as a string in the same format as the LSIF output
pub fn get_project_symbols_string(project_root: &Path) -> Result<String, String> {
    let symbols = parse_directory(project_root)?;
    Ok(format_symbols(&symbols))
}

/// Creates a better organized symbol hierarchy
pub fn organize_symbols(symbols: &HashMap<String, SymbolInfo>) -> String {
    // Normalize symbol types
    let mut normalized_symbols: HashMap<String, SymbolInfo> = HashMap::new();
    for (path, info) in symbols {
        let mut normalized_info = info.clone();
        normalized_info.symbol_type = normalize_symbol_type(info);
        normalized_symbols.insert(path.clone(), normalized_info);
    }

    // Map enum variants to parent enums
    let mut enum_variants: HashMap<String, Vec<String>> = HashMap::new();
    let mut parent_enums: HashSet<String> = HashSet::new();

    for (path, info) in &normalized_symbols {
        if path.matches("::").count() == 2 {
            let parts: Vec<&str> = path.split("::").collect();
            if parts.len() == 3 {
                let enum_path = format!("{}::{}", parts[0], parts[1]);
                if let Some(enum_info) = normalized_symbols.get(&enum_path) {
                    if enum_info.symbol_type == SymbolType::Enum {
                        enum_variants
                            .entry(enum_path.clone())
                            .or_insert_with(Vec::new)
                            .push(path.clone());
                        parent_enums.insert(path.clone());
                    }
                }
            }
        }
    }

    // Map fields to parent structs
    let mut struct_fields: HashMap<String, Vec<String>> = HashMap::new();
    let mut parent_structs: HashSet<String> = HashSet::new();

    for (path, info) in &normalized_symbols {
        if path.matches("::").count() == 2 && info.symbol_type == SymbolType::Field {
            let parts: Vec<&str> = path.split("::").collect();
            if parts.len() == 3 {
                let struct_path = format!("{}::{}", parts[0], parts[1]);
                if let Some(struct_info) = normalized_symbols.get(&struct_path) {
                    if struct_info.symbol_type == SymbolType::Struct {
                        struct_fields
                            .entry(struct_path.clone())
                            .or_insert_with(Vec::new)
                            .push(path.clone());
                        parent_structs.insert(path.clone());
                    }
                }
            }
        }
    }

    // Organize symbols by type
    let mut output = String::from("crate lsp: Found symbols\n");
    let mut by_type: BTreeMap<String, Vec<(String, &SymbolInfo)>> = BTreeMap::new();

    // Group symbols by type
    for (path, info) in &normalized_symbols {
        if parent_enums.contains(path) || parent_structs.contains(path) {
            continue; // Skip enum variants and struct fields
        }

        let category = format!("{:?}S", info.symbol_type).to_uppercase();
        by_type
            .entry(category)
            .or_default()
            .push((path.clone(), info));
    }

    // Generate output
    for (category, symbols) in by_type {
        if symbols.is_empty() {
            continue;
        }

        output.push_str(&format!("{}:\n", category));

        for (path, info) in symbols {
            match info.symbol_type {
                SymbolType::Enum => {
                    let doc = extract_doc_comment(&info.hover_text);
                    output.push_str(&format!("  {}", path));
                    if !doc.is_empty() {
                        output.push_str(&format!(" /* {} */", doc));
                    }
                    output.push_str("\n");

                    // Add enum variants
                    if let Some(variants) = enum_variants.get(&path) {
                        for variant_path in variants {
                            let variant_name = extract_name(variant_path);
                            if let Some(variant_info) = normalized_symbols.get(variant_path) {
                                let doc = extract_doc_comment(&variant_info.hover_text);
                                output.push_str(&format!("    {}", variant_name));
                                if !doc.is_empty() {
                                    output.push_str(&format!(" /* {} */", doc));
                                }
                                output.push_str("\n");
                            }
                        }
                    }
                }
                SymbolType::Struct => {
                    let doc = extract_doc_comment(&info.hover_text);
                    output.push_str(&format!("  {}", path));
                    if !doc.is_empty() {
                        output.push_str(&format!(" /* {} */", doc));
                    }
                    output.push_str("\n");

                    // Add struct fields
                    if let Some(fields) = struct_fields.get(&path) {
                        output.push_str("    ");
                        let field_names: Vec<_> = fields
                            .iter()
                            .filter_map(|field_path| {
                                normalized_symbols.get(field_path).map(|field_info| {
                                    let name = extract_name(field_path);
                                    let doc = extract_doc_comment(&field_info.hover_text);
                                    if !doc.is_empty() {
                                        format!("{} /* {} */", name, doc)
                                    } else {
                                        name
                                    }
                                })
                            })
                            .collect();
                        output.push_str(&field_names.join("\n    "));
                        output.push_str("\n");
                    }
                }
                SymbolType::Function => {
                    let signature = extract_function_signature(&info.hover_text);
                    let doc = extract_doc_comment(&info.hover_text);
                    output.push_str(&format!("  {}: {}", path, signature));
                    if !doc.is_empty() {
                        output.push_str(&format!(" /* {} */", doc));
                    }
                    output.push_str("\n");
                }
                _ => {
                    let doc = extract_doc_comment(&info.hover_text);
                    output.push_str(&format!("  {}", path));
                    if !doc.is_empty() {
                        output.push_str(&format!(" /* {} */", doc));
                    }
                    output.push_str("\n");
                }
            }
        }
        output.push_str("\n");
    }

    output
}

/// Normalizes the Unknown symbol types to their proper types
fn normalize_symbol_type(symbol: &SymbolInfo) -> SymbolType {
    match &symbol.symbol_type {
        SymbolType::Unknown(type_str) => match type_str.to_lowercase().as_str() {
            "struct" => SymbolType::Struct,
            "enum" => SymbolType::Enum,
            "function" => SymbolType::Function,
            "trait" => SymbolType::Trait,
            "impl" => SymbolType::Impl,
            "const" => SymbolType::Const,
            "static" => SymbolType::Static,
            "module" => SymbolType::Module,
            "field" => SymbolType::Field,
            "variant" => SymbolType::Variant,
            "method" => SymbolType::Method,
            "typealias" => SymbolType::TypeAlias,
            "macro" => SymbolType::Macro,
            "crate" => SymbolType::Crate,
            _ => SymbolType::Unknown(type_str.clone()),
        },
        other => other.clone(),
    }
}

/// Extracts documentation from hover text
fn extract_doc_comment(hover_text: &str) -> String {
    // Check for documentation after --- marker
    if let Some(doc_section) = hover_text.split("---").nth(1) {
        return doc_section.trim().to_string();
    }
    String::new()
}

fn extract_rust_code_block(markdown: &str) -> String {
    // Regular expression to find Rust code blocks
    let re = Regex::new(r"```(?:rust|rs)\s*\n([\s\S]*?)```").unwrap();

    // Find all Rust code blocks
    let mut captures = re.captures_iter(markdown);

    // Take the second code block if available (first is usually just namespace)
    if let Some(first) = captures.next() {
        if let Some(second) = captures.next() {
            if let Some(code) = second.get(1) {
                return code.as_str().trim().to_owned();
            }
        }

        // If there's no second block, use the first one
        if let Some(code) = first.get(1) {
            return code.as_str().trim().to_owned();
        }
    }

    String::new()
}

/// Extracts function signature from hover text
fn extract_function_signature(hover_text: &str) -> String {
    let re = Regex::new(r"```rust\s*\n(.*?)\n```").unwrap_or_else(|_| Regex::new(r"").unwrap());

    // Get the second code block if available (first is usually namespace)
    let blocks: Vec<_> = re.captures_iter(hover_text).collect();
    if blocks.len() > 1 {
        if let Some(code) = blocks[1].get(1) {
            let signature = code.as_str().trim();
            return signature.to_string();
        }
    }
    String::new()
}

/// Extracts name from path
fn extract_name(path: &str) -> String {
    path.split("::").last().unwrap_or(path).to_string()
}
