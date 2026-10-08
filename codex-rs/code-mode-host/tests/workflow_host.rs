//! Real private host process and V8 execution, with synthetic framed responses.
//! These fixtures never authenticate, call a model, or prove native agent admission.
use std::process::Stdio;
use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::process::Command;
use tokio::time::timeout;

const DEADLINE: Duration = Duration::from_secs(15);

async fn spawn() -> Child {
    spawn_mode("--workflow-host").await
}

async fn spawn_mode(mode: &str) -> Child {
    let mut child = Command::new(
        codex_utils_cargo_bin::cargo_bin("codex-code-mode-host").expect("host binary"),
    )
    .arg(mode)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true)
    .spawn()
    .expect("host process");
    assert_eq!(read(&mut child).await, json!({"kind":"ready","version":1}));
    child
}

async fn write(child: &mut Child, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    timeout(DEADLINE, async {
        let input = child.stdin.as_mut().unwrap();
        input
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .unwrap();
        input.write_all(&bytes).await.unwrap();
        input.flush().await.unwrap();
    })
    .await
    .expect("frame deadline");
}

async fn read(child: &mut Child) -> Value {
    serde_json::from_slice(&read_bytes(child).await).unwrap()
}

async fn read_bytes(child: &mut Child) -> Vec<u8> {
    timeout(DEADLINE, async {
        let output = child.stdout.as_mut().unwrap();
        let length = output.read_u32().await.unwrap() as usize;
        assert!(length > 0 && length <= 1024 * 1024);
        let mut bytes = vec![0; length];
        output.read_exact(&mut bytes).await.unwrap();
        bytes
    })
    .await
    .expect("frame deadline")
}

async fn start(child: &mut Child, script: &str) {
    write(
        child,
        json!({"kind":"start","version":1,"script":script,"arguments":{"topic":"été"}}),
    )
    .await;
}

#[tokio::test]
async fn real_v8_groups_parallel_and_pipeline_in_order() {
    let mut child = spawn().await;
    let script = r#"
      const schema = {type:'object',properties:{text:{type:'string'}},required:['text'],additionalProperties:false};
      phase('Examiner');
      const first = await parallel(['a','b'].map(label => () => agent(args.topic + label, {label, schema})));
      phase('Relire');
      const second = await pipeline(first, (value, i) => agent(value.text, {label:`review:${i}`,schema}));
      return second;
    "#;
    start(&mut child, script).await;
    let first = read(&mut child).await;
    assert_eq!(first["kind"], "group");
    assert_eq!(first["sequence"], 1);
    assert_eq!(first["phase"], "Examiner");
    assert_eq!(
        first["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| call["prompt"].clone())
            .collect::<Vec<_>>(),
        vec![json!("étéa"), json!("étéb")]
    );
    write(&mut child, json!({"kind":"group_result","version":1,"sequence":1,"values":[{"text":"A"},{"text":"B"}]})).await;
    let second = read(&mut child).await;
    assert_eq!(second["sequence"], 2);
    assert_eq!(second["phase"], "Relire");
    assert_eq!(
        second["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| call["prompt"].clone())
            .collect::<Vec<_>>(),
        vec![json!("A"), json!("B")]
    );
    write(&mut child, json!({"kind":"group_result","version":1,"sequence":2,"values":[{"text":"OK A"},{"text":"OK B"}]})).await;
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"done","version":1,"result":[{"text":"OK A"},{"text":"OK B"}]})
    );
    assert!(
        timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn late_import_and_escaped_import_refused_before_first_group() {
    for script in [
        "await agent('first', {schema:{type:'object'}}); await import('late');",
        r"await agent('first', {schema:{type:'object'}}); await \u0069mport('late');",
    ] {
        let mut child = spawn().await;
        start(&mut child, script).await;
        assert_eq!(
            read(&mut child).await,
            json!({"kind":"failed","version":1,"code":"script"})
        );
        assert!(
            !timeout(DEADLINE, child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

#[tokio::test]
async fn local_script_budget_is_independent_from_agent_context_budget() {
    let mut child = spawn().await;
    let script = format!(
        "/* {} */ return {{topic:args.topic}};",
        "x".repeat(256 * 1024)
    );
    start(&mut child, &script).await;
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"done","version":1,"result":{"topic":"été"}})
    );
    assert!(
        timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn unknown_sequence_fails_without_second_group() {
    let mut child = spawn().await;
    start(
        &mut child,
        "return await agent('one', {schema:{type:'object'}});",
    )
    .await;
    assert_eq!(read(&mut child).await["sequence"], 1);
    write(
        &mut child,
        json!({"kind":"group_result","version":1,"sequence":2,"values":[{}]}),
    )
    .await;
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"failed","version":1,"code":"protocol"})
    );
    assert!(
        !timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn caught_clock_random_and_eval_cannot_obtain_native_entropy_or_code_generation() {
    let mut child = spawn().await;
    start(&mut child, "const results=[]; for (const f of [()=>Date.now(),()=>Math.random(),()=>Function('return 1')(),()=>eval('1'),()=>Intl.DateTimeFormat().format()]) {try {f();results.push(false)} catch {results.push(true)}} return results;").await;
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"done","version":1,"result":[true,true,true,true,true]})
    );
    assert!(
        timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn unrepresentable_parent_numbers_are_refused_without_rounding() {
    let mut child = spawn().await;
    start(
        &mut child,
        "return await agent('one', {schema:{type:'object'}});",
    )
    .await;
    assert_eq!(read(&mut child).await["sequence"], 1);
    write(
        &mut child,
        json!({"kind":"group_result","version":1,"sequence":1,"values":[{"number":0.5}]}),
    )
    .await;
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"failed","version":1,"code":"unsupported"})
    );
    assert!(
        !timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn rejected_top_level_script_revokes_queued_agents_before_group_emission() {
    for script in [
        "agent('one', {schema:{type:'object'}}); throw Error('stop');",
        "agent('one', {schema:{type:'object'}}); await Promise.reject(Error('stop'));",
    ] {
        let mut child = spawn().await;
        start(&mut child, script).await;
        // Failed must be the first frame after Ready, so no group/admission is exposed.
        assert_eq!(
            read(&mut child).await,
            json!({"kind":"failed","version":1,"code":"script"})
        );
        assert!(
            !timeout(DEADLINE, child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

#[tokio::test]
async fn reserved_json_keys_survive_real_v8_argument_and_output_boundaries() {
    let mut child = spawn().await;
    let arguments = json!({
        "$serde_json::private::Number":"123456789012345678901234567890",
        "$serde_json::private::RawValue":"[[[[[[[[[[[[[[null]]]]]]]]]]]]]]"
    });
    write(
        &mut child,
        json!({"kind":"start","version":1,"script":"return args;","arguments":arguments}),
    )
    .await;
    // Inspect raw output, avoiding serde_json Value's own private-key interpretation.
    let output = String::from_utf8(read_bytes(&mut child).await).unwrap();
    assert!(output.contains(r#""$serde_json::private::Number":"123456789012345678901234567890""#));
    assert!(
        output.contains(r#""$serde_json::private::RawValue":"[[[[[[[[[[[[[[null]]]]]]]]]]]]]]""#)
    );
    assert!(output.contains(r#""kind":"done""#));
    assert!(
        timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn agent_without_schema_is_refused_before_group_emission() {
    let mut child = spawn().await;
    start(&mut child, "return await agent('one');").await;
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"failed","version":1,"code":"script"})
    );
    assert!(
        !timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn final_json_conversion_cannot_hide_faults_or_admit_new_agents() {
    for script in [
        "return {toJSON(){try {agent('one',{schema:null})} catch {} return {};}};",
        "return {toJSON(){try {agent('one',{schema:{type:'object'}})} catch {} return {};}};",
        "return {toJSON(){Promise.resolve().then(()=>agent('one',{schema:{type:'object'}})); return {};}};",
    ] {
        let mut child = spawn().await;
        start(&mut child, script).await;
        assert_eq!(
            read(&mut child).await,
            json!({"kind":"failed","version":1,"code":"script"})
        );
        assert!(
            !timeout(DEADLINE, child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

#[tokio::test]
async fn entire_group_numeric_preflight_precedes_observable_promise_resolution() {
    let mut child = spawn().await;
    let script = r#"
      const schema={type:'object'};
      const results=parallel([()=>agent('first',{schema}),()=>agent('second',{schema})]);
      Object.defineProperty(Object.prototype,'then',{configurable:true,get(){
        throw Error('partial resolution observed');
      }});
      return await results;
    "#;
    start(&mut child, script).await;
    assert_eq!(read(&mut child).await["kind"], "group");
    write(
        &mut child,
        json!({"kind":"group_result","version":1,"sequence":1,"values":[{},0.5]}),
    )
    .await;
    // Unsupported numeric input wins before the first resolver can invoke that getter.
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"failed","version":1,"code":"unsupported"})
    );
    assert!(
        !timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn ast_preflight_accepts_prompt_import_text_and_edits_only_actual_meta_export() {
    let mut child = spawn().await;
    let script = r#"
      // export const meta = wrong; import('comment');
      export /* whitespace is preserved */
      const meta = {name:'fixture'};
      const obj={import(){return 'property'}};
      const prompt=`node -e "import('module').then(console.log)"`;
      return {prompt, property:obj.import(), unicode:'\u00e9'};
    "#;
    start(&mut child, script).await;
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"done","version":1,"result":{"prompt":"node -e \"import('module').then(console.log)\"","property":"property","unicode":"é"}})
    );
    assert!(
        timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn actual_template_substitution_import_is_refused_before_first_agent() {
    let mut child = spawn().await;
    start(
        &mut child,
        "await agent('first',{schema:{type:'object'}}); return `text ${await import('late')}`;",
    )
    .await;
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"failed","version":1,"code":"script"})
    );
    assert!(
        !timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn private_parse_only_mode_never_evaluates_metadata_or_workflow_body() {
    let mut child = spawn_mode("--workflow-preflight").await;
    let script = "export const meta=agent('metadata',{schema:{type:'object'}}); while(true){}";
    start(&mut child, script).await;
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"done","version":1,"result":{"parseOnly":true,"v8Compiled":false,"sourceBytes":script.len(),"metaTokenRemoved":true,"agentsAdmitted":0}})
    );
    assert!(
        timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
#[ignore = "Manual read-only fixture: supply CLAUDEX_FILM_SOURCE_PATH; never executes the workflow"]
async fn actual_film_source_ast_preflight_only() {
    use std::io::Read;
    let path = std::path::PathBuf::from(
        std::env::var_os("CLAUDEX_FILM_SOURCE_PATH").expect("film source path required"),
    );
    assert_eq!(path.file_name().unwrap(), "film.workflow.js");
    let file = std::fs::File::open(path).expect("local workflow source");
    assert!(file.metadata().unwrap().len() <= 512 * 1024);
    let mut bytes = Vec::new();
    file.take(512 * 1024 + 1).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 512 * 1024);
    let script = String::from_utf8(bytes).expect("UTF8 local source");
    let mut child = spawn_mode("--workflow-preflight").await;
    start(&mut child, &script).await;
    assert_eq!(
        read(&mut child).await,
        json!({"kind":"done","version":1,"result":{"parseOnly":true,"v8Compiled":false,"sourceBytes":script.len(),"metaTokenRemoved":true,"agentsAdmitted":0}})
    );
    assert!(
        timeout(DEADLINE, child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}
