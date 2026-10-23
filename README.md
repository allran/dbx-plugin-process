# Process Manager

Browse, search, and terminate local and remote OS processes inside DBX.

## Features

- Local process list (`sysinfo`) sorted by memory, with ports / threads / icons
- Remote SSH hosts via connection provider (password or private key)
- Search by name / PID / port
- Kill with in-modal confirmation (`SIGTERM`)
- Protected system-process guard (local + remote)
- Command palette / app toolbar entry for the local workbench

## Naming

| Item | Value |
| --- | --- |
| Repo / folder | `dbx-plugin-process` |
| Plugin id | `io.dbx.process` |
| Publisher | `alan` |
| Sidecar binary | `dbx-plugin-process` |
| Workbench | `io.dbx.process.workbench` |
| Connection | `io.dbx.process.connection` |

## Develop

Requires Node.js 22+ (see `.nvmrc`) and a Rust toolchain:

```bash
nvm use
npm install
dbx-plugin dev --path . --port 5190
```

UI source lives in `src/*.svelte` and builds into `ui/` via Vite.

## Package

```bash
npm run build
dbx-plugin package .
```

Produces an unsigned platform-specific `.dbxp` under `dist/`.

## Remote SSH

- Dial `runtime.host` / `runtime.port` (DBX tunnel endpoint). Falls back to connection host/port when `runtime` is absent (e.g. some test flows).
- Auth: password and/or OpenSSH/PEM private key (`private_key_path` / `private_key` + optional passphrase).
- Supported remote OS: Linux and macOS. Process list uses `ps`; ports use `ss`/`lsof`.
- Icons and “show in folder” are local-only.

## RPC

| Method | Scope | Notes |
| --- | --- | --- |
| `process/summary` | local / remote | Memory totals |
| `process/list` | local / remote | `{ query?, limit? }` — name / PID / port |
| `process/related` | local / remote | Parent + children |
| `process/kill` | local / remote | `{ pid, signal? }` — `TERM` / `KILL` / `INT` |
| `process/icon` | local | Lazy icon data URL |
| `process/reveal-path` | local | Open containing folder |
| `connection/test\|connect\|disconnect` | remote | Real SSH handshake |
