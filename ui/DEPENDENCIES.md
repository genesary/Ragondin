# `ui/` dependencies

The `[workspace.dependencies]` rule, transposed to the npm tree (ADR-C36 § 5; `AGENTS.md` § Conventions):

- **Every runtime dependency has a row below, with its role and its reason.** A runtime dependency is one in `package.json`'s `dependencies`: it is bundled into what the binary serves.
- **Adding a runtime dependency is named in the pull request under its own heading**, and its row lands in the same pull request.
- **A runtime dependency that duplicates a role already filled** — a second graph library, router or state store — **or a component kit, escalates** as a duplicated utility role does in `AGENTS.md` § Rules of engagement: stop and open a `decision` issue.
- Development dependencies build, lint and test the application and never reach the bundle. They are listed below by role only.

`tests/dependencies-doc.test.ts` fails when a table here and `package.json` disagree, in either direction. Every package in the tree, runtime or development, passes the licence and advisory audit (`ARCHITECTURE.md` § The dependency audit).

## Runtime dependencies

| Package | Role | Reason |
|---|---|---|
| `react` | The component model and rendering. | Decided by ADR-C36 § 5, and what React Flow, the canvas library that ADR names, is built on. |
| `react-dom` | Renders React into the browser's DOM. | React's own renderer for the web; `react` alone renders nothing. |
| `@xyflow/react` | The canvas library: the graph surface the pipeline canvas draws on — the viewport with pan and zoom, node placement, ports as handles, edges between them, the dot grid, focusable nodes. Only `src/canvas/` imports it (`ARCHITECTURE.md` § The canvas). | Decided by ADR-C36 § 5 (React Flow), for the one canvas ADR-016 makes Replay and the editor share. MIT; its tree is MIT, ISC (the `d3-*` modules) and BSD-3-Clause (`d3-ease`). It brings `zustand` as its internal store, which no code here imports — the lint refuses it everywhere (`ARCHITECTURE.md` § The canvas): the UI holds no state store of its own. |
| `@dagrejs/dagre` | The automatic layout: places a graph left to right, in ranks, when no stored layout names a node. | The layout library the front-end design names for the canvas (§ 6, "automatic layout (dagre)"); the maintained line of `dagre`, ESM with its own types, MIT, one dependency (`@dagrejs/graphlib`, MIT). The other candidate, `elkjs`, is licensed `EPL-2.0 OR GPL-3.0-or-later`, both off the allow list. |

## Development dependencies

| Package | Role |
|---|---|
| `@eslint/js` | ESLint's recommended JavaScript rules. |
| `@playwright/test` | The browser-driving test runner of the end-to-end journeys (`e2e/`, `npm run e2e`): drives Chromium against the real binary over the fixture workspace (`ARCHITECTURE.md` § The end-to-end journeys). Apache-2.0, as are the two packages it brings, `playwright` and `playwright-core`. The browser it drives is downloaded by `npx playwright install chromium`, outside the lockfile and the tree. |
| `@testing-library/dom` | DOM queries; the peer dependency `@testing-library/react` builds on. |
| `@testing-library/react` | Renders components in tests and queries them as a user would. |
| `accessibility-checker-engine` | The accessibility pass's rules engine: injected into each screen, it reports WCAG 2.2 A and AA violations (`e2e/support/accessibility.ts`). Apache-2.0, no dependency. Chosen over `axe-core`, licensed MPL-2.0, which `deny.toml` leaves off the allow list. |
| `@types/node` | Node's types, for the scripts, the tests and the configuration files. |
| `@types/react` | React's types. |
| `@types/react-dom` | React DOM's types. |
| `eslint` | The linter, and with it the network-confinement rule. |
| `globals` | The browser's and Node's global names, for ESLint's scopes. |
| `happy-dom` | The DOM a component test runs in. |
| `typescript` | The type checker (`npm run typecheck`). |
| `typescript-eslint` | Lets ESLint parse TypeScript, and its recommended rules. |
| `vite` | The dev server and the production build. |
| `vitest` | The test runner. |

## Roles filled without a dependency

Two roles the front end needs are filled by code in `ui/` rather than by a package, because no candidate passed the audit or met the role: **the API type generator** (`scripts/api-types.mjs`; every generator evaluated pulls `argparse`, licensed Python-2.0) and **the typed hash router** (`src/routes.ts`; the typed candidates were off the licence list, untyped outside a framework mode, or broken in hash mode). `ARCHITECTURE.md` § The generated types and § The router and the URL state give the candidates and the reasons. A package that later takes either role is added under the rule above, and replaces the code rather than sitting beside it.
