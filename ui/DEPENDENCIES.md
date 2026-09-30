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

## Development dependencies

| Package | Role |
|---|---|
| `@eslint/js` | ESLint's recommended JavaScript rules. |
| `@testing-library/dom` | DOM queries; the peer dependency `@testing-library/react` builds on. |
| `@testing-library/react` | Renders components in tests and queries them as a user would. |
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
