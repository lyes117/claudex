// Profiles request native ceilings; managed policies and project config still apply.
// No tool is exposed/dispatched; this is not a guarantee against MCP or managed-hook startup.
export function executionProfile(value = 'inherit') {
  if (value !== 'inherit' && value !== 'text-only') throw new Error('Invalid workflow execution profile; expected inherit or text-only');
  return value;
}

export function profileIdentity(value) {
  const profile = executionProfile(value);
  // Preserve serialization and hashes of all existing inherit checkpoints.
  return profile === 'inherit' ? {} : { executionProfile: profile };
}

export function childExecutionProfile(value, environment = process.env) {
  if (executionProfile(value) === 'inherit') return { args: [], spawnOptions: {} };
  const env = { ...environment };
  // Windows environment names are case insensitive. Never mutate the parent.
  for (const key of Object.keys(env)) if (['OPENAI_API_KEY', 'CODEX_API_KEY'].includes(key.toUpperCase())) delete env[key];
  return {
    args: ['--ignore-user-config', '--ephemeral', '-s', 'read-only',
      '-c', 'model_provider="openai"',
      '-c', 'tools.enabled=false', '-c', 'features.hooks=false',
      '-c', 'features.plugins=false', '-c', 'features.apps=false',
      '-c', 'mcp_servers={}'],
    spawnOptions: { env },
  };
}
