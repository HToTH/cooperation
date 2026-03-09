# cooperation on macOS

Validated on March 9, 2026 with:

- macOS 26.3.1
- Node.js 25.6.0
- npm 11.8.0
- Rust 1.88.0
- Cargo 1.88.0

## Prerequisites

- Xcode Command Line Tools
- Node.js and npm
- Rust toolchain

Install Xcode Command Line Tools if needed:

```bash
xcode-select --install
```

Optional CLIs for non-raw agent modes:

- `claude` for "Claude Code (CLI)"
- `gemini` for "Gemini CLI"
- `codex` for "Codex CLI (OpenAI)"

If you only use the raw API modes, you only need the matching API keys.

## Backend

The backend listens on `http://localhost:8080` by default and stores data in
`agentflow/backend/agentflow.db` unless `DATABASE_URL` is overridden.

Important: the server does not automatically load `.env`. It only reads process
environment variables. The `.env.example` file is a template.

Start the backend in one terminal:

```bash
cd backend
cp -n .env.example .env
set -a
source .env
set +a
cargo run -p agentflow-server
```

## Frontend

The frontend expects the backend at:

- `http://localhost:8080`
- `ws://localhost:8080/ws`

Start the frontend in a second terminal:

```bash
cd frontend
npm install
npm run dev -- --host 127.0.0.1
```

Open:

```text
http://127.0.0.1:5173
```

## Build Checks

Run these from the project directories:

```bash
cd backend
cargo test
```

```bash
cd frontend
npm run build
```

## Quick Smoke Test

After both services are running:

1. Open the frontend in the browser.
2. Confirm the page loads without a blank screen.
3. Click `Load`.
4. Verify the saved workflow list appears.
5. Select a workflow and confirm the canvas updates.

## Troubleshooting

### Frontend fails after copying `node_modules` from another OS

Reinstall dependencies on macOS:

```bash
cd frontend
rm -rf node_modules
npm install
```

This ensures macOS-native optional packages such as Rollup and Esbuild are
installed for the current platform.

### Frontend cannot reach the backend

Check that the backend is running on port `8080`. The frontend currently uses
hard-coded backend URLs in source, so changing the backend port requires code
changes.

### Agent mode fails to start

Check the selected agent mode:

- Raw API modes need the matching API key in the environment.
- CLI modes need the matching executable installed and authenticated on PATH.

## Verified in This Workspace

The following passed on macOS:

- `cargo test`
- `npm run build`
- Frontend dev server startup
- Backend server startup
- Browser load of the frontend
- Workflow list fetch through the frontend
- Workflow load through the frontend
