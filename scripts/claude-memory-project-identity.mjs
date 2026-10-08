// Claudex checkout identity v1. Moving a root creates a new identity; this
// resolver never migrates rows or reads remotes to guess a shared repository.
import { createHash } from 'node:crypto';
import { realpathSync, statSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { homedir, tmpdir } from 'node:os';
import path from 'node:path';

export function normalizeIdentityPath(value, platform = process.platform) {
  if (platform !== 'win32') return path.posix.normalize(value);
  let normalized = path.win32.normalize(value).replaceAll('\\', '/');
  if (/^\/\/\?\/UNC\//i.test(normalized)) normalized = '//' + normalized.slice(8);
  else if (normalized.startsWith('//?/')) normalized = normalized.slice(4);
  return normalized.toLowerCase();
}

export function projectKeyForRoot(root, platform = process.platform) {
  try {
    if (typeof root !== 'string' || !root.trim() || !statSync(root).isDirectory()) throw new Error();
    const physical = realpathSync.native(root);
    const canonical = normalizeIdentityPath(physical, platform);
    const digest = createHash('sha256').update('claudex-project-v1\0').update(canonical).digest('hex');
    return `cx1-${digest}`;
  } catch {
    // Do not include the invalid input or filesystem error in observer output.
    throw new Error('Claudex memory requires an existing project directory');
  }
}

export function getProjectIdentity(cwd, helpers, depth = 0) {
  const { platform = process.platform, findGitRepoRoot, findMarkerProjectRoot, detectWorktree } = helpers;
  // Validate before invoking Git/settings/helpers, and never use a shared
  // unknown-project bucket for missing, nonexistent or non-directory inputs.
  projectKeyForRoot(cwd, platform);
  if (depth >= 32) throw new Error('Claudex memory project ancestry cannot be resolved');
  const physicalCwd = realpathSync.native(cwd);
  const repoRoot = findGitRepoRoot(physicalCwd);
  const root = repoRoot ?? findMarkerProjectRoot(physicalCwd) ?? physicalCwd;
  const checkout = projectKeyForRoot(root, platform);
  const info = repoRoot ? detectWorktree(repoRoot) : null;
  if ((info?.isWorktree || info?.isSubmodule) && info.parentRepoPath) {
    const parent = getProjectIdentity(info.parentRepoPath, helpers, depth + 1).primary;
    const primary = `${parent}/${checkout}`;
    return { primary, parent, isWorktree: info.isWorktree, isSubmodule: info.isSubmodule,
      allProjects: [parent, primary], keySource: 'path' };
  }
  return { primary: checkout, parent: null, isWorktree: false, isSubmodule: false,
    allProjects: [checkout], keySource: 'path' };
}

function gitQuery(cwd, args) {
  try {
    return execFileSync('git', ['-C', cwd, ...args], {
      encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'], windowsHide: true,
      timeout: 5000, maxBuffer: 1024 * 1024,
    }).trim() || null;
  } catch { return null; }
}

function physicalOrResolved(dir) {
  try { return realpathSync.native(dir); } catch { return path.resolve(dir); }
}

function isWithin(child, parent) {
  const relative = path.relative(parent, child);
  return relative === '' || (relative !== '..' && !relative.startsWith('..' + path.sep) && !path.isAbsolute(relative));
}

// Pure Node adapter shared by the bundle and local CLI. No settings, logger,
// plugin configuration or database modules are imported by this entry point.
export function resolveProjectIdentity(cwd, {
  platform = process.platform, home = homedir(), temporary = tmpdir(),
  configDir = process.env.CLAUDE_CONFIG_DIR || path.join(home, '.claude'),
} = {}) {
  if (typeof cwd !== 'string' || !cwd.trim()) throw new Error('Claudex memory requires an existing project directory');
  const expanded = cwd === '~' ? home
    : cwd.startsWith('~/') || (platform === 'win32' && cwd.startsWith('~\\')) ? path.join(home, cwd.slice(2)) : cwd;
  const stopRoots = [home, temporary, configDir].map(physicalOrResolved);
  const configRoot = physicalOrResolved(configDir);
  const helpers = {
    platform,
    findGitRepoRoot: dir => gitQuery(dir, ['rev-parse', '--show-toplevel']),
    findMarkerProjectRoot(dir) {
      if (isWithin(dir, configRoot)) return null;
      for (let depth = 0; depth < 64; depth++) {
        if (stopRoots.includes(dir)) return null;
        const parent = path.dirname(dir);
        if (parent === dir) return null;
        if (['.claude-mem-project', '.claude-mem.json'].some(marker => existsSync(path.join(dir, marker)))) return dir;
        dir = parent;
      }
      return null;
    },
    detectWorktree(dir) {
      // Git resolves this for submodules inside linked worktrees too, unlike
      // parsing only the legacy <main>/.git/modules/<name> filesystem layout.
      const superproject = gitQuery(dir, ['rev-parse', '--show-superproject-working-tree']);
      if (superproject) return { isWorktree: false, isSubmodule: true, parentRepoPath: superproject };
      const gitDir = gitQuery(dir, ['rev-parse', '--absolute-git-dir']);
      const commonDir = gitQuery(dir, ['rev-parse', '--path-format=absolute', '--git-common-dir']);
      const isWorktree = Boolean(gitDir && commonDir && gitDir !== commonDir);
      const parentRepoPath = isWorktree
        ? gitQuery(dir, ['worktree', 'list', '--porcelain'])?.match(/^worktree (.+)$/m)?.[1] : null;
      if (isWorktree && !parentRepoPath) throw new Error('Claudex memory project ancestry cannot be resolved');
      return { isWorktree, isSubmodule: false, parentRepoPath };
    },
  };
  return getProjectIdentity(expanded, helpers);
}
