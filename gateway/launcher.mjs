import { spawn } from 'node:child_process';
import { writeFile, rename } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { aliases, modelIds, loadSettings, openaiContextWindow } from './settings.mjs';
import { providerStatus } from './providers.mjs';
import { ensureGateway, stopGateway, gatewayHealth, launchSettingsFile, mcpConfigFile } from './daemon.mjs';

const root = dirname(fileURLToPath(import.meta.url));
export const claudeExe = join(homedir(), '.local', 'bin', 'claude.exe');

export function permissionArgs(args) {
  // An explicit mode or restricted session always wins over the autonomous default.
  return args.some(arg => arg === '--restricted' || arg === '--dangerously-skip-permissions' || arg === '--permission-mode' || arg.startsWith('--permission-mode=')) ? args : ['--permission-mode', 'bypassPermissions', ...args];
}

export async function launch(args = process.argv.slice(2)) {
  if (args[0] === 'doctor') {
    const settings = await loadSettings();
    console.log(JSON.stringify({ claudeExecutable: claudeExe, credentials: await providerStatus(), routes: settings.routes, zaiFallback: settings.zaiFallback || null }, null, 2));
    return;
  }
  if (args[0] === 'gateway-stop') { await stopGateway(); return; }
  const runtime = await ensureGateway();
  const { token, port } = runtime;
  const settings = await loadSettings();
  const ids = modelIds(settings);
    const env = {
      ANTHROPIC_BASE_URL: `http://127.0.0.1:${port}`, ANTHROPIC_AUTH_TOKEN: token, ANTHROPIC_API_KEY: '',
      ANTHROPIC_DEFAULT_OPUS_MODEL: aliases.opus, ANTHROPIC_DEFAULT_SONNET_MODEL: aliases.sonnet, ANTHROPIC_DEFAULT_HAIKU_MODEL: aliases.haiku,
      ANTHROPIC_MODEL: aliases.opus, ANTHROPIC_SMALL_FAST_MODEL: aliases.haiku,
      CLAUDE_CODE_GATEWAY_HINT_HEADERS: '1', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1',
      CLAUDE_CODE_MAX_CONTEXT_TOKENS: String(openaiContextWindow),
      CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT: '1',
      ENABLE_TOOL_SEARCH: 'false',
      CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY: '1',
      ANTHROPIC_DEFAULT_OPUS_MODEL_NAME: settings.routes.opus.model,
      ANTHROPIC_DEFAULT_SONNET_MODEL_NAME: settings.routes.sonnet.model,
      ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME: settings.routes.haiku.model,
      ANTHROPIC_CUSTOM_MODEL_OPTION: aliases.opus,
      ANTHROPIC_CUSTOM_MODEL_OPTION_NAME: settings.routes.opus.model,
      ...Object.fromEntries(Object.entries(settings.routes).flatMap(([tier, route]) => [
        [`ANTHROPIC_DEFAULT_${tier.toUpperCase()}_MODEL_DESCRIPTION`, `${route.provider} · abonnement · ${tier}${route.effort ? ` · ${route.effort}` : ''}`],
        [`ANTHROPIC_DEFAULT_${tier.toUpperCase()}_MODEL_SUPPORTED_CAPABILITIES`, route.provider === 'openai' ? 'effort,xhigh_effort,thinking,interleaved_thinking' : ''],
      ])),
    };
    const atomicWrite = async (path, content) => { const temporary = `${path}.${process.pid}`; await writeFile(temporary, JSON.stringify(content), { mode: 0o600 }); await rename(temporary, path); };
    await atomicWrite(launchSettingsFile, { env, autoCompactWindow: 720000, modelPicker: { replaceBuiltInOptions: true, options: [
      ...Object.entries(settings.routes).map(([tier, route]) => ({ model: aliases[tier], label: `${route.model}${route.effort ? ` · ${route.effort}` : ''} · auto`, description: `${route.provider} · abonnement · rôle ${tier}` })),
      ...Object.entries(settings.routes).map(([tier, route]) => ({ model: ids[tier], label: `${route.model}${route.effort ? ` · ${route.effort}` : ''}`, description: `${route.provider} · abonnement · modèle fixe` })),
      { model: 'zai/GLM-5.3', label: 'GLM-5.3', description: 'Z.ai Coding Plan' },
      ...settings.catalog.filter(model => !Object.values(settings.routes).some(route => route.model === model.slug)).map(model => ({ model: `openai/${model.slug}`, label: model.slug, description: 'ChatGPT · abonnement' })),
    ] } });
    await atomicWrite(mcpConfigFile, { mcpServers: { 'claudex-images': { command: process.execPath, args: [join(root, 'images-mcp.mjs')] } } });
    // Inherit the real console. Claude owns the TUI, tools, permissions and workflows.
    const claude = spawn(claudeExe, ['--settings', launchSettingsFile, '--mcp-config', mcpConfigFile, ...permissionArgs(args)], { stdio: 'inherit', env: { ...process.env, ...env } });
    const onSignal = () => {}; process.on('SIGINT', onSignal);
    try {
      const code = await new Promise((resolve, reject) => { claude.once('error', reject); claude.once('exit', value => resolve(value ?? 1)); });
      if (process.env.CLAUDEX_RECEIPT_FILE) {
        const health = await gatewayHealth(runtime);
        await writeFile(process.env.CLAUDEX_RECEIPT_FILE, JSON.stringify(health.receipts, null, 2));
      }
      process.exitCode = code;
    } finally { process.off('SIGINT', onSignal); }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  launch().catch(() => { console.error('Claudex could not start. Run claudex doctor; no paid API fallback is used.'); process.exitCode = 1; });
}
