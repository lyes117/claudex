# Native Claude-format file tools

Original `Read`, `Glob` and `Grep` implementations, installed by Claudex's
app-server extension registry. They use the invocation's `ExecutorFileSystem`
with its `FileSystemSandboxContext`; they never invoke a shell or fall back to
host filesystem access. Native tool policies, permission gates and hooks still
apply, including when called through CodeMode.

These are intentionally bounded text operations, not full Claude tool parity:

- `Read`: valid UTF-8 regular files up to 1 MiB, 1-based line offset, up to
  2000 requested lines. Binary, PDF, image and oversized files are rejected.
- `Glob`: depth 16, at most 1000 directories and 4000 examined entries;
  directory symlinks are not traversed. Relative paths sort lexically.
- `Grep`: Rust regex syntax, per-line matches, optional filename glob and `-i`,
  content/filename/matching-line-count modes; at most 128 files of 1 MiB each.
  Unreadable, binary and oversized files contribute to `skipped`.
- Successful JSON results fit 8 KiB including JSON escaping; truncation is explicit.
  Backend error messages also fit 8 KiB of UTF-8 text, with an explicit truncation
  marker; that limit applies before protocol-envelope serialization.
  Unsupported arguments are rejected rather than silently ignored.
- Exactly one active host environment is required. A managed read restriction
  without an available filesystem sandbox fails closed.

Native turn items display progress and completion using the existing TUI tool
cards. Completed cards survive app-server restart in both legacy and paginated
history. Explicit client-provided tools with the same name take priority.

Tests use real files, an executor-policy forwarding spy, synthetic permission
denials through app-server and direct/CodeMode response bounds. TUI snapshots
cover widths 26 and 80 and restoration. Mock model tests prove execution
plumbing; real ChatGPT inference is a separate opt-in verification.

Current limitations: cancellation can leave a started item without a persisted
completion; display completion precedes host PostToolUse result filtering.
The CLI exec JSON formatter does not yet expose these native display cards.
No source configuration is migrated or duplicated.
