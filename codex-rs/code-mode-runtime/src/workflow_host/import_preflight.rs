//! Bounded syntax preflight using the official JavaScript tree-sitter grammar.
//! No metadata or script expression is evaluated. V8 compilation remains authoritative.
use std::ops::Range;
use std::time::Duration;
use std::time::Instant;

use tree_sitter::Node;
use tree_sitter::ParseOptions;
use tree_sitter::Parser;

use super::codec::Fault;
use super::codec::Result;
use super::codec::SCRIPT_BYTES;

#[derive(Clone, Copy)]
struct Limits {
    nodes: usize,
    depth: usize,
    parser_checks: usize,
    time: Duration,
}

const LIMITS: Limits = Limits {
    nodes: 200_000,
    depth: 512,
    parser_checks: 4096,
    time: Duration::from_secs(/*secs*/ 2),
};

pub(super) fn prepare(source: &str) -> Result<String> {
    prepare_with_limits(source, LIMITS)
}

fn prepare_with_limits(source: &str, limits: Limits) -> Result<String> {
    if source.is_empty() || source.len() > SCRIPT_BYTES {
        return Err(Fault::Limit);
    }
    let started = Instant::now();
    let mut checks = 0;
    let mut progress = |_: &tree_sitter::ParseState| {
        checks += 1;
        checks > limits.parser_checks || started.elapsed() >= limits.time
    };
    let mut input =
        |offset: usize, _: tree_sitter::Point| source.as_bytes().get(offset..).unwrap_or_default();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .map_err(|_| Fault::Unsupported)?;
    let tree = parser
        .parse_with_options(
            &mut input,
            /*old_tree*/ None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )
        .ok_or(Fault::Limit)?;
    let root = tree.root_node();
    if root.has_error() {
        return Err(Fault::Script);
    }
    let mut cursor = tree.walk();
    let mut depth = 0;
    let mut nodes = 0;
    let mut export = None;
    loop {
        nodes += 1;
        if nodes > limits.nodes || depth > limits.depth || started.elapsed() >= limits.time {
            return Err(Fault::Limit);
        }
        let node = cursor.node();
        match node.kind() {
            "import" | "import_statement" => return Err(Fault::Script),
            "export_statement" => {
                if export.is_some() {
                    return Err(Fault::Script);
                }
                export = Some(meta_export_token(node, source)?);
            }
            "expression_statement"
                if node.named_child(0).is_some_and(|child| {
                    child.kind() == "identifier"
                        && &source.as_bytes()[child.byte_range()] == b"export"
                }) =>
            {
                // The JavaScript grammar can represent a newline-separated export
                // as an identifier statement followed by its declaration. Only
                // admit this exact top-level token and the same const-meta shape.
                if export.is_some()
                    || !node
                        .parent()
                        .is_some_and(|parent| parent.kind() == "program")
                {
                    return Err(Fault::Script);
                }
                let token = node.named_child(0).ok_or(Fault::Script)?;
                let mut statement_cursor = node.walk();
                if node
                    .children(&mut statement_cursor)
                    .any(|child| child.id() != token.id() && child.kind() != "comment")
                {
                    return Err(Fault::Script);
                }
                let mut next = node.next_named_sibling();
                while next.is_some_and(|sibling| sibling.kind() == "comment") {
                    next = next.and_then(|sibling| sibling.next_named_sibling());
                }
                validate_meta_declaration(next.ok_or(Fault::Script)?, source)?;
                export = Some(token.byte_range());
            }
            "identifier" => {
                // Escaped identifiers are a private-stage conservative restriction.
                // String/comment Unicode escapes remain untouched; property identifiers
                // such as obj.import are not identifier nodes and are permitted.
                if source.as_bytes()[node.byte_range()].contains(&b'\\') {
                    return Err(Fault::Script);
                }
            }
            _ => {}
        }
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                let mut prepared = source.as_bytes().to_vec();
                if let Some(range) = export {
                    prepared[range].fill(b' ');
                }
                return String::from_utf8(prepared).map_err(|_| Fault::Script);
            }
            depth = depth.checked_sub(1).ok_or(Fault::Script)?;
        }
    }
}

fn meta_export_token(node: Node<'_>, source: &str) -> Result<Range<usize>> {
    if !node
        .parent()
        .is_some_and(|parent| parent.kind() == "program")
    {
        return Err(Fault::Script);
    }
    if node.child_by_field_name("decorator").is_some() {
        return Err(Fault::Script);
    }
    let declaration = node
        .child_by_field_name("declaration")
        .ok_or(Fault::Script)?;
    validate_meta_declaration(declaration, source)?;
    let mut cursor = node.walk();
    let mut token = None;
    for child in node.children(&mut cursor) {
        if child.id() == declaration.id() || child.kind() == "comment" {
            continue;
        }
        if child.kind() == "export" && token.is_none() {
            token = Some(child);
        } else {
            // Includes default, namespace/clause/source tokens and extra syntax.
            return Err(Fault::Script);
        }
    }
    let token = token.ok_or(Fault::Script)?;
    let range = token.byte_range();
    if &source.as_bytes()[range.clone()] != b"export" {
        return Err(Fault::Script);
    }
    Ok(range)
}

fn validate_meta_declaration(declaration: Node<'_>, source: &str) -> Result<()> {
    if declaration.kind() != "lexical_declaration"
        || !declaration
            .child_by_field_name("kind")
            .is_some_and(|kind| kind.kind() == "const")
    {
        return Err(Fault::Script);
    }
    let mut cursor = declaration.walk();
    let mut declarators = declaration
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "variable_declarator");
    let declarator = declarators.next().ok_or(Fault::Script)?;
    if declarators.next().is_some() {
        return Err(Fault::Script);
    }
    let name = declarator
        .child_by_field_name("name")
        .ok_or(Fault::Script)?;
    if name.kind() != "identifier"
        || &source.as_bytes()[name.byte_range()] != b"meta"
        || declarator.child_by_field_name("value").is_none()
    {
        return Err(Fault::Script);
    }
    Ok(())
}

#[cfg(test)]
#[path = "import_preflight_tests.rs"]
mod tests;
