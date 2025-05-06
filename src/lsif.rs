use std::collections::HashMap;
use lsp_types::lsif::{Entry, Element, Vertex, Edge, RangeTag};
use lsp_types::{MarkedString, NumberOrString, SymbolKind};

/// Main structure to hold parsed LSIF data
#[derive(Debug, Default)]
pub struct LsifIndex {
    vertices: HashMap<String, Vertex>,
    outgoing_edges: HashMap<String, Vec<(String, Edge)>>, // source_id -> [(edge_id, edge)]
    incoming_edges: HashMap<String, Vec<(String, Edge)>>, // target_id -> [(edge_id, edge)]
    definitions: HashMap<String, (String, String)>, // Maps path (full symbol name) to (vertex_id of range, documentation)
    symbol_types: HashMap<String, SymbolType>,     // Maps path (full symbol name) to their types
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolType {
    Enum,
    Function,
    Constant,
    Struct,
    Unknown,
}

impl SymbolType {
    fn from_kind(kind: SymbolKind) -> Self {
        match kind {
            SymbolKind::ENUM => SymbolType::Enum, // Typically kind 10 in LSIF JSON
            SymbolKind::METHOD | SymbolKind::FUNCTION => SymbolType::Function, // METHOD (6), FUNCTION (12)
            SymbolKind::CONSTANT => SymbolType::Constant, // Typically kind 14
            SymbolKind::STRUCT => SymbolType::Struct, // Typically kind 23
            _ => SymbolType::Unknown,
        }
    }
}

/// Parse LSIF content into our index structure
pub fn parse_lsif(content: &str) -> Result<LsifIndex, Box<dyn std::error::Error>> {
    let mut index = LsifIndex::default();
    
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: Entry = serde_json::from_str(line)?;
        let id_str = nos_to_string(&entry.id);
        
        match entry.data {
            Element::Vertex(vertex) => {
                index.vertices.insert(id_str, vertex);
            },
            Element::Edge(edge) => {
                // Extract source and target IDs based on edge type
                let (source_id_opt, target_id_opt): (Option<String>, Option<String>) = match &edge {
                    Edge::Moniker(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::NextMoniker(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::Next(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::PackageInformation(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::Definition(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::Declaration(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::Hover(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::References(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::Implementation(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::TypeDefinition(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::FoldingRange(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::DocumentLink(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::DocumentSymbol(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::Diagnostic(data) => (Some(nos_to_string(&data.out_v)), Some(nos_to_string(&data.in_v))),
                    Edge::Contains(data) => {
                        let out_v = nos_to_string(&data.out_v);
                         index.outgoing_edges.entry(out_v)
                            .or_default()
                            .push((id_str.clone(), clone_edge(&edge)));
                        for in_v_val in &data.in_vs {
                            index.incoming_edges.entry(nos_to_string(&in_v_val))
                                .or_default()
                                .push((id_str.clone(), clone_edge(&edge)));
                        }
                        (None, None) // Handled specially
                    },
                    Edge::Item(item) => {
                        let doc_id = nos_to_string(&item.document);
                        index.outgoing_edges.entry(doc_id)
                            .or_default()
                            .push((id_str.clone(), clone_edge(&edge)));
                        for in_v_val in &item.edge_data.in_vs {
                             index.incoming_edges.entry(nos_to_string(&in_v_val))
                                .or_default()
                                .push((id_str.clone(), clone_edge(&edge)));
                        }
                        (None, None) // Handled specially
                    },
                };
                
                if let Some(source_id) = source_id_opt {
                    index.outgoing_edges.entry(source_id)
                        .or_default()
                        .push((id_str.clone(), clone_edge(&edge)));
                }
                if let Some(target_id) = target_id_opt {
                     index.incoming_edges.entry(target_id)
                        .or_default()
                        .push((id_str, edge));
                }
            },
        }
    }
    
    // Second pass: identify definitions and their documentation
    for (vertex_id_of_range, vertex) in &index.vertices {
        if let Vertex::Range { range: _, tag } = vertex {
            if let Some(tag_content) = tag {
                if let RangeTag::Definition(def_tag) = tag_content {
                    let def_tag = clone_def(def_tag);
                    let text = &def_tag.text;
                    let path = extract_full_path(vertex_id_of_range, &index, text);
                    let hover_text = find_hover_text(vertex_id_of_range, &index);
                    let symbol_type = SymbolType::from_kind(def_tag.kind);
                    
                    index.definitions.insert(path.clone(), (vertex_id_of_range.clone(), hover_text));
                    index.symbol_types.insert(path, symbol_type);
                }
            }
        }
    }
    
    Ok(index)
}


#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionTag {
    /// The text covered by the range     
    text: String,
    /// The symbol kind.
    kind: lsp_types::SymbolKind,
    /// Indicates if this symbol is deprecated.
    #[serde(default)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    deprecated: bool,
    /// The full range of the definition not including leading/trailing whitespace but everything else, e.g comments and code.
    /// The range must be included in fullRange.
    full_range: lsp_types::Range,
    /// Optional detail information for the definition.
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

fn clone_def(e: &lsp_types::lsif::DefinitionTag) -> DefinitionTag {
    serde_json::from_str(&serde_json::to_string(&e).unwrap_or_default()).unwrap()
}

fn clone_edge(e: &Edge) -> Edge {
    serde_json::from_str(&serde_json::to_string(&e).unwrap_or_default()).unwrap()
}

fn nos_to_string(s: &NumberOrString) -> String {
    match s {
        NumberOrString::Number(n) => n.to_string(),
        NumberOrString::String(s) => s.clone(),
    }
}

/// Extract a full path for a symbol from its range vertex
fn extract_full_path(range_id: &str, index: &LsifIndex, name: &str) -> String {
    if let Some(edges) = index.outgoing_edges.get(range_id) {
        for (_edge_id, edge) in edges {
            if let Edge::Next(data) = edge {
                let result_set_id = nos_to_string(&data.in_v);
                if let Some(rs_edges) = index.outgoing_edges.get(&result_set_id) {
                    for (_rs_edge_id, rs_edge) in rs_edges {
                        if let Edge::Moniker(m_data) = rs_edge {
                            let moniker_id = nos_to_string(&m_data.in_v);
                            if let Some(Vertex::Moniker(moniker)) = index.vertices.get(&moniker_id) {
                                return moniker.identifier.clone();
                            }
                        }
                    }
                }
            }
        }
    }
    name.to_string() // Fallback
}

/// Find hover text (documentation) for a symbol
fn find_hover_text(range_id: &str, index: &LsifIndex) -> String {
    if let Some(outgoing) = index.outgoing_edges.get(range_id) {
        for (_edge_id, edge) in outgoing {
            if let Edge::Next(data) = edge {
                let result_set_id = nos_to_string(&data.in_v);
                if let Some(rs_edges) = index.outgoing_edges.get(&result_set_id) {
                    for (_rs_edge_id, rs_edge_data) in rs_edges {
                        if let Edge::Hover(hover_data) = rs_edge_data {
                            let hover_id = nos_to_string(&hover_data.in_v);
                            if let Some(Vertex::HoverResult { result }) = &index.vertices.get(&hover_id) {
                                match &result.contents {
                                    lsp_types::HoverContents::Scalar(marked_string) => {
                                        match marked_string {
                                            MarkedString::String(s) => return s.clone(),
                                            MarkedString::LanguageString(ls) => return ls.value.clone(),
                                        }
                                    },
                                    lsp_types::HoverContents::Array(arr) => {
                                        if let Some(first_marked_string) = arr.get(0) {
                                            match first_marked_string {
                                                MarkedString::String(s) => return s.clone(),
                                                MarkedString::LanguageString(ls) => return ls.value.clone(),
                                            }
                                        }
                                    },
                                    lsp_types::HoverContents::Markup(markup) => {
                                        return markup.value.clone();
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    String::new()
}

/// Query for a specific symbol by path
pub fn query(index: &LsifIndex, path: &str) -> Option<String> {
    if let Some((_vertex_id, doc)) = index.definitions.get(path) {
        let parts: Vec<&str> = path.split("::").collect();
        let module_path = if parts.len() > 1 {
            parts[..parts.len()-1].join("::")
        } else {
            // If there's no "::", it might be a top-level item in a crate,
            // or the path itself is the crate/module.
            // For "test_crate::MyEnum", module_path becomes "test_crate".
            // For "MyGlobalConst", module_path becomes "".
            // Depending on desired output, this might need adjustment.
            // The test implies for "test_crate::MyEnum", the first block is "test_crate".
            // If path was "MyCrate", this logic makes module_path "".
             if parts.len() == 1 && !path.contains("::") { // e.g. crate root or single segment path
                String::new() // No separate module prefix to display
            } else {
                 parts.get(0).map_or(String::new(), |s| s.to_string()) // Simplistic: take first segment as crate/module
            }
        };
        
        let formatted_doc = format!("```rust\n{}\n```\n\n```rust\n{}\n```", module_path, doc);
        return Some(formatted_doc);
    }
    None
}

/// List all symbols organized by type
pub fn list(index: &LsifIndex) -> String {
    let mut enums = Vec::new();
    let mut functions = Vec::new();
    let mut constants = Vec::new();
    let mut structs = Vec::new();
    let mut unknown = Vec::new();
    
    for (path, symbol_type) in &index.symbol_types {
        match symbol_type {
            SymbolType::Enum => enums.push(path.clone()),
            SymbolType::Function => functions.push(path.clone()),
            SymbolType::Constant => constants.push(path.clone()),
            SymbolType::Struct => structs.push(path.clone()),
            SymbolType::Unknown => unknown.push(path.clone()),
        }
    }
    
    enums.sort();
    functions.sort();
    constants.sort();
    structs.sort();
    unknown.sort();
    
    let mut result = String::new();
    
    if !enums.is_empty() {
        result.push_str("ENUMS:\n");
        for path in enums {
            result.push_str(&format!("  {}\n", path));
        }
    }
    
    if !functions.is_empty() {
        if !result.is_empty() { result.push('\n'); }
        result.push_str("FUNCTIONS:\n");
        for path in functions {
            result.push_str(&format!("  {}\n", path));
        }
    }
    
    if !constants.is_empty() {
        if !result.is_empty() { result.push('\n'); }
        result.push_str("CONSTANTS:\n");
        for path in constants {
            result.push_str(&format!("  {}\n", path));
        }
    }
    
    if !structs.is_empty() {
        if !result.is_empty() { result.push('\n'); }
        result.push_str("STRUCTS:\n");
        for path in structs {
            result.push_str(&format!("  {}\n", path));
        }
    }
    
    if !unknown.is_empty() {
        if !result.is_empty() { result.push('\n'); }
        result.push_str("UNKNOWN:\n");
        for path in unknown {
            result.push_str(&format!("  {}\n", path));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::lsif::{
        DefinitionTag, Edge, EdgeData, Element, MetaData, ResultSet, Vertex
    };
    use lsp_types::{Range, Position, HoverContents, MarkupContent, MarkupKind, SymbolKind};

    // Helper to create a string from an LSIF entry
    fn lsif_entry_to_string(entry: &Entry) -> String {
        serde_json::to_string(entry).unwrap()
    }
    
    // Helper to create a Vertex::Range for definition
    fn create_def_range_vertex(id: &str, text: &str, kind: SymbolKind, line: u32) -> Entry {

        // We need to create a DefinitionTag struct first, then serialize it to a string,
        // then deserialize it back into a RangeTag::Definition.
        // This is because DefinitionTag has private fields.
        // We can use a temporary struct with public fields that matches DefinitionTag for serialization.
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct PublicDefinitionTag<'a> {
            text: &'a str,
            kind: SymbolKind,
            deprecated: bool,
            full_range: Range,
            detail: Option<&'a str>,
        }

        let public_def_tag = PublicDefinitionTag {
            text,
            kind,
            deprecated: false,
            full_range: Range::new(Position::new(line,0), Position::new(line,12 + text.len() as u32)),
            detail: None,
        };
        let def_tag_json = serde_json::to_string(&public_def_tag).unwrap();
        let def_tag_data: DefinitionTag = serde_json::from_str(&def_tag_json).unwrap();


        Entry {
            id: NumberOrString::String(id.to_string()),
            // type_ and label are handled by serde(tag=...) in Element and Vertex
            data: Element::Vertex(Vertex::Range {
                range: Range::new(Position::new(line,5), Position::new(line,5 + text.len() as u32)),
                tag: Some(RangeTag::Definition(def_tag_data)),
            }),
        }
    }

    fn create_result_set_vertex(id: &str) -> Entry {
         Entry {
            id: NumberOrString::String(id.to_string()),
            data: Element::Vertex(Vertex::ResultSet(ResultSet { key: None })),
        }
    }

    fn create_moniker_vertex(id: &str, identifier: &str, kind_opt: Option<&str>) -> Entry {
        let kind = kind_opt.and_then(|k| serde_json::from_str(k).ok());
         Entry {
            id: NumberOrString::String(id.to_string()),
            data: Element::Vertex(Vertex::Moniker(lsp_types::Moniker {
                scheme: "test_scheme".to_string(),
                identifier: identifier.to_string(),
                unique: lsp_types::UniquenessLevel::Scheme,
                kind,
            })),
        }
    }
    
    fn create_hover_result_vertex(id: &str, hover_text: &str) -> Entry {
         Entry {
            id: NumberOrString::String(id.to_string()),
            data: Element::Vertex(Vertex::HoverResult {
                result: lsp_types::Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: hover_text.to_string(),
                    }),
                    range: None,
                },
            }),
        }
    }

    fn create_next_edge(id: &str, out_v: &str, in_v: &str) -> Entry {
        Entry {
            id: NumberOrString::String(id.to_string()),
            data: Element::Edge(Edge::Next(EdgeData {
                out_v:  NumberOrString::String(out_v.to_string()),
                in_v:  NumberOrString::String(in_v.to_string()),
            })),
        }
    }

    fn create_moniker_edge(id: &str, out_v: &str, in_v: &str) -> Entry {
         Entry {
            id: NumberOrString::String(id.to_string()),
            data: Element::Edge(Edge::Moniker(EdgeData {
                out_v:  NumberOrString::String(out_v.to_string()),
                in_v:  NumberOrString::String(in_v.to_string()),
            })),
        }
    }
    
    fn create_hover_edge(id: &str, out_v: &str, in_v: &str) -> Entry {
         Entry {
            id: NumberOrString::String(id.to_string()),
            data: Element::Edge(Edge::Hover(EdgeData {
                out_v:  NumberOrString::String(out_v.to_string()),
                in_v:  NumberOrString::String(in_v.to_string()),
            })),
        }
    }

    #[test]
    fn test_parse_minimal_lsif_and_query_list() {
        use std::str::FromStr;
        let entries = vec![
            lsif_entry_to_string(&Entry {
                id: NumberOrString::String("meta".to_string()), 
                data: Element::Vertex(Vertex::MetaData(MetaData {
                    version: "0.5.0".to_string(),
                    project_root: lsp_types::Uri::from_str("file:///test").unwrap(),
                    position_encoding: lsp_types::lsif::Encoding::Utf16, // Use the enum variant
                    tool_info: None,
                })),
            }),
            // MyEnum
            lsif_entry_to_string(&create_def_range_vertex("1", "MyEnum", SymbolKind::ENUM, 1)),
            lsif_entry_to_string(&create_result_set_vertex("2")),
            lsif_entry_to_string(&create_moniker_vertex("3", "test_crate::MyEnum", Some("export"))),
            lsif_entry_to_string(&create_hover_result_vertex("4", "Docs for MyEnum")),
            lsif_entry_to_string(&create_next_edge("e1", "1", "2")),
            lsif_entry_to_string(&create_moniker_edge("e2", "2", "3")),
            lsif_entry_to_string(&create_hover_edge("e3", "2", "4")),

            // my_func
            lsif_entry_to_string(&create_def_range_vertex("5", "my_func", SymbolKind::FUNCTION, 2)),
            lsif_entry_to_string(&create_result_set_vertex("6")),
            lsif_entry_to_string(&create_moniker_vertex("7", "test_crate::module::my_func", Some("export"))),
            lsif_entry_to_string(&create_hover_result_vertex("8", "Docs for my_func")),
            lsif_entry_to_string(&create_next_edge("e4", "5", "6")),
            lsif_entry_to_string(&create_moniker_edge("e5", "6", "7")),
            lsif_entry_to_string(&create_hover_edge("e6", "6", "8")),
            
            // MY_CONST
            lsif_entry_to_string(&create_def_range_vertex("9", "MY_CONST", SymbolKind::CONSTANT, 3)),
            lsif_entry_to_string(&create_result_set_vertex("10")),
            lsif_entry_to_string(&create_moniker_vertex("11", "test_crate::MY_CONST", Some("export"))),
            lsif_entry_to_string(&create_hover_result_vertex("12", "Docs for MY_CONST")),
            lsif_entry_to_string(&create_next_edge("e7", "9", "10")),
            lsif_entry_to_string(&create_moniker_edge("e8", "10", "11")),
            lsif_entry_to_string(&create_hover_edge("e9", "10", "12")),

            // MyStruct
            lsif_entry_to_string(&create_def_range_vertex("13", "MyStruct", SymbolKind::STRUCT, 4)),
            lsif_entry_to_string(&create_result_set_vertex("14")),
            lsif_entry_to_string(&create_moniker_vertex("15", "test_crate::MyStruct", Some("export"))),
            lsif_entry_to_string(&create_hover_result_vertex("16", "Docs for MyStruct")),
            lsif_entry_to_string(&create_next_edge("e10", "13", "14")),
            lsif_entry_to_string(&create_moniker_edge("e11", "14", "15")),
            lsif_entry_to_string(&create_hover_edge("e12", "14", "16")),

            // GlobalConst (no module)
            lsif_entry_to_string(&create_def_range_vertex("17", "GlobalConst", SymbolKind::CONSTANT, 5)),
            lsif_entry_to_string(&create_result_set_vertex("18")),
            lsif_entry_to_string(&create_moniker_vertex("19", "GlobalConst", Some("export"))), 
            lsif_entry_to_string(&create_hover_result_vertex("20", "Docs for GlobalConst")),
            lsif_entry_to_string(&create_next_edge("e13", "17", "18")),
            lsif_entry_to_string(&create_moniker_edge("e14", "18", "19")),
            lsif_entry_to_string(&create_hover_edge("e15", "18", "20")),
        ];
        let lsif_content = entries.join("\n");
        let index = parse_lsif(&lsif_content).unwrap();

        assert_eq!(index.definitions.len(), 5);
        assert_eq!(index.symbol_types.len(), 5);

        let enum_query = query(&index, "test_crate::MyEnum");
        assert!(enum_query.is_some());
        assert_eq!(enum_query.unwrap(), "```rust\ntest_crate\n```\n\n```rust\nDocs for MyEnum\n```");

        let func_query = query(&index, "test_crate::module::my_func");
        assert!(func_query.is_some());
        assert_eq!(func_query.unwrap(), "```rust\ntest_crate::module\n```\n\n```rust\nDocs for my_func\n```");
        
        let global_const_query = query(&index, "GlobalConst");
        assert!(global_const_query.is_some());
        assert_eq!(global_const_query.unwrap(), "```rust\n\n```\n\n```rust\nDocs for GlobalConst\n```");


        let list_output = list(&index);
        
        let expected_list_parts = [
            "ENUMS:\n  test_crate::MyEnum",
            "FUNCTIONS:\n  test_crate::module::my_func",
            // Order of constants can vary due to HashMap iteration if not sorted before adding
            "CONSTANTS:",
            "  GlobalConst",
            "  test_crate::MY_CONST",
            "STRUCTS:\n  test_crate::MyStruct",
        ];

        let mut output_lines: Vec<&str> = list_output.trim().lines().map(str::trim).filter(|s| !s.is_empty()).collect();
        output_lines.sort_unstable(); // Sort to handle HashMap iteration order

        let mut expected_lines: Vec<String> = expected_list_parts.join("\n").trim().lines().map(str::trim).filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
        expected_lines.sort_unstable();
        
        assert_eq!(output_lines, expected_lines, "List output mismatch.\nActual:\n{}\nExpected structure:\n{}", list_output, expected_list_parts.join("\n"));
    }

    /* 
    #[test]
    #[ignore] 
    fn test_parse_and_query_original() {
        // This test requires the actual `out.txt` file to be present in the project root 
        let lsif_content = include_str!("../out.txt"); 
        let index = parse_lsif(lsif_content).unwrap();
        
        let result = query(&index, "gem::cli::DebugMode");
        assert!(result.is_some(), "Query for gem::cli::DebugMode should succeed.");
        
        let list_result = list(&index);
        assert!(list_result.contains("gem::"), "List should contain gem symbols.");
        assert!(list_result.contains("ENUMS:"), "List should have ENUMS section.");
        assert!(list_result.contains("FUNCTIONS:"), "List should have FUNCTIONS section.");
    }
    */
}