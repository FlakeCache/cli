# FlakeCache alignment — `cli`

**Role:** the FlakeCache **client** (`flakecache` binary) — OAuth login, push/pull/warm
against the cache, daemon mode, CI helper. Rust; depends on the `chunker` crate for
FastCDC + Ed25519. Does **client-side (zero-knowledge) signing** — the pusher holds
the secret key. `API_MAPPING.md` is the authoritative client↔server wire contract.

**SE (`REPO.md`) placement:** **`products/flakecache/`** (client surface of the
product). Distribution siblings: `crate-index` (Cargo sparse index for chunker),
`releases` (binary hosting), `nix-installer` (CI on-ramp action).

**Keep aligned:** `API_MAPPING.md` must track `server`'s protocol + `chunker`'s wire
format; the client-side signing here is the zero-knowledge counterpart to server's
`signing_public_key`-only storage.

## Shared FlakeCache architecture (all repos)
- Current production cache: **`flakecache/server`** (Elixir) on K8s + CNPG + Garage + StorageBox; its `docs/architecture/{CURRENT,PHASES}.md` are the SoT.
- Trust: per-cache Ed25519 keys, zero-knowledge signing (pusher holds the secret) — this CLI is the pusher.
- No Raft; central authority = CNPG. Deploy: K8s + Flux (`/srv/infra` → `deployment/gitops/`); Fly.io retired.
- **Master cross-repo alignment:** `flakecache/chunker` → `docs/plans/2026-07-14-flakecache-repos-alignment.md`.
