# Claudex gateway

Keep the existing Rust fork and all user projects unchanged. Launch the installed
Claude Code executable with inherited terminal handles; do not reproduce its UI.
Never log prompts, provider responses, credentials, headers, or account identities.
Subscription endpoints only; never fall back to a metered API key.
Use the pinned converter dependency rather than implementing another protocol.
Run `npm test` after logic changes. Keep live checks synthetic and within this
repository's `artifacts/` directory. Review the diff before delivery.
