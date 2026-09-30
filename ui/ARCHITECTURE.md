# `ui/` — architecture

The front end: a TypeScript application, built with Vite and React, that the `ragondin ui` subcommand serves (ADR-C36). It is **not a crate** and sits outside Cargo, but it is **load-bearing** in the sense `AGENTS.md` § Documentation ships with the code it describes means: this file is read before `ui/` is modified, and a diff that falsifies it corrects it in the same pull request (ADR-C36 § 5).

Today it is the governed, empty application: one placeholder page, every gate green, every rule written. No screen, no design token and no API call exist yet.

## What lives here

```
ui/
├── .node-version        # the one pinned Node major; CI reads it, `engines` mirrors it
├── .npmrc               # engine-strict: `npm ci` refuses any other Node major
├── package.json         # scripts, runtime and development dependencies
├── package-lock.json    # committed; installed with `npm ci` only
├── DEPENDENCIES.md      # every dependency, with its role (and, if runtime, its reason)
├── index.html           # the single entry
├── eslint.config.js     # lint, including the network-confinement rule
├── vite.config.ts       # build and test runner configuration
├── tsconfig*.json       # strict TypeScript: app code (DOM) and tooling (Node) apart
├── src/
│   ├── main.tsx         # mounts the application
│   ├── App.tsx          # the placeholder page
│   └── api/             # the only module that may touch the network
│       └── base.ts      # the API's base address, '/api/v1'
├── scripts/             # the dependency audit: policy, licence check, advisory check
└── tests/               # tests of the governance itself: lint rule, audit, DEPENDENCIES.md
```

A component's test sits beside it in `src/`; a test of a rule about `ui/` sits in `tests/`. `tests/setup.ts` unmounts what each test rendered: Vitest's globals are off, so Testing Library cannot register that cleanup itself.

## The gates

`npm run check` is the one command, and CI's `ui` job, `just check-ui` and a contributor all run it. It runs, cheapest first:

| Script | What it does |
|---|---|
| `lint` | ESLint over everything, warnings are errors. |
| `typecheck` | `tsc -b` over the application and the tooling, both strict. |
| `test` | Vitest: component tests in a DOM, governance tests in Node. |
| `build` | `vite build` into `dist/`. |
| `audit` | The licence audit, then the advisory audit (§ The dependency audit). |

`just check-ui` runs `npm ci` first, so the gate always installs exactly the lockfile. **The Rust build does not need Node** (ADR-C36 § 5): no cargo command and no cargo-based `just` recipe reads anything under `ui/`. Only `just check`, which covers both worlds, does.

## The one-address rule

The UI talks to one address: the origin that served it. Every request goes to the binary's JSON API under a **relative** base address, `API_BASE` in `src/api/base.ts`, and every asset — fonts included — is bundled and served by the binary, never fetched from a CDN or any other host (ADR-C36 § 5, applying ADR-012 on the browser side). `src/api/base.test.ts` asserts that the base address resolves against whatever origin served the page.

`ui/` is the **only consumer of `/api/v1`** (ADR-C36 § 2). That is what keeps the API internal rather than a stable boundary: it changes with the UI, in the same pull request. A second consumer is a decision, not a change.

## The network lint

The second, best-effort layer of that rule. `eslint.config.js` makes it an error, anywhere outside `src/api/`, to name the browser's network primitives — `fetch`, `XMLHttpRequest`, `WebSocket`, `EventSource` — whether as a bare global (`no-restricted-globals`) or as a property of `window`, `globalThis`, `self`, `top`, `parent`, `frames` or `opener`, dotted or with a literal key (`no-restricted-properties`). Each message names `src/api/` and this section. It covers every source extension `tsc` and Vite accept — `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.mts`, `.cts`, `.tsx` — because a file the lint does not match is a file it silently does not read. `tests/network-lint.test.ts` lints the same code as if it lived outside and inside `src/api/`, against the real configuration, and once per extension.

It is a **scan of source text, and it does not see**:

- a property computed at run time (`window['fe' + 'tch']`), or one reached through an alias or a longer path: `const g = globalThis; g.fetch(...)`, `window.window.fetch`, `document.defaultView.fetch`, `Reflect.get(window, 'fetch')`;
- a worker (`new Worker(url)`), whose own code the lint reads only if it is a source file here, and whose script address is not checked;
- a primitive named by a dependency's own code, which the lint never reads;
- other ways to reach the network: an `<img>` or `<link>` pointing elsewhere, a `navigator.sendBeacon`, a dynamic `import()` of a URL.

A green lint is evidence, not proof. **The layer that holds is the content security policy the binary sends** (ADR-C36 § 1 and § 5), whose default source is `'self'`: the browser itself refuses any other origin, for the UI's code and for every dependency it bundles.

## The dependency audit

`deny.toml` transposed, since the two toolchains cannot share a file. The policy is `scripts/audit-policy.mjs`.

**Licences** (`scripts/check-licenses.mjs`, logic in `scripts/licenses.mjs`). Every package in the lockfile, runtime and development alike, must carry a licence expression satisfiable from `LICENSE_ALLOW`. That list is a **copy of `deny.toml`'s `[licenses] allow`**, duplicated on purpose and kept identical by `tests/licenses.test.ts`, which reads both. An installed package is read from its own `package.json`; a platform-specific package this machine did not install is read from the licence the lockfile records, so every platform's tree is audited, as `deny.toml` audits every target. A missing licence field, a `WITH` exception, `UNLICENSED` or `SEE LICENSE IN …` fails.

`LICENSE_EXCEPTIONS` admits one extra licence for one named package, **only while that package is a development dependency** — the check ignores an exception for anything the bundle ships, so the runtime tree meets `deny.toml`'s list exactly. Each entry carries `package`, `license`, `date` and `reason`, the shape of cargo-deny's `[[licenses.exceptions]]`. The one entry today is `minimatch` (BlueOak-1.0.0, permissive), which ESLint depends on unconditionally. An exception that admits nothing any more — its package gone, or its licence now allowed — is reported, as an unmatched advisory exception is, so it gets deleted.

**Advisories** (`scripts/check-advisories.mjs`, logic in `scripts/advisories.mjs`). Runs `npm audit --audit-level=high --json` and fails on any high or critical advisory that `ADVISORY_EXCEPTIONS` does not name. npm has no ignore list, which is why the report is read rather than npm's exit code trusted; a missing or unrecognised report fails. Every exception carries:

| Field | Content |
|---|---|
| `id` | the advisory's GHSA identifier |
| `date` | the day it was added, `YYYY-MM-DD` |
| `reason` | why it is an allowance rather than a fix |
| `removedBy` | the issue that removes it, as `#<number>` |

— the shape `deny.toml`'s `ignore` comment prescribes. An exception that no longer matches anything is reported, so it gets deleted. The list is empty today. The advisory audit reads the registry's advisory database, which changes daily: like `just check-deny`, it can go red on a branch whose diff caused nothing.

Adding a dependency is governed by `DEPENDENCIES.md`, whose rule is `AGENTS.md` § Conventions'.

## Toolchain

The choices ADR-C36 § 5 left to the implementation, and why each was made:

- **Node 24**, the current LTS major, pinned once in `.node-version`. CI's setup action reads that file; `engines` mirrors it and `.npmrc`'s `engine-strict` makes `npm ci` refuse another major. `tests/node-version.test.ts` fails when the two name different majors.
- **Vite 7, not 8.** Vite 8 depends unconditionally on `lightningcss`, licensed MPL-2.0 — which `deny.toml` leaves off its list deliberately, as file-level copyleft. Moving to Vite 8 is therefore a change to the licence policy of both toolchains, taken in `deny.toml` first, not a version bump.
- **No React plugin.** esbuild compiles JSX with the automatic runtime by itself (`vite.config.ts`). The plugin's contribution is Fast Refresh in the dev server; without it an edit reloads the page. The plugin's line for Vite 7 (5.x) also pulls `@babel/core`, and through it `caniuse-lite`, licensed CC-BY-4.0, off the list.
- **happy-dom, not jsdom**, as the test DOM. jsdom's tree carries MIT-0, CC0-1.0 and BlueOak-1.0.0 packages, and its current release declares `engines` of Node `^24.15.0`, a minor the major pin does not guarantee; happy-dom is MIT with a small tree.
- **TypeScript 6.0.** TypeScript 7 exists, but `typescript-eslint` supports up to 6.0; the lint parses with the same compiler the type check runs.
- **Vitest and Testing Library** for tests, **ESLint** with `typescript-eslint` for the lint, as the front-end design names them.

## How the assets reach the binary

`npm run build` writes `dist/`, which is ignored by git: nothing here commits built assets. Embedding `dist/` into `bin/ragondin` behind its `ui` feature, and what a build does when `dist/` is absent, belong to the `ragondin ui` subcommand's issue (#339) and to ADR-C36 § 1 and § 5; nothing on the Rust side reads `ui/` yet.
