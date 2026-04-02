// Session management — binds API connections to identities.
//
// When a client connects via Unix socket, the engine extracts
// the peer's PID/UID via UCred and maps it to a 0-bytes identity.
//
// Sessions are tracked in the filesystem at `sessions/~{id}/`.

// TODO: Phase 7 — session implementation
