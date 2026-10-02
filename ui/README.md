# `ui/` — the Ragondin front end

The browser application `ragondin ui` serves. Read [`ARCHITECTURE.md`](ARCHITECTURE.md) before changing anything here, and [`DEPENDENCIES.md`](DEPENDENCIES.md) before adding a package.

## Install

Use the Node major pinned in [`.node-version`](.node-version) (any version manager that reads that file, `nvm`, `fnm` and `volta` among them, picks it up). `npm ci` refuses any other major.

```bash
cd ui
npm ci          # never `npm install`: the lockfile is the pin
```

`npm install <package>` is how a dependency is added, and only then; the rule for doing so is in `DEPENDENCIES.md`.

## Run the dev server

```bash
npm run dev     # http://localhost:5173
```

The dev server serves the UI alone, with no API behind it: the shell renders, and the workspace shows as unreachable. The UI talks to the API of the `ragondin ui` binary that serves it. To see it served by the binary, beside its API, build it and then the binary: `npm run build`, then, from the repository root, `cargo run -p ragondin --features ui -- ui --workspace <dir>`.

The design system's preview — every primitive in every state, in both themes — is served by the same dev server at <http://localhost:5173/design/preview/>. It never ships: the production build leaves it out.

## Change a token

Tokens are written in [`design/tokens.json`](design/tokens.json) only. After changing it:

```bash
npm run tokens  # regenerates design/tokens.css; a test fails until you do
```

## Regenerate the API types

`src/api/types.ts` is generated from the API's description, `runtime/ragondin-api/api/v1.json`, and never edited by hand. After the description changes (`just gen-api-description`), from the repository root:

```bash
just gen-ui-types   # regenerates src/api/types.ts; `npm run check` fails until you do
```

## Run the gates

```bash
npm run check   # API types, lint, typecheck, test, build, notices, audit — what CI runs
```

Or one at a time: `npm run types:check`, `npm run lint`, `npm run typecheck`, `npm run test`, `npm run build`, `npm run notices` (after a build), `npm run audit`. From the repository root, `just build-ui` runs `npm ci` and `npm run build`, and `just check-ui` runs it and then every other step of `npm run check`; `just check` runs `build-ui` before the Rust tests that embed `dist/`, and `check-ui` at the end, so it builds once. The audit queries the npm registry, so it needs the network.
