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

There is no API to talk to yet; the page is a placeholder.

## Run the gates

```bash
npm run check   # lint, typecheck, test, build, audit — what CI runs
```

Or one at a time: `npm run lint`, `npm run typecheck`, `npm run test`, `npm run build`, `npm run audit`. From the repository root, `just check-ui` runs `npm ci` and then `npm run check`, and `just check` includes it. The audit queries the npm registry, so it needs the network.
