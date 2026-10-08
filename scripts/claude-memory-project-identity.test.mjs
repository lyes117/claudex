import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, realpathSync, symlinkSync, unlinkSync, renameSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { getProjectIdentity, resolveProjectIdentity, normalizeIdentityPath, projectKeyForRoot } from './claude-memory-project-identity.mjs';
import { patchProjectIdentitySource, patchProjectRemapSource } from './claude-memory-build.mjs';

const git = (cwd, args) => execFileSync('git', ['-C', cwd, ...args], {
  encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
});

function fixture(t) {
  const root = mkdtempSync(path.join(tmpdir(), 'claudex-identity-'));
  const beforeCleanup = [];
  t.after(() => {
    assert.ok(path.resolve(root).startsWith(path.resolve(tmpdir()) + path.sep));
    for (const cleanup of beforeCleanup) cleanup();
    rmSync(root, { recursive: true, force: true });
  });
  const directory = relative => {
    const dir = path.join(root, relative);
    mkdirSync(dir, { recursive: true });
    return dir;
  };
  const repo = relative => {
    const dir = directory(relative);
    git(dir, ['init', '-q']);
    git(dir, ['config', 'user.name', 'Identity Fixture']);
    git(dir, ['config', 'user.email', 'identity@example.invalid']);
    git(dir, ['commit', '--allow-empty', '-qm', 'fixture']);
    return dir;
  };
  return { root, directory, repo, beforeCleanup };
}

// Every Git fixture uses the same pure Node adapter shipped in the bundle.
const identity = resolveProjectIdentity;

test('homonymous repositories stay isolated even when their remotes are identical', t => {
  const f = fixture(t), a = f.repo('one/shared'), b = f.repo('two/shared');
  for (const cwd of [a, b]) git(cwd, ['remote', 'add', 'origin', 'https://example.invalid/org/shared.git']);
  const before = identity(a), other = identity(b);
  assert.notEqual(before.primary, other.primary);
  assert.deepEqual(before.allProjects, [before.primary]);
  assert.deepEqual(other.allProjects, [other.primary]);
  assert.match(before.primary, /^cx1-[a-f0-9]{64}$/);
  git(a, ['remote', 'set-url', 'origin', 'https://example.invalid/different/remote.git']);
  assert.deepEqual(identity(a), before);
  assert.doesNotMatch(JSON.stringify(before), /shared|example\.invalid|[/\\]one/);
});

test('Git root identity is stable across nested directories and repeated resolution', t => {
  const f = fixture(t), root = f.repo('repo'), nested = f.directory('repo/a/b');
  assert.deepEqual(identity(nested), identity(root));
  assert.deepEqual(identity(path.join(root, '.')), identity(root));
});

test('marked non-Git projects share only their canonical marker root', t => {
  const f = fixture(t), root = f.directory('marked'), nested = f.directory('marked/child');
  writeFileSync(path.join(root, '.claude-mem-project'), 'fixture');
  assert.deepEqual(identity(root), identity(nested));
  const other = f.directory('other/marked');
  assert.notEqual(identity(root).primary, identity(other).primary);
  assert.equal(identity(other).primary, projectKeyForRoot(other));
});

test('worktrees with identical leaf names retain distinct checkout keys and one proven parent', t => {
  const f = fixture(t), parent = f.repo('main/repo');
  const a = path.join(f.root, 'a/repo'), b = path.join(f.root, 'b/repo');
  mkdirSync(path.dirname(a), { recursive: true });
  mkdirSync(path.dirname(b), { recursive: true });
  git(parent, ['worktree', 'add', '-q', '-b', 'fixture-a', a]);
  git(parent, ['worktree', 'add', '-q', '-b', 'fixture-b', b]);
  const parentKey = identity(parent).primary, first = identity(a), second = identity(b);
  assert.notEqual(first.primary, second.primary);
  for (const context of [first, second]) {
    assert.equal(context.parent, parentKey);
    assert.equal(context.isWorktree, true);
    assert.equal(context.isSubmodule, false);
    assert.deepEqual(context.allProjects, [parentKey, context.primary]);
    assert.match(context.primary, /^cx1-[a-f0-9]{64}\/cx1-[a-f0-9]{64}$/);
  }
  mkdirSync(path.join(a, 'nested'));
  assert.deepEqual(identity(path.join(a, 'nested')), first);
});

test('two local Git submodules with one source stay distinct under their parent', t => {
  const f = fixture(t), parent = f.repo('parent'), source = f.repo('source');
  git(parent, ['-c', 'protocol.file.allow=always', 'submodule', 'add', '-q', source, 'a/shared']);
  git(parent, ['-c', 'protocol.file.allow=always', 'submodule', 'add', '-q', source, 'b/shared']);
  const a = identity(path.join(parent, 'a/shared')), b = identity(path.join(parent, 'b/shared'));
  assert.notEqual(a.primary, b.primary);
  for (const context of [a, b]) {
    assert.equal(context.parent, identity(parent).primary);
    assert.equal(context.isWorktree, false);
    assert.equal(context.isSubmodule, true);
    assert.deepEqual(context.allProjects, [context.parent, context.primary]);
  }
});

test('existing physical directory aliases resolve to the same key', t => {
  const f = fixture(t), root = f.repo('physical'), alias = path.join(f.root, 'alias');
  try { symlinkSync(root, alias, process.platform === 'win32' ? 'junction' : 'dir'); }
  catch (error) {
    if (['EPERM', 'EACCES', 'ENOSYS'].includes(error.code)) { t.skip('Directory alias creation unavailable'); return; }
    throw error;
  }
  f.beforeCleanup.push(() => unlinkSync(alias));
  assert.equal(realpathSync.native(root), realpathSync.native(alias));
  assert.deepEqual(identity(alias), identity(root));
});

test('non-Git nested junction uses the physical marker ancestry rather than alias ancestry', t => {
  const f = fixture(t), root = f.directory('physical'), nested = f.directory('physical/child');
  writeFileSync(path.join(root, '.claude-mem-project'), 'fixture');
  const alias = path.join(f.directory('outside'), 'alias');
  try { symlinkSync(nested, alias, process.platform === 'win32' ? 'junction' : 'dir'); }
  catch (error) {
    if (['EPERM', 'EACCES', 'ENOSYS'].includes(error.code)) { t.skip('Directory alias creation unavailable'); return; }
    throw error;
  }
  f.beforeCleanup.push(() => unlinkSync(alias));
  assert.deepEqual(identity(alias), identity(nested));
  assert.deepEqual(identity(alias), identity(root));
});

test('submodule inside a linked worktree preserves its proven composite parent', t => {
  const f = fixture(t), main = f.repo('main'), source = f.repo('source');
  git(main, ['-c', 'protocol.file.allow=always', 'submodule', 'add', '-q', source, 'nested/shared']);
  git(main, ['commit', '-qam', 'submodule fixture']);
  const worktree = path.join(f.root, 'worktree');
  git(main, ['worktree', 'add', '-q', '-b', 'fixture-submodule', worktree]);
  git(worktree, ['-c', 'protocol.file.allow=always', 'submodule', 'update', '--init', '-q']);
  const parent = identity(worktree), child = identity(path.join(worktree, 'nested/shared'));
  assert.equal(child.parent, parent.primary);
  assert.equal(parent.parent, identity(main).primary);
  assert.equal(child.isSubmodule, true);
  assert.deepEqual(child.allProjects, [parent.primary, child.primary]);
  assert.match(child.primary, /^cx1-[a-f0-9]{64}\/cx1-[a-f0-9]{64}\/cx1-[a-f0-9]{64}$/);
});

test('Windows normalization equates case, separators and extended path syntax', () => {
  assert.equal(normalizeIdentityPath('C:\\Work\\Repo', 'win32'), 'c:/work/repo');
  assert.equal(normalizeIdentityPath('c:/WORK/Repo/.', 'win32'), 'c:/work/repo');
  assert.equal(normalizeIdentityPath('\\\\?\\C:\\Work\\Repo', 'win32'), 'c:/work/repo');
  assert.equal(normalizeIdentityPath('\\\\?\\UNC\\Server\\Share\\Repo', 'win32'), '//server/share/repo');
  assert.notEqual(normalizeIdentityPath('/Work/Repo', 'linux'), normalizeIdentityPath('/work/repo', 'linux'));
});

test('missing or invalid directories fail closed without logging their input', t => {
  const f = fixture(t), file = path.join(f.root, 'not-a-directory');
  writeFileSync(file, 'fixture');
  const forbidden = { findGitRepoRoot() { throw new Error('helper must not execute'); } };
  for (const input of [null, undefined, '', ' ', path.join(f.root, 'missing'), file, 'invalid\0input']) {
    assert.throws(() => getProjectIdentity(input, forbidden), {
      message: 'Claudex memory requires an existing project directory',
    });
  }
});

test('moving a project root creates a new identity rather than an implicit alias', t => {
  const f = fixture(t), a = f.repo('a'), b = path.join(f.root, 'b'), before = identity(a);
  renameSync(a, b);
  const after = identity(b);
  assert.notEqual(before.primary, after.primary);
  assert.equal(after.parent, null);
  assert.deepEqual(after.allProjects, [after.primary]);
});

test('pinned resolver and remap patches delegate to one module and reject unknown boundaries', t => {
  const upstream = process.env.CLAUDEX_MEMORY_IDENTITY_SOURCE
    ?? fileURLToPath(new URL('../.build-tools/cm-build-a1951f2-cycle2/source/', import.meta.url));
  if (!existsSync(path.join(upstream, 'src/utils/project-name.ts'))) {
    t.skip('Pinned public upstream source is unavailable'); return;
  }
  const resolver = readFileSync(path.join(upstream, 'src/utils/project-name.ts'), 'utf8');
  const remap = readFileSync(path.join(upstream, 'src/services/infrastructure/ProcessManager.ts'), 'utf8');
  const patched = patchProjectIdentitySource(resolver);
  assert.match(patched, /return resolveProjectIdentity\(cwd, \{ platform \}\)/);
  assert.equal((patched.match(/return getProjectContext\(cwd, platform\)/g) ?? []).length, 2);
  const patchedRemap = patchProjectRemapSource(remap);
  assert.match(patchedRemap, /project: context\.primary/);
  assert.doesNotMatch(patchedRemap, /buildWorktreeProjectKey|const leaf = path\.basename\(toplevel\)/);
  for (const patch of [patchProjectIdentitySource, patchProjectRemapSource]) {
    assert.throws(() => patch('unknown source'));
    assert.throws(() => patch(patch === patchProjectIdentitySource ? patched : patchedRemap));
  }
});
