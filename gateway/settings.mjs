import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const codexHome = process.env.CODEX_HOME || join(homedir(), '.codex');
export const stateHome = process.env.CLAUDEX_GATEWAY_STATE_DIR || join(homedir(), '.claudex', 'gateway');
export const settingsPath = join(stateHome, 'settings.json');
// Custom IDs keep proactive compaction while allowing the native context override.
export const aliases = { opus: 'claudex/deep', sonnet: 'claudex/balanced', haiku: 'claudex/fast' };
export const openaiContextWindow = 800000; // Subscription accepted 805028 input tokens.
export const modelIds = settings => Object.fromEntries(Object.entries(settings.routes).map(([tier, route]) => [tier, `${route.provider}/${route.model}${route.effort ? `@${route.effort}` : ''}`]));

export async function readJson(file, fallback) {
  try { return JSON.parse(await readFile(file, 'utf8')); }
  catch (error) { if (error.code === 'ENOENT') return fallback; throw new Error(`Invalid or unreadable configuration: ${file}`); }
}

export async function loadSettings() {
  const config = await readJson(settingsPath, {});
  const cache = await readJson(join(codexHome, 'models_cache.json'), {});
  const catalog = cache.models || [];
  const names = catalog.map(model => model.slug);
  const pick = choices => choices.find(name => names.includes(name));
  const main = pick(['gpt-6.1-sol', 'gpt-6-sol', 'gpt-5.6-sol', 'gpt-5.5', 'gpt-5.4']);
  const fast = pick(['gpt-6.1-luna', 'gpt-6-luna', 'gpt-5.6-luna', 'gpt-5.4-mini']) || main;
  const routes = {
    opus: { provider: 'openai', model: main, effort: 'high' },
    sonnet: { provider: 'openai', model: main, effort: 'medium' },
    haiku: { provider: 'openai', model: fast, effort: 'low' },
    ...config.routes,
  };
  for (const route of Object.values(routes)) {
    if (!['openai', 'zai'].includes(route.provider) || !route.model || !/^[a-zA-Z0-9._-]+$/.test(route.model)) throw new Error('A route needs a supported provider and an explicit model. Run claudex doctor.');
    if (route.effort && !['none', 'minimal', 'low', 'medium', 'high', 'xhigh'].includes(route.effort)) throw new Error('Invalid reasoning effort');
  }
  if (config.zaiFallback && !/^[a-zA-Z0-9._-]+$/.test(config.zaiFallback)) throw new Error('Invalid Z.ai fallback model');
  return { ...config, routes, catalog, openaiFast: fast };
}

export function resolveRoute(model, settings) {
  model = model.replace(/\[1m\]$/i, '');
  const tier = /(^|[-/])fable([-/]|$)/i.test(model) || model === 'best' ? 'opus' : Object.keys(aliases).find(key => model === key || model === aliases[key] || new RegExp(`(^|[-/])${key}([-/]|$)`, 'i').test(model));
  if (tier) return { ...settings.routes[tier], tier };
  const explicit = /^(?:claude-claudex-)?(openai|zai)\/(\w[\w.-]*)(?:@(none|minimal|low|medium|high|xhigh))?$/.exec(model);
  if (explicit) return { provider: explicit[1], model: explicit[2], effort: explicit[3] || 'medium', tier: 'explicit' };
  if (settings.catalog.some(item => item.slug === model)) return { provider: 'openai', model, effort: 'medium', tier: 'explicit' };
  throw Object.assign(new Error('Unknown model alias; use opus, sonnet, haiku, openai/<model> or zai/<model>.'), { status: 400 });
}
