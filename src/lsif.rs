/// Helper function to convert NumberOrString to a standard String
fn number_or_string_to_string(id: &NumberOrString) -> String {
    match id {
        NumberOrString::Number(n) => n.to_string(),
        NumberOrString::String(s) => s.clone(),
    }
}
use std::collections::HashMap;
use lsp_types::lsif::{Entry, Element, Vertex, Edge, RangeTag};
use lsp_types::{MarkedString, SymbolKind, NumberOrString};
use serde::{Deserialize, Serialize};

/// Main structure to hold parsed LSIF data
#[derive(Debug, Default)]
pub struct LsifIndex {
    vertices: HashMap<String, Vertex>,
    outgoing_edges: HashMap<String, Vec<(String, Edge)>>, // source_id -> [(edge_id, edge)]
    incoming_edges: HashMap<String, Vec<(String, Edge)>>, // target_id -> [(edge_id, edge)]
    definitions: HashMap<String, (String, String)>, // Maps paths to (vertex_id, documentation)
    symbol_types: HashMap<String, SymbolType>,     // Maps paths to their types
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
            SymbolKind::ENUM => SymbolType::Enum,
            SymbolKind::FUNCTION | SymbolKind::METHOD => SymbolType::Function, // METHOD or FUNCTION
            SymbolKind::CONSTANT => SymbolType::Constant, // CONSTANT
            SymbolKind::STRUCT => SymbolType::Struct,
            _ => SymbolType::Unknown,
        }
    }
}

/// Parse LSIF content into our index structure
pub fn parse_lsif(content: &str) -> Result<LsifIndex, Box<dyn std::error::Error>> {
    let mut index = LsifIndex::default();
    let mut id_map: HashMap<String, String> = HashMap::new(); // edge_id -> (source_id, target_id)
    
    // First pass: collect all vertices
    for line in content.lines() {
        let entry: Entry = serde_json::from_str(line)?;
        let id_str = number_or_string_to_string(&entry.id);
        
        match entry.data {
            Element::Vertex(vertex) => {
                index.vertices.insert(id_str, vertex);
            },
            Element::Edge(edge) => {

                let edge2 = match clone_edge(&edge) {
                    Some(s) => s,
                    None => continue,
                };

                // Extract source and target IDs based on edge type
                let (source_id, target_id) = match &edge {
                    Edge::Moniker(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::NextMoniker(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::Next(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::PackageInformation(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::Definition(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::Declaration(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::Hover(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::References(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::Implementation(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::TypeDefinition(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::FoldingRange(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::DocumentLink(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::DocumentSymbol(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::Diagnostic(data) => (number_or_string_to_string(&data.out_v), number_or_string_to_string(&data.in_v)),
                    Edge::Contains(data) => {
                        // Handle multi-in edges specially
                        let out_v = number_or_string_to_string(&data.out_v);
                        index.outgoing_edges.entry(out_v.clone())
                            .or_insert_with(Vec::new)
                            .push((id_str.clone(), edge2));
                            
                        // Store edge ID mapping
                        id_map.insert(id_str, out_v);
                        continue;
                    },
                    Edge::Item(item) => {
                        // Handle Item edge specially
                        let doc_id = number_or_string_to_string(&item.document);
                        index.outgoing_edges.entry(doc_id.clone())
                            .or_insert_with(Vec::new)
                            .push((id_str.clone(), edge2));
                            
                        // Store edge ID mapping
                        id_map.insert(id_str, doc_id);
                        continue;
                    },
                };
                
                // Store outgoing edges
                index.outgoing_edges.entry(source_id.clone())
                    .or_insert_with(Vec::new)
                    .push((id_str.clone(), edge2));
                
                // Store incoming edges
                index.incoming_edges.entry(target_id.clone())
                    .or_insert_with(Vec::new)
                    .push((id_str.clone(), edge));
                
                // Store edge ID mapping
                id_map.insert(id_str, source_id);
            },
        }
    }
    
    // Second pass: identify definitions and their documentation
    for (id, vertex) in &index.vertices {
        if let Vertex::Range { range: _, tag } = vertex {
            if let Some(tag) = tag {
                if let RangeTag::Definition(def_tag) = tag {

                    // def_tag.text is not public, therefore workaround
                    let json = serde_json::to_string(&def_tag).unwrap_or_default();
                    let def_tag = match serde_json::from_str::<MyDefinitionTag>(&json).ok() {
                        Some(s) => s,
                        None => continue,
                    };
                    
                    // Get the definition text
                    let text = &def_tag.text;
                    
                    // Find the full path
                    let path = extract_full_path(id, &index, text);
                    
                    // Find hover text (documentation)
                    let hover_text = find_hover_text(id, &index);
                    
                    // Determine symbol type
                    let symbol_type = SymbolType::from_kind(def_tag.kind);
                    
                    // Store the definition
                    index.definitions.insert(path.clone(), (id.clone(), hover_text));
                    index.symbol_types.insert(path, symbol_type);
                }
            }
        }
    }
    
    Ok(index)
}

fn clone_edge(e: &lsp_types::lsif::Edge) -> Option<lsp_types::lsif::Edge> {
    serde_json::from_str(&serde_json::to_string(e).ok()?).ok()
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyDefinitionTag {
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


/// Extract a full path for a symbol from its range vertex
fn extract_full_path(range_id: &str, index: &LsifIndex, name: &str) -> String {
    // Look for outgoing "next" edges from this range
    if let Some(edges) = index.outgoing_edges.get(range_id) {
        for (_, edge) in edges {
            if let Edge::Next(data) = edge {
                let result_set_id = number_or_string_to_string(&data.in_v);
                
                // Look for outgoing "moniker" edges from the result set
                if let Some(rs_edges) = index.outgoing_edges.get(&result_set_id) {
                    for (_, rs_edge) in rs_edges {
                        if let Edge::Moniker(m_data) = rs_edge {
                            let moniker_id = number_or_string_to_string(&m_data.in_v);
                            
                            // Get the moniker vertex
                            if let Some(Vertex::Moniker(moniker)) = index.vertices.get(&moniker_id) {
                                // Return the identifier from the moniker
                                return moniker.identifier.clone();
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Fallback to just the name if we can't extract the full path
    name.to_string()
}

/// Find hover text (documentation) for a symbol
fn find_hover_text(range_id: &str, index: &LsifIndex) -> String {
    // Follow "next" edge to get to result set
    if let Some(outgoing) = index.outgoing_edges.get(range_id) {
        for (_, edge) in outgoing {
            if let Edge::Next(data) = edge {
                let result_set_id = number_or_string_to_string(&data.in_v);
                
                // Follow "hover" edge to get hover result
                if let Some(rs_edges) = index.outgoing_edges.get(&result_set_id) {
                    for (_, edge) in rs_edges {
                        if let Edge::Hover(hover_data) = edge {
                            let hover_id = number_or_string_to_string(&hover_data.in_v);
                            
                            // Extract hover content
                            if let Some(Vertex::HoverResult { result }) = &index.vertices.get(&hover_id) {
                                match &result.contents {
                                    lsp_types::HoverContents::Scalar(marked_string) => {
                                        match marked_string {
                                            MarkedString::String(s) => return s.clone(),
                                            MarkedString::LanguageString(ls) => return ls.value.clone(),
                                        }
                                    },
                                    lsp_types::HoverContents::Array(arr) => {
                                        if !arr.is_empty() {
                                            match &arr[0] {
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
    if let Some((_, doc)) = index.definitions.get(path) {
        // Extract module name from path
        let module_name = path.split("::").next().unwrap_or(path);
        
        // Format the documentation as requested
        let formatted_doc = format!("```rust\n{}\n```\n\n```rust\n{}\n```", module_name, doc);
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
    
    // Categorize symbols by their type
    for (path, symbol_type) in &index.symbol_types {
        match symbol_type {
            SymbolType::Enum => enums.push(path.clone()),
            SymbolType::Function => functions.push(path.clone()),
            SymbolType::Constant => constants.push(path.clone()),
            SymbolType::Struct => structs.push(path.clone()),
            SymbolType::Unknown => unknown.push(path.clone()),
        }
    }
    
    // Sort items for consistent output
    enums.sort();
    functions.sort();
    constants.sort();
    structs.sort();
    unknown.sort();
    
    // Format the output
    let mut result = String::new();
    
    result.push_str("ENUMS:\n");
    for path in enums {
        result.push_str(&format!("  {}\n", path));
    }
    
    result.push_str("\nFUNCTIONS:\n");
    for path in functions {
        result.push_str(&format!("  {}\n", path));
    }
    
    result.push_str("\nCONSTANTS:\n");
    for path in constants {
        result.push_str(&format!("  {}\n", path));
    }
    
    result.push_str("\nSTRUCTS:\n");
    for path in structs {
        result.push_str(&format!("  {}\n", path));
    }
    
    if !unknown.is_empty() {
        result.push_str("\nUNKNOWN:\n");
        for path in unknown {
            result.push_str(&format!("  {}\n", path));
        }
    }
    
    result
}


#[test]
fn test_parse_and_query() {
    let lsif_content = include_str!("../out.txt");
    let index = parse_lsif(lsif_content).unwrap();
    println!("{index:?}");

    println!("----");

    // Test list function
    let list_result = list(&index);
    println!("{list_result}");
    assert!(!list_result.contains("FUNCTIONS:"));
}
