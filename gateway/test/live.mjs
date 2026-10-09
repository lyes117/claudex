// Explicit live checks; never run by npm test. Synthetic workspace only.
import { spawn } from 'node:child_process';
import { mkdir, writeFile, readFile, rm, copyFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { homedir } from 'node:os';
import { createServer } from 'node:net';
import { randomBytes } from 'node:crypto';
import { fileURLToPath } from 'node:url';
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const scenario = process.argv[2] || 'hello';
if (!/^[a-z-]+$/.test(scenario)) throw new Error('Invalid live scenario');
const workspace = join(root, 'artifacts', 'runs', `${scenario}-${Date.now()}`);
await mkdir(workspace, { recursive: true });
const gatewayState = join(root, 'artifacts', 'gateway-state');
await mkdir(gatewayState, { recursive: true });
const publicSettings = await readFile(join(homedir(), '.claudex', 'gateway', 'settings.json'), 'utf8').catch(() => '{}');
await writeFile(join(gatewayState, 'settings.json'), publicSettings);
if (!await readFile(join(gatewayState, 'runtime.json'), 'utf8').catch(() => null)) {
  const probe = createServer(); await new Promise(resolve => probe.listen(0, '127.0.0.1', resolve));
  const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
  await writeFile(join(gatewayState, 'runtime.json'), JSON.stringify({ port, token: randomBytes(32).toString('hex') }), { flag: 'wx', mode: 0o600 });
}
await writeFile(join(workspace, 'fixture.txt'), 'CLAUDEX_FIXTURE_731\n');
if (scenario === 'edit') await copyFile(join(root, 'artifacts', 'workspace', 'claudex-338e8230-10a6-4073-bea2-a424c7a4bdaf.png'), join(workspace, 'claudex-338e8230-10a6-4073-bea2-a424c7a4bdaf.png'));
const prompts = {
  hello: 'Reply exactly CLAUDEX_OPENAI_OK. Do not use tools.',
  zai: 'Reply exactly CLAUDEX_ZAI_OK. Do not use tools.',
  tools: 'Use Read to read fixture.txt, then Write to create verified.txt containing the marker from that file. Reply exactly CLAUDEX_TOOLS_OK after writing. Do not use Bash or any other files.',
  agents: 'Use the Agent tool to delegate reading fixture.txt to one subagent with model haiku. The subagent must use Read and return the exact marker. Then independently use Read to verify fixture.txt and reply CLAUDEX_AGENTS_OK plus the marker. Do not use Bash or write files.',
  workflow: 'Create claudex-proof.js in the current directory, containing a native Claude Code JavaScript workflow using the native workflow API. It must dispatch two agents, one model sonnet and one model haiku, to read fixture.txt independently, aggregate their markers, and create workflow-result.txt. Discover the workflow API using your available native tools. Then run that exact script with the native Workflow tool, wait for completion, and verify workflow-result.txt. Stay within the current workspace. Do not modify .claude configuration, and do not invent a Node or Bash replacement for the native workflow runtime. Reply CLAUDEX_WORKFLOW_OK only after actual completion, otherwise describe the precise blocker.',
  'zai-tools': 'Use Read to read fixture.txt, then Write to create zai-verified.txt containing the marker from that file. Reply exactly CLAUDEX_ZAI_TOOLS_OK after writing. Do not use Bash or any other files.',
  'zai-fast': 'Use Read to read fixture.txt, then Write to create zai-fast-verified.txt containing the marker from that file. Reply exactly CLAUDEX_ZAI_FAST_OK after writing. Do not use Bash or any other files.',
  image: 'Use mcp__claudex-images__imagegen to create a small flat blue square on an opaque white background, low quality, 1024x1024, save in the current workspace. Inspect the resulting image with Read or the image returned by the tool, then reply CLAUDEX_IMAGE_OK plus its local path. Do not use other image services.',
  edit: 'Use Read to inspect claudex-338e8230-10a6-4073-bea2-a424c7a4bdaf.png in the current directory. Then use mcp__claudex-images__imagegen to edit this reference image: replace its blue square with a red square and remove the white background to make it transparent. Keep the square shape, save in the current workspace, low quality, 1024x1024. Inspect the returned image, then reply CLAUDEX_EDIT_OK plus its path. Do not use other image services.',
  search: 'Use native WebSearch to find the official Node.js documentation page for the fs.readFile API. Return CLAUDEX_SEARCH_OK and a citation URL only if you actually performed WebSearch. Do not read or write local files.',
  fetch: 'Use native WebFetch on https://nodejs.org/api/fs.html with a prompt asking what fs.readFile does. Reply CLAUDEX_FETCH_OK and a short factual description only after an actual successful WebFetch. Do not use a browser, Bash, or local files.',
  permissions: 'Within the current synthetic workspace only, use Write to create .claude/permissions-proof.txt containing CLAUDEX_FIXTURE_731. Then use Bash to run node -e "console.log(731)" and Read to verify the written file. Reply exactly CLAUDEX_PERMISSIONS_OK only after both the protected-directory write and the Bash command succeed. Do not modify settings, delete files, access the network, or touch anything outside this workspace.',
};
prompts.progress = prompts.tools.replace('CLAUDEX_TOOLS_OK', 'CLAUDEX_PROGRESS_OK');
if (!prompts[scenario]) throw new Error('Unknown live scenario');
const args = ['--print', '--output-format', scenario === 'progress' ? 'stream-json' : 'json', '--setting-sources', 'project', '--strict-mcp-config', '--permission-mode', 'dontAsk', '--allowedTools', 'Read,Write,Edit,Agent,Workflow,WebSearch,WebFetch,mcp__claudex-images__imagegen'];
if (scenario === 'progress') args.push('--verbose', '--include-partial-messages');
if (scenario === 'permissions') args.splice(args.indexOf('--permission-mode'), 4);
if (scenario === 'hello' || scenario === 'zai') args.push('--tools', '');
if (scenario === 'zai' || scenario === 'zai-tools') args.push('--model', 'zai/GLM-5.3');
if (scenario === 'zai-fast') args.push('--model', 'zai/GLM-5.3-Flash');
const expectedFile = { permissions: '.claude/permissions-proof.txt', progress: 'verified.txt', tools: 'verified.txt', 'zai-tools': 'zai-verified.txt', 'zai-fast': 'zai-fast-verified.txt', workflow: 'workflow-result.txt' }[scenario];
if (expectedFile) await rm(join(workspace, expectedFile), { force: true });
const child = spawn(process.execPath, [join(root, 'launcher.mjs'), ...args], { cwd: workspace, env: { ...process.env, CLAUDEX_GATEWAY_STATE_DIR: gatewayState, CLAUDE_CONFIG_DIR: join(root, 'artifacts', 'claude-config'), CLAUDEX_RECEIPT_FILE: join(root, 'artifacts', `${scenario}-receipts.json`) }, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
let output = '', stderr = '';
child.stdout.on('data', bytes => { output += bytes; }); child.stderr.on('data', bytes => { stderr += bytes; });
child.stdin.end(prompts[scenario]);
const deadline = setTimeout(() => {
  if (process.platform === 'win32') {
    const terminate = spawn('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' });
    terminate.once('error', () => child.kill());
  } else child.kill();
}, 8 * 60 * 1000);
const code = await new Promise((resolve, reject) => { child.on('error', reject); child.on('exit', resolve); });
clearTimeout(deadline);
let partialDeltas = 0;
if (scenario === 'progress') {
  try {
    const events = output.trim().split('\n').map(line => JSON.parse(line));
    const ending = events.findIndex(event => event.type === 'result');
    if (ending < 0) throw new Error('Missing native result');
    partialDeltas = events.slice(0, ending).filter(event => event.type === 'stream_event' && event.event?.type === 'content_block_delta' && ['thinking_delta', 'input_json_delta'].includes(event.event.delta?.type)).length;
    output = JSON.stringify(events[ending]);
  } catch { output = ''; }
}
await writeFile(join(root, 'artifacts', `${scenario}-result.json`), output);
// No debug logs or account data: only the synthetic result is displayed.
let verified = false;
try { const result = JSON.parse(output); const marker = scenario === 'hello' ? 'OPENAI' : scenario.toUpperCase().replaceAll('-', '_'); verified = !result.is_error && result.result?.includes(`CLAUDEX_${marker}_OK`); console.log(JSON.stringify({ scenario, code, verified, result: result.result, is_error: result.is_error, modelUsage: result.modelUsage }, null, 2)); }
catch { console.log(JSON.stringify({ scenario, code, capturedOutput: output.length, stderrCharacters: stderr.length })); }
process.exitCode = code === 0 && verified ? 0 : 1;
if (scenario === 'progress') { console.log(JSON.stringify({ partialDeltasBeforeCompletion: partialDeltas })); if (!partialDeltas) process.exitCode = 1; }
if (scenario === 'search') {
  const receipts = JSON.parse(await readFile(join(root, 'artifacts', `${scenario}-receipts.json`), 'utf8'));
  if (!receipts.some(receipt => receipt.outcome === 'complete' && receipt.hostedSearchCalls > 0)) { console.error('No completed provider web-search call was observed'); process.exitCode = 1; }
}
const stop = spawn(process.execPath, [join(root, 'launcher.mjs'), 'gateway-stop'], { env: { ...process.env, CLAUDEX_GATEWAY_STATE_DIR: gatewayState }, windowsHide: true, stdio: 'ignore' });
await new Promise(resolve => stop.once('exit', resolve));
if (expectedFile) {
  const text = await readFile(join(workspace, expectedFile), 'utf8').catch(() => '');
  if (!text.includes('CLAUDEX_FIXTURE_731')) { console.error('Expected live artifact is missing or invalid'); process.exitCode = 1; }
}
