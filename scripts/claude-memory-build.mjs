// Build a source-pinned Apache-2.0 claude-mem edition, without invoking its installer.
import { existsSync, readFileSync, writeFileSync, mkdirSync, cpSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
import { REVISION, VERSION, IDENTITY_SCHEME, sha256 } from './claude-memory-runtime.mjs';

export function patchObserverSource(text) {
  const patches = [
    ["    project_doc_max_bytes: 0,", "    project_doc_max_bytes: 0,\n    'tools.enabled': false,\n    'hooks.enabled': false,"],
    ["  if (scopedCodexHome) result.CODEX_HOME = scopedCodexHome;", `  if (scopedCodexHome) {
    result.CODEX_HOME = scopedCodexHome;
    // Isolate only the observer app server. Its native auth path was resolved
    // before creating the private runtime; no user Claude config is inherited.
    const privateUserHome = join(scopedCodexHome, '..', 'user-home');
    result.HOME = privateUserHome;
    result.USERPROFILE = privateUserHome;
    delete result.HOMEDRIVE;
    delete result.HOMEPATH;
    delete result.XDG_CONFIG_HOME;
    delete result.XDG_DATA_HOME;
    delete result.XDG_CACHE_HOME;
  }`],
    ["throw codexSetupError(`Cannot read Codex ChatGPT auth: ${error instanceof Error ? error.message : String(error)}`);", "throw codexSetupError('Cannot read Codex ChatGPT auth JSON');"],
  ];
  for (const [before, after] of patches) {
    if (text.split(before).length !== 2) throw new Error('Pinned observer source patch did not match exactly once');
    text = text.replace(before, after);
  }
  return text;
}

function replaceExact(text, before, after) {
  if (text.split(before).length !== 2) throw new Error('Pinned identity source patch did not match exactly once');
  return text.replace(before, after);
}

export function patchPortReclaimSource(text) {
  text = replaceExact(text, "        | 'out-of-budget'; // a caller's hook deadline leaves too little to finish (RECLAIM_MIN_BUDGET_MS)",
    "        | 'claudex-reclaim-disabled' // Claudex never reclaims a port owner\n        | 'out-of-budget'; // a caller's hook deadline leaves too little to finish (RECLAIM_MIN_BUDGET_MS)");
  const signature = `export async function reclaimGhostListeningPort(
  port: number,
  deps: GhostPortReclaimDeps = {}
): Promise<GhostPortReclaimResult> {`;
  if (text.split(signature).length !== 2) throw new Error('Pinned port reclaim signature changed');
  const from = text.indexOf(signature);
  // At the pinned revision this is the last declaration in the file. Refuse
  // any new declaration rather than silently deleting another public API.
  const tail = text.slice(from + signature.length);
  if (/^export |^(?:async )?function /m.test(tail) || !tail.trimEnd().endsWith('}')) {
    throw new Error('Pinned port reclaim boundary changed');
  }
  return text.slice(0, from) + `// Claudex owns only children it explicitly started. An occupied port is
// refused; no PID discovery, ghost cleanup or foreign process termination.
export async function reclaimGhostListeningPort(
  _port: number,
  _deps: GhostPortReclaimDeps = {}
): Promise<GhostPortReclaimResult> {
  return { reclaimed: false, reason: 'claudex-reclaim-disabled', killedPids: [] };
}
`;
}

export function patchProjectIdentitySource(text) {
  if (text.includes('./claudex-project-identity.js')) throw new Error('Project identity source is already patched');
  text = replaceExact(text, "import path from 'path';", "import path from 'path';\nimport { resolveProjectIdentity } from './claudex-project-identity.js';");
  const patches = [
    ['export function getProjectName(', '\n/**\n * How a project key was derived:', `export function getProjectName(cwd: string | null | undefined, platform: NodeJS.Platform = process.platform): string {
  return getProjectContext(cwd, platform).primary;
}
`],
    ['export function getProjectContext(', '\n/** Re-key a context to', `export function getProjectContext(cwd: string | null | undefined, platform: NodeJS.Platform = process.platform): ProjectContext {
  if (typeof cwd !== 'string' || !cwd.trim()) throw new Error('Claudex memory requires an existing project directory');
  return resolveProjectIdentity(cwd, { platform });
}
`],
    ['export function getPathModeProjectContext(', '\n/** Path-mode identity:', `export function getPathModeProjectContext(cwd: string, platform: NodeJS.Platform = process.platform): ProjectContext {
  return getProjectContext(cwd, platform);
}
`],
  ];
  for (const [start, end, replacement] of patches) {
    if (text.split(start).length !== 2 || text.split(end).length !== 2) throw new Error('Pinned identity function boundary changed');
    const from = text.indexOf(start), to = text.indexOf(end, from);
    if (to < from) throw new Error('Pinned identity function boundaries are reversed');
    text = text.slice(0, from) + replacement + text.slice(to);
  }
  return text;
}

export function patchProjectRemapSource(text) {
  text = replaceExact(text, "import { buildWorktreeProjectKey } from '../../utils/project-name.js';",
    "import { getPathModeProjectContext } from '../../utils/project-name.js';");
  const start = 'function classifyCwdForRemap(cwd: string): CwdClassification {';
  const end = '\nexport function runOneTimeCwdRemap(';
  if (text.split(start).length !== 2 || text.split(end).length !== 2) throw new Error('Pinned remap function boundary changed');
  const from = text.indexOf(start), to = text.indexOf(end, from);
  if (to < from) throw new Error('Pinned remap function boundaries are reversed');
  return text.slice(0, from) + `function classifyCwdForRemap(cwd: string): CwdClassification {
  try {
    const context = getPathModeProjectContext(cwd);
    return { kind: context.isWorktree ? 'worktree' : 'main', project: context.primary };
  } catch {
    return { kind: 'skip' };
  }
}
` + text.slice(to);
}

const identityTypes = `export interface IdentityContext {
  primary: string; parent: string | null; isWorktree: boolean; isSubmodule: boolean;
  allProjects: string[]; keySource: 'path';
}
export function getProjectIdentity(cwd: string, helpers: {
  platform: NodeJS.Platform;
  findGitRepoRoot(cwd: string): string | null;
  findMarkerProjectRoot(cwd: string): string | null;
  detectWorktree(cwd: string): { isWorktree: boolean; isSubmodule: boolean; parentRepoPath: string | null };
}): IdentityContext;
export function normalizeIdentityPath(value: string, platform?: NodeJS.Platform): string;
export function projectKeyForRoot(root: string, platform?: NodeJS.Platform): string;
export function resolveProjectIdentity(cwd: string, options?: {
  platform?: NodeJS.Platform; home?: string; temporary?: string; configDir?: string;
}): IdentityContext;
`;

export async function buildMemoryPackage({ source, output, bun = 'bun' }) {
  source = resolve(source); output = resolve(output);
  const revision = execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  if (revision !== REVISION) throw new Error('Unexpected upstream source revision');
  if (existsSync(output)) throw new Error('Build output exists; select a new owned directory');
  mkdirSync(output, { recursive: true });
  const sourceCopy = join(output, 'source');
  mkdirSync(sourceCopy);
  const archive = join(output, 'public-source.tar');
  // Only Git-tracked public upstream source enters the build, never profile files.
  execFileSync('git', ['-C', source, 'archive', '--format=tar', `--output=${archive}`, REVISION]);
  const entries = execFileSync('tar', ['-tf', archive], { encoding: 'utf8' }).split(/\r?\n/).filter(Boolean);
  if (entries.some(entry => /^[/\\]|^[A-Za-z]:/.test(entry) || entry.split(/[\\/]/).includes('..'))) {
    throw new Error('Public source archive escapes its build directory');
  }
  // Git stores optional skill aliases as symlinks; Windows extraction cannot
  // create these without privilege. They are not part of the runtime build.
  const symlinks = execFileSync('git', ['-C', source, 'ls-tree', '-r', REVISION], { encoding: 'utf8' })
    .split(/\r?\n/).filter(line => line.startsWith('120000 ')).map(line => line.split('\t')[1]);
  execFileSync('tar', [...symlinks.map(path => `--exclude=${path}`), '-xf', archive, '-C', sourceCopy]);
  const observer = join(sourceCopy, 'src/services/worker/CodexAppServerClient.ts');
  writeFileSync(observer, patchObserverSource(readFileSync(observer, 'utf8')));
  const portReclaim = join(sourceCopy, 'src/shared/port-reclaim.ts');
  writeFileSync(portReclaim, patchPortReclaimSource(readFileSync(portReclaim, 'utf8')));
  const projectNames = join(sourceCopy, 'src/utils/project-name.ts');
  const projectRemap = join(sourceCopy, 'src/services/infrastructure/ProcessManager.ts');
  const projectIdentity = join(sourceCopy, 'src/utils/claudex-project-identity.js');
  const projectIdentityTypes = join(sourceCopy, 'src/utils/claudex-project-identity.d.ts');
  writeFileSync(projectNames, patchProjectIdentitySource(readFileSync(projectNames, 'utf8')));
  writeFileSync(projectRemap, patchProjectRemapSource(readFileSync(projectRemap, 'utf8')));
  cpSync(join(dirname(fileURLToPath(import.meta.url)), 'claude-memory-project-identity.mjs'), projectIdentity);
  writeFileSync(projectIdentityTypes, identityTypes);
  const packagePath = join(sourceCopy, 'package.json');
  const metadata = JSON.parse(readFileSync(packagePath, 'utf8'));
  metadata.version = VERSION;
  writeFileSync(packagePath, JSON.stringify(metadata, null, 2) + '\n');
  const run = (command, args, cwd) => execFileSync(command, args, {
    cwd, stdio: 'inherit', windowsHide: true, shell: false,
  });
  // Upstream publishes only a plugin lock, not a root lock. Generate the root
  // build lock without lifecycle scripts, then require it on the second pass.
  run(bun, ['install', '--ignore-scripts'], sourceCopy);
  run(bun, ['install', '--frozen-lockfile', '--ignore-scripts'], sourceCopy);
  run(process.execPath, ['scripts/build-hooks.js'], sourceCopy);
  run(bun, ['install', '--frozen-lockfile', '--ignore-scripts'], join(sourceCopy, 'plugin'));
  const packageRoot = join(output, 'package');
  mkdirSync(packageRoot);
  cpSync(join(sourceCopy, 'plugin'), join(packageRoot, 'plugin'), { recursive: true });
  const packagedIdentity = join(packageRoot, 'plugin/scripts/claude-memory-project-identity.mjs');
  cpSync(projectIdentity, packagedIdentity);
  cpSync(join(sourceCopy, 'LICENSE'), join(packageRoot, 'LICENSE'));
  const hashes = {};
  for (const file of ['plugin/scripts/worker-service.cjs', 'plugin/scripts/mcp-server.cjs',
    'plugin/scripts/claude-memory-project-identity.mjs']) hashes[file] = sha256(join(packageRoot, file));
  const receipt = { version: VERSION, revision, toolsEnabled: false, privateUserHome: true,
    portReclaimDisabled: true, portReclaimPatch: sha256(portReclaim),
    sourcePatch: sha256(observer), rootLock: sha256(join(sourceCopy, 'bun.lock')),
    projectIdentity: { scheme: IDENTITY_SCHEME, relocation: 'new-identity',
      relatedCheckouts: 'parent-and-checkout-only', source: sha256(projectIdentity),
      types: sha256(projectIdentityTypes), resolver: sha256(projectNames), remap: sha256(projectRemap) },
    pluginLock: sha256(join(sourceCopy, 'plugin/bun.lock')), sha256: hashes,
    edition: 'local patched source build; not published claude-mem 13.28.0' };
  writeFileSync(join(packageRoot, 'claudex-observer-build.json'), JSON.stringify(receipt, null, 2) + '\n');
  return { packageRoot, version: VERSION, revision };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [source, output] = process.argv.slice(2);
  if (!source || !output) throw new Error('Usage: node claude-memory-build.mjs UPSTREAM_CHECKOUT NEW_OUTPUT');
  console.log(JSON.stringify(await buildMemoryPackage({ source, output })));
}
