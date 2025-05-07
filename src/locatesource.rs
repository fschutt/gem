use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use quote::ToTokens;
use syn::File;

// Assuming your parser module is at `crate::parser`
// Ensure SymbolInfo and SymbolType from parser.rs are pub and Clone.
use crate::parser::{self, SymbolInfo, SymbolType};

// --- Error Type ---
#[derive(Debug)]
pub enum SourceRetrieverError {
    Io(std::io::Error, Option<PathBuf>),
    SynError(PathBuf, syn::Error),
    SymbolError(String), // Error from parser::parse_directory
    ItemNotInSymbolMap(String),
    ItemNotFoundInAst {
        qname: String,
        local_name: String,
        file_path: PathBuf,
    },
    FileNotFound(PathBuf),
    CargoMetadataError(String),
    CargoTomlError(String),
    DependencyNotFoundInMetadata(String),
    MethodComponentParsingError(String),
}

impl fmt::Display for SourceRetrieverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceRetrieverError::Io(e, path) => {
                if let Some(p) = path {
                    write!(f, "IO error for file {:?}: {}", p, e)
                } else {
                    write!(f, "IO error: {}", e)
                }
            }
            SourceRetrieverError::SynError(path, e) => {
                write!(f, "Failed to parse Rust code in {:?}: {}", path, e)
            }
            SourceRetrieverError::SymbolError(e) => write!(f, "Symbol parsing/lookup error: {}", e),
            SourceRetrieverError::ItemNotInSymbolMap(item) => {
                write!(f, "Item '{}' not found in symbol map.", item)
            }
            SourceRetrieverError::ItemNotFoundInAst {
                qname,
                local_name,
                file_path,
            } => write!(
                f,
                "Item '{}' (local name '{}') not found in AST file {:?}.",
                qname, local_name, file_path
            ),
            SourceRetrieverError::FileNotFound(path) => write!(f, "File not found: {:?}", path),
            SourceRetrieverError::CargoMetadataError(e) => {
                write!(f, "cargo metadata failed: {}", e)
            }
            SourceRetrieverError::CargoTomlError(e) => {
                write!(f, "Error processing Cargo.toml: {}", e)
            }
            SourceRetrieverError::DependencyNotFoundInMetadata(dep_name) => {
                write!(f, "Dependency '{}' not found in cargo metadata.", dep_name)
            }
            SourceRetrieverError::MethodComponentParsingError(qname) => {
                write!(f, "Could not parse components (type, method) from qualified name '{}' for method extraction.", qname)
            }
        }
    }
}

impl std::error::Error for SourceRetrieverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SourceRetrieverError::Io(e, _) => Some(e),
            SourceRetrieverError::SynError(_, e) => Some(e),
            _ => None,
        }
    }
}

// Helper for converting io::Error with path context
fn io_err(e: std::io::Error, path: &Path) -> SourceRetrieverError {
    SourceRetrieverError::Io(e, Some(path.to_path_buf()))
}

type Result<T, E = SourceRetrieverError> = std::result::Result<T, E>;

#[derive(serde::Deserialize, Debug)]
struct CargoPackage {
    name: String,
    manifest_path: PathBuf,
}

#[derive(serde::Deserialize, Debug)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_root: PathBuf,
}

fn get_cargo_metadata(project_root: &Path) -> Result<CargoMetadata> {
    // Ensure dependencies are available by running `cargo check` quietly.
    // This triggers downloads if necessary.
    let check_status = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(project_root)
        .status()
        .map_err(|e| SourceRetrieverError::Io(e, Some(PathBuf::from("cargo check command"))))?;

    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--no-deps") // We only need info about direct dependencies listed in Cargo.toml for their source paths
        .arg("--format-version=1")
        .current_dir(project_root)
        .output()
        .map_err(|e| SourceRetrieverError::Io(e, Some(PathBuf::from("cargo metadata command"))))?;

    if !output.status.success() {
        return Err(SourceRetrieverError::CargoMetadataError(format!(
            "status: {}, stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| SourceRetrieverError::CargoMetadataError(format!("JSON parse error: {}", e)))
}

fn get_current_crate_name(project_root: &Path) -> Result<String> {
    let cargo_toml_path = project_root.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        return Err(SourceRetrieverError::CargoTomlError(format!(
            "Cargo.toml not found at {:?}",
            cargo_toml_path
        )));
    }
    let content = fs::read(&cargo_toml_path).map_err(|e| io_err(e, &cargo_toml_path))?;
    let manifest = cargo_toml::Manifest::from_slice(&content).map_err(|e| {
        SourceRetrieverError::CargoTomlError(format!(
            "TOML parse error for {:?}: {}",
            cargo_toml_path, e
        ))
    })?;

    Ok(manifest
        .package
        .ok_or_else(|| {
            SourceRetrieverError::CargoTomlError(format!(
                "package.name not found in {:?}",
                cargo_toml_path
            ))
        })?
        .name)
}

/// Retrieves the source code for a given item (qualified name) or an entire file (path).
pub fn retrieve_item_source(project_root: &Path, item_qname_or_path: &str) -> Result<String> {
    if is_file_path(item_qname_or_path) {
        return retrieve_direct_file_content(project_root, item_qname_or_path);
    }
    retrieve_qualified_item_source(project_root, item_qname_or_path)
}

fn is_file_path(path_str: &str) -> bool {
    path_str.ends_with(".rs") || path_str.contains('/') || path_str.contains('\\')
}

fn retrieve_direct_file_content(project_root: &Path, file_path_str: &str) -> Result<String> {
    let file_path = project_root.join(file_path_str);
    if !file_path.exists() {
        return Err(SourceRetrieverError::FileNotFound(file_path));
    }
    fs::read_to_string(&file_path).map_err(|e| io_err(e, &file_path))
}

fn determine_effective_root_and_qname<'a>(
    project_root: &'a Path,
    item_qname: &'a str,
    current_crate_name: &str,
) -> Result<(PathBuf, &'a str)> {
    let first_part = item_qname.split("::").next().unwrap_or("");

    if first_part == current_crate_name || first_part == "crate" {
        Ok((project_root.to_path_buf(), item_qname))
    } else {
        // Dependency item
        let metadata = get_cargo_metadata(project_root)?;
        let dep_package = metadata
            .packages
            .iter()
            .find(|p| p.name == first_part)
            .ok_or_else(|| {
                SourceRetrieverError::DependencyNotFoundInMetadata(first_part.to_string())
            })?;

        let dep_manifest_path = &dep_package.manifest_path;
        let dep_root = dep_manifest_path.parent().ok_or_else(|| {
            SourceRetrieverError::CargoTomlError(format!(
                "Could not get parent directory of dependency manifest path: {:?}",
                dep_manifest_path
            ))
        })?;
        Ok((dep_root.to_path_buf(), item_qname))
    }
}

fn retrieve_qualified_item_source(project_root: &Path, item_qname: &str) -> Result<String> {
    let current_crate_name = get_current_crate_name(project_root)?;
    let (effective_project_root, qname_for_parser) =
        determine_effective_root_and_qname(project_root, item_qname, &current_crate_name)?;

    let symbols = parser::parse_directory(&effective_project_root)
        .map_err(SourceRetrieverError::SymbolError)?;

    let symbol_info = symbols
        .get(qname_for_parser)
        .ok_or_else(|| SourceRetrieverError::ItemNotInSymbolMap(qname_for_parser.to_string()))?;

    // Ensure source_vertex_id from parser is correctly resolved
    let mut source_file_path = PathBuf::from(symbol_info.source_vertex_id.trim_matches('"'));
    if !source_file_path.is_absolute() {
        source_file_path = effective_project_root.join(&source_file_path);
    }

    if !source_file_path.exists() {
        return Err(SourceRetrieverError::FileNotFound(source_file_path.clone()));
    }

    extract_source_from_symbol_info(&source_file_path, symbol_info, qname_for_parser)
}

fn extract_source_from_symbol_info(
    source_file_path: &Path,
    symbol_info: &SymbolInfo,
    full_qname: &str, // The full qualified name, e.g. my_crate::MyStruct::foo
) -> Result<String> {
    if symbol_info.symbol_type == SymbolType::Module {
        return fs::read_to_string(source_file_path).map_err(|e| io_err(e, source_file_path));
    }

    let file_content =
        fs::read_to_string(source_file_path).map_err(|e| io_err(e, source_file_path))?;

    let ast = syn::parse_file(&file_content)
        .map_err(|e| SourceRetrieverError::SynError(source_file_path.to_path_buf(), e))?;

    find_item_in_ast(&ast, full_qname, symbol_info, source_file_path)
}

fn get_item_ident_name(item: &syn::Item) -> Option<String> {
    match item {
        syn::Item::Const(i) => Some(i.ident.to_string()),
        syn::Item::Enum(i) => Some(i.ident.to_string()),
        syn::Item::ExternCrate(i) => Some(i.ident.to_string()),
        syn::Item::Fn(i) => Some(i.sig.ident.to_string()),
        syn::Item::Macro(i) => i.ident.as_ref().map(|id| id.to_string()),
        syn::Item::Mod(i) => Some(i.ident.to_string()),
        syn::Item::Static(i) => Some(i.ident.to_string()),
        syn::Item::Struct(i) => Some(i.ident.to_string()),
        syn::Item::Trait(i) => Some(i.ident.to_string()),
        syn::Item::TraitAlias(i) => Some(i.ident.to_string()),
        syn::Item::Type(i) => Some(i.ident.to_string()),
        syn::Item::Union(i) => Some(i.ident.to_string()),
        _ => None, // Impl, Use, etc.
    }
}

fn find_item_in_ast(
    ast: &File,
    full_qname: &str,
    symbol_info: &SymbolInfo,
    source_file_path: &Path,
) -> Result<String> {
    if symbol_info.symbol_type == SymbolType::Method {
        return find_method_in_ast(ast, full_qname, &symbol_info.identifier, source_file_path);
    }

    // For other types like Struct, Enum, Function (not method), Trait, etc.
    for item in &ast.items {
        if get_item_ident_name(item).as_deref() == Some(&symbol_info.identifier) {
            return Ok(item.to_token_stream().to_string());
        }
    }

    Err(SourceRetrieverError::ItemNotFoundInAst {
        qname: full_qname.to_string(),
        local_name: symbol_info.identifier.clone(),
        file_path: source_file_path.to_path_buf(),
    })
}

fn find_method_in_ast(
    ast: &File,
    full_qname: &str,        // e.g., "my_crate::MyType::my_method"
    method_local_name: &str, // e.g., "my_method"
    source_file_path: &Path,
) -> Result<String> {
    let mut qname_parts: Vec<&str> = full_qname.split("::").collect();
    if qname_parts.len() < 2 {
        // crate::Type::method
        return Err(SourceRetrieverError::MethodComponentParsingError(
            full_qname.to_string(),
        ));
    }
    qname_parts.pop(); // Remove method name
    let type_name_from_qname = qname_parts
        .pop()
        .ok_or_else(|| SourceRetrieverError::MethodComponentParsingError(full_qname.to_string()))?;

    for item in &ast.items {
        if let syn::Item::Impl(item_impl) = item {
            let self_ty_matches_target = match &*item_impl.self_ty {
                syn::Type::Path(type_path) => type_path
                    .path
                    .segments
                    .last()
                    .map_or(false, |seg| seg.ident == type_name_from_qname),
                _ => false,
            };

            if self_ty_matches_target {
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item {
                        if impl_fn.sig.ident == method_local_name {
                            return Ok(impl_fn.to_token_stream().to_string());
                        }
                    }
                }
            }
        }
    }
    Err(SourceRetrieverError::ItemNotFoundInAst {
        qname: full_qname.to_string(),
        local_name: method_local_name.to_string(),
        file_path: source_file_path.to_path_buf(),
    })
}

// --- Tests ---
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // Helper trait for tests to compare strings ignoring whitespace differences
    trait StringExt {
        fn replace_whitespace(&self) -> String;
    }
    impl StringExt for str {
        fn replace_whitespace(&self) -> String {
            self.chars().filter(|c| !c.is_whitespace()).collect()
        }
    }
    impl StringExt for String {
        fn replace_whitespace(&self) -> String {
            self.chars().filter(|c| !c.is_whitespace()).collect()
        }
    }

    // Helper to create a dummy project structure
    fn create_test_project(
        dir: &Path,
        crate_name: &str,
        lib_rs_content: &str,
        dependencies: Option<&str>, // e.g. "my_other_lib = { path = \"../my_other_lib\" }"
        modules: Option<Vec<(&str, &str)>>, // Vec of (filename, content) relative to src/
    ) -> PathBuf {
        let project_path = dir.join(crate_name);
        fs::create_dir_all(project_path.join("src")).unwrap();

        let deps_str = dependencies.unwrap_or("");
        let cargo_toml_content = format!(
            r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
{}
"#,
            crate_name, deps_str
        );
        fs::write(project_path.join("Cargo.toml"), cargo_toml_content).unwrap();
        fs::write(project_path.join("src/lib.rs"), lib_rs_content).unwrap();

        if let Some(mods) = modules {
            for (name, content) in mods {
                let mod_path = project_path.join("src").join(name);
                if let Some(parent) = mod_path.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::write(mod_path, content).unwrap();
            }
        }
        project_path
    }

    #[test]
    fn test_retrieve_whole_file_directly() {
        let dir = tempdir().unwrap();
        let lib_content = "pub fn hello() {} // direct file";
        let project_root =
            create_test_project(dir.path(), "test_direct_file", lib_content, None, None);

        let result = retrieve_item_source(&project_root, "src/lib.rs").unwrap();
        assert_eq!(result.trim(), lib_content.trim());
    }

    #[test]
    fn test_retrieve_struct_local_crate() {
        let dir = tempdir().unwrap();
        let lib_content = r#"
pub struct MyLocalStruct { pub val: u8 }
fn another() {}
"#;
        let project_root =
            create_test_project(dir.path(), "local_struct_crate", lib_content, None, None);
        // Your parser.rs needs to be able to find "local_struct_crate::MyLocalStruct"
        // SymbolInfo.identifier should be "MyLocalStruct"
        // SymbolInfo.source_vertex_id should point to src/lib.rs (ideally absolute or resolvable)
        let result =
            retrieve_item_source(&project_root, "local_struct_crate::MyLocalStruct").unwrap();
        let expected_code = "pub struct MyLocalStruct { pub val: u8 }";
        assert_eq!(
            result.replace_whitespace(),
            expected_code.replace_whitespace()
        );
    }

    #[test]
    fn test_retrieve_method_local_crate() {
        let dir = tempdir().unwrap();
        let lib_content = r#"
pub struct Calc { num: i32 }
impl Calc {
    pub fn add(&mut self, val: i32) { self.num += val; }
}
"#;
        let project_root =
            create_test_project(dir.path(), "local_method_crate", lib_content, None, None);
        let result = retrieve_item_source(&project_root, "local_method_crate::Calc::add").unwrap();
        let expected_code = "pub fn add(& mut self , val : i32) { self . num += val ; }";
        assert_eq!(
            result.replace_whitespace(),
            expected_code.replace_whitespace()
        );
    }

    #[test]
    fn test_retrieve_item_from_submodule_local_crate() {
        let dir = tempdir().unwrap();
        let lib_rs = "pub mod inner;";
        let inner_rs = "pub struct DeepStruct { id: String }";
        let project_root = create_test_project(
            dir.path(),
            "submod_crate",
            lib_rs,
            None,
            Some(vec![("inner.rs", inner_rs)]),
        );
        let result =
            retrieve_item_source(&project_root, "submod_crate::inner::DeepStruct").unwrap();
        let expected_code = "pub struct DeepStruct { id : String }";
        assert_eq!(
            result.replace_whitespace(),
            expected_code.replace_whitespace()
        );
    }

    #[test]
    fn test_retrieve_whole_module_content_by_qname() {
        let dir = tempdir().unwrap();
        let lib_rs = "pub mod data_mod;";
        let data_mod_rs = "pub const VERSION: &str = \"1.2.3\";";
        let project_root = create_test_project(
            dir.path(),
            "mod_qname_crate",
            lib_rs,
            None,
            Some(vec![("data_mod.rs", data_mod_rs)]),
        );
        // Parser should identify "mod_qname_crate::data_mod" as SymbolType::Module
        let result = retrieve_item_source(&project_root, "mod_qname_crate::data_mod").unwrap();
        assert_eq!(result.trim(), data_mod_rs.trim());
    }

    // Dependency test setup
    fn setup_dependent_project(base_dir: &Path) -> (PathBuf, PathBuf) {
        // Dependency Crate
        let dep_crate_name = "sample_dep";
        let dep_lib_content = r#"
pub struct DepInfo { pub version: &'static str }
impl DepInfo {
    pub fn format_version(&self) -> String { format!("v{}", self.version) }
}
pub fn get_dep_name() -> &'static str { "sample_dep" }
"#;
        let dep_project_root =
            create_test_project(base_dir, dep_crate_name, dep_lib_content, None, None);

        // Main Crate (depends on dep_crate)
        let main_crate_name = "user_app";
        let main_lib_content = format!(
            r#"
// use sample_dep::DepInfo; // Not strictly needed for test if not compiling main_app
pub fn main_func() {{ }}
"#
        );
        let main_project_root = create_test_project(
            base_dir,
            main_crate_name,
            &main_lib_content,
            Some(&format!(
                "{} = {{ path = \"../{}\" }}",
                dep_crate_name, dep_crate_name
            )), // Dependency line
            None,
        );

        (main_project_root, dep_project_root)
    }

    #[test]
    fn test_retrieve_struct_from_dependency() {
        let dir = tempdir().unwrap();
        let (main_project_root, _dep_project_root) = setup_dependent_project(dir.path());

        let result = retrieve_item_source(&main_project_root, "sample_dep::DepInfo").unwrap();
        let expected_code = "pub struct DepInfo { pub version : & 'static str }";
        assert_eq!(
            result.replace_whitespace(),
            expected_code.replace_whitespace()
        );
    }

    #[test]
    fn test_retrieve_fn_from_dependency() {
        let dir = tempdir().unwrap();
        let (main_project_root, _dep_project_root) = setup_dependent_project(dir.path());

        let result = retrieve_item_source(&main_project_root, "sample_dep::get_dep_name").unwrap();
        let expected_code = "pub fn get_dep_name () -> & 'static str { \"sample_dep\" }";
        assert_eq!(
            result.replace_whitespace(),
            expected_code.replace_whitespace()
        );
    }

    #[test]
    fn test_retrieve_method_from_dependency() {
        let dir = tempdir().unwrap();
        let (main_project_root, _dep_project_root) = setup_dependent_project(dir.path());

        let result =
            retrieve_item_source(&main_project_root, "sample_dep::DepInfo::format_version")
                .unwrap();
        let expected_code =
            "pub fn format_version (& self) -> String { format ! (\"v{}\" , self . version) }";
        assert_eq!(
            result.replace_whitespace(),
            expected_code.replace_whitespace()
        );
    }
}
