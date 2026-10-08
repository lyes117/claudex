use super::*;
use pretty_assertions::assert_eq;

#[test]
fn strings_comments_regex_properties_and_unicode_text_remain_unchanged() {
    for source in [
        "// import('x')\n/* export const meta=bad */ return 'import(never)';",
        r#"const prompt = `node -e "import('file').then(console.log)"`; return prompt;"#,
        r#"const prompt = "\\u0069mport('plain string')"; return prompt;"#,
        r#"const x=/import|export const meta/; return x.source;"#,
        "const obj={import(){return 'été'}}; return obj.import();",
        "return `plain ${'import'} and ${ {import:'ok'}.import }`;",
    ] {
        assert_eq!(prepare(source).unwrap(), source);
    }
}

#[test]
fn all_actual_import_locations_are_refused_before_script_execution() {
    for source in [
        "import x from 'x'; return x;",
        "import 'x';",
        "await agent('first',{schema:{type:'object'}}); await import('late');",
        "return `nested ${await import('late')}`;",
        "const load=()=>import('later'); return load;",
        "return import.meta.url;",
        r"await agent('first',{schema:{type:'object'}}); await \u0069mport('late');",
        "export * from 'late';",
        "export {x} from 'late';",
    ] {
        assert_eq!(prepare(source), Err(Fault::Script));
    }
}

#[test]
fn only_one_top_level_const_meta_export_token_is_blank_preserving_lines() {
    let source = "// export const meta=wrong\nexport/*guard*/\nconst meta = {name:'x'};\nreturn 'export const meta';";
    let expected = "// export const meta=wrong\n      /*guard*/\nconst meta = {name:'x'};\nreturn 'export const meta';";
    assert_eq!(prepare(source).unwrap(), expected);
    for (source, expected) in [
        ("export\nconst meta={};", "      \nconst meta={};"),
        (
            "export// guard\nconst meta={};",
            "      // guard\nconst meta={};",
        ),
        (
            "export/*guard*/\r\nconst meta={};",
            "      /*guard*/\r\nconst meta={};",
        ),
    ] {
        assert_eq!(prepare(source).unwrap(), expected);
    }
    for source in [
        "export let meta={};",
        "export const other={};",
        "export const meta={},other={};",
        "export default {};",
        "export default const meta={};",
        "export default function meta(){}",
        "export default class meta{}",
        "export {meta};",
        "export * from 'x';",
        "export const meta={}; export const meta={};",
        "if(true){export const meta={};}",
        "export const {meta}={meta:{}};",
        "export/*guard*/\nlet meta={};",
        "export/*guard*/\nconst other={};",
        "export/*guard*/\nconst meta={},other={};",
        "export/*guard*/\nconst {meta}={meta:{}};",
        "export\nreturn {};",
        "export\nconst meta={}; export\nconst meta={};",
        "if(true){export\nconst meta={};}",
    ] {
        assert_eq!(prepare(source), Err(Fault::Script));
    }
}

#[test]
fn source_depth_nodes_and_parser_progress_are_independently_bounded() {
    assert_eq!(prepare(&";".repeat(SCRIPT_BYTES + 1)), Err(Fault::Limit));
    let deeply_nested = format!("return {}0{};", "(".repeat(600), ")".repeat(600));
    assert_eq!(prepare(&deeply_nested), Err(Fault::Limit));
    assert_eq!(
        prepare_with_limits("return [1,2,3,4,5];", Limits { nodes: 4, ..LIMITS }),
        Err(Fault::Limit)
    );
    // A dense valid source exercises the real native parser's progress cancellation.
    let dense = ";\n".repeat(50_000);
    assert_eq!(
        prepare_with_limits(
            &dense,
            Limits {
                parser_checks: 1,
                time: Duration::from_secs(60),
                ..LIMITS
            }
        ),
        Err(Fault::Limit)
    );
}

#[test]
fn malformed_syntax_has_a_generic_script_fault() {
    for source in ["const x = ;", "`unterminated", "export const meta="] {
        assert_eq!(prepare(source), Err(Fault::Script));
    }
}
