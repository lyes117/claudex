// Local, non-production fixture for the compiled fork. Auth stays in the official Codex home.
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { resolve, join } from 'node:path';
import { homedir } from 'node:os';

const root = resolve('.verification/project');
const claude = join(root, '.claude');
mkdirSync(join(claude, 'agents'), { recursive: true });
mkdirSync(join(claude, 'skills', 'fixture'), { recursive: true });
mkdirSync(join(claude, 'commands'), { recursive: true });
mkdirSync(join(claude, 'rules'), { recursive: true });
spawnSync('git', ['init', '--quiet', root], { windowsHide: true });
const globalPath = join(homedir(), '.claude', 'settings.json');
const globalSettings = existsSync(globalPath) ? JSON.parse(readFileSync(globalPath, 'utf8')) : {};
const disabledPlugins = Object.fromEntries(Object.keys(globalSettings.enabledPlugins || {}).map(key => [key, false]));
const registryPath = join(homedir(), '.claude', 'plugins', 'installed_plugins.json');
if (existsSync(registryPath)) for (const name of Object.keys(JSON.parse(readFileSync(registryPath, 'utf8')).plugins || {})) disabledPlugins[name] = false;
const hookPath = join(root, 'hook.mjs');
const mcpPath = join(root, 'mcp.mjs');
writeFileSync(join(root, 'CLAUDE.md'), 'When the user asks CLAUDEX_CONFIGURATION_CHECK, report CLAUDEX_INSTRUCTIONS_OK.\n');
writeFileSync(join(claude, 'rules', 'unused.md'), '', { flag: 'w' });
writeFileSync(join(claude, 'agents', 'fixture-reviewer.md'), '---\nname: fixture-reviewer\ndescription: Local Claudex verification role\n---\nRespond CLAUDEX_AGENT_OK when asked to verify this role.\n');
writeFileSync(join(claude, 'skills', 'fixture', 'SKILL.md'), '---\nname: fixture\ndescription: Local verification skill\ndisable-model-invocation: true\n---\nRespond CLAUDEX_SKILL_OK with these arguments: $ARGUMENTS\n');
writeFileSync(join(claude, 'commands', 'fixture-command.md'), '---\ndescription: Local verification command\n---\nRespond CLAUDEX_COMMAND_OK with $ARGUMENTS[0] and $1.\n');
writeFileSync(hookPath, `import {appendFileSync} from 'node:fs';
let input=''; for await (const chunk of process.stdin) input+=chunk;
const payload=JSON.parse(input);
appendFileSync('hook-events.jsonl',JSON.stringify({event:payload.hook_event_name,projectDir:process.env.CLAUDE_PROJECT_DIR})+'\\n');
if(payload.hook_event_name==='PreToolUse' && String(payload.tool_input?.command||'').includes('HOOK_DENIED')) console.log(JSON.stringify({hookSpecificOutput:{hookEventName:'PreToolUse',permissionDecision:'deny',permissionDecisionReason:'Claudex local fixture denied'}}));
`);
writeFileSync(mcpPath, `import {createInterface} from 'node:readline';
for await (const line of createInterface({input:process.stdin})) {
 const request=JSON.parse(line); if(request.id===undefined) continue;
 let result={};
 if(request.method==='initialize') result={protocolVersion:request.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'claudex-fixture',version:'1'}};
 if(request.method==='tools/list') result={tools:[{name:'fixture_value',description:'Return the Claudex test value',annotations:{readOnlyHint:true,destructiveHint:false,openWorldHint:false},inputSchema:{type:'object',properties:{},additionalProperties:false}}]};
 if(request.method==='tools/call') result={content:[{type:'text',text:'CLAUDEX_MCP_OK'}]};
 console.log(JSON.stringify({jsonrpc:'2.0',id:request.id,result}));
}
`);
writeFileSync(join(claude, 'settings.json'), JSON.stringify({ enabledPlugins: disabledPlugins,
 permissions: {deny:['Bash(*CLAUDEX_DENIED*)']},
 hooks: {SessionStart:[{hooks:[{type:'command',command:`node "${hookPath}"`}]}],PreToolUse:[{matcher:'Bash',hooks:[{type:'command',command:`node "${hookPath}"`}]}]}
}, null, 2));
writeFileSync(join(root, '.mcp.json'), JSON.stringify({mcpServers:{fixture:{command:'node',args:[mcpPath],type:'stdio'}}}));
writeFileSync(join(root, 'hook-events.jsonl'), '');
const binary = process.env.CLAUDEX_BIN || join(process.env.LOCALAPPDATA, 'Programs', 'Claudex', 'bin', 'claudex.exe');
const prompt = process.argv.includes('--agent')
 ? 'Spawn exactly one fixture-reviewer agent, ask it to verify its configured role without tools, wait for its answer and report that answer. Do not spawn other agents or modify files.'
 : process.argv.includes('--tools')
 ? 'Run the shell command echo CLAUDEX_DENIED once. If blocked, do not bypass it. Then run echo HOOK_DENIED once. If blocked, do not bypass it. Call the fixture MCP fixture_value tool. Report whether each command was blocked and the MCP result. Do not modify any files.'
 : 'CLAUDEX_CONFIGURATION_CHECK. Do not use tools; answer the configured verification phrase only.';
const output = join(root, process.argv.includes('--agent') ? 'agent-output.txt' : process.argv.includes('--tools') ? 'tools-output.txt' : 'auth-output.txt');
const child = spawnSync(binary, ['exec','--ignore-user-config','--skip-git-repo-check','--dangerously-bypass-hook-trust','-s','read-only','-c',`projects={${JSON.stringify(root)}={trust_level="trusted"}}`,'-c','forced_login_method="chatgpt"','-c','model_reasoning_effort="low"','-m','gpt-6.1-sol','-C',root,'--output-last-message',output,prompt], {cwd:root,windowsHide:true,encoding:'utf8',timeout:180000});
writeFileSync(join(root,'runtime.log'), child.stdout + child.stderr);
assert.equal(child.status, 0, 'Claudex runtime failed; inspect the local verification log');
const answer = readFileSync(output,'utf8').trim();
assert.ok(answer.includes(process.argv.includes('--agent') ? 'CLAUDEX_AGENT_OK' : process.argv.includes('--tools') ? 'CLAUDEX_MCP_OK' : 'CLAUDEX_INSTRUCTIONS_OK'));
const hooks = readFileSync(join(root,'hook-events.jsonl'),'utf8').trim().split('\n').map(JSON.parse);
assert.ok(hooks.some(event => event.event === 'SessionStart'));
assert.ok(hooks.every(event => event.projectDir.toLowerCase() === root.toLowerCase()));
if (process.argv.includes('--tools')) {
 assert.ok(child.stderr.includes('tool blocked by Claude deny permission'), 'Claude deny must reach dispatch');
 assert.ok(child.stderr.includes('Claudex local fixture denied'), 'PreToolUse hook must block the second command');
}
console.log('PASS native ChatGPT inference, in-place CLAUDE.md and SessionStart hook' + (process.argv.includes('--tools') ? ', MCP tool and two permission refusals' : process.argv.includes('--agent') ? ', one Claude Markdown agent role' : ''));
