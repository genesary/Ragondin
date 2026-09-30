# `ui/` — architecture

The front end: a TypeScript application, built with Vite and React, that the `ragondin ui` subcommand serves (ADR-C36). It is **not a crate** and sits outside Cargo, but it is **load-bearing** in the sense `AGENTS.md` § Documentation ships with the code it describes means: this file is read before `ui/` is modified, and a diff that falsifies it corrects it in the same pull request (ADR-C36 § 5).

Today it is the governed application with its design system: one placeholder page, every gate green, every rule written, and the tokens, faces, glyphs and primitive components every screen will be built from under `design/` (§ The design system). No screen and no API call exist yet.

## What lives here

```
ui/
├── .node-version        # the one pinned Node major; CI reads it, `engines` mirrors it
├── .npmrc               # engine-strict: `npm ci` refuses any other Node major
├── package.json         # scripts, runtime and development dependencies
├── package-lock.json    # committed; installed with `npm ci` only
├── DEPENDENCIES.md      # every dependency, with its role (and, if runtime, its reason)
├── index.html           # the single entry of the production build
├── eslint.config.js     # lint, including the network-confinement rule
├── vite.config.ts       # build and test runner configuration
├── tsconfig*.json       # strict TypeScript: app code (DOM) and tooling (Node) apart
├── design/              # the design system as code (§ The design system)
│   ├── tokens.json      # the token source: colours, type, space, radius, depth, motion, sizes
│   ├── tokens.css       # generated from tokens.json (`npm run tokens`), committed
│   ├── fonts.css        # @font-face for the committed faces
│   ├── base.css         # reset, body ground and ink, focus, reduced motion; imports the two above
│   ├── fonts/           # the woff2 files, their OFL texts, LICENSES.md (also the font audit's manifest)
│   ├── glyphs/          # one SVG per glyph, Glyph and FamilyTile
│   ├── forms/           # what the form fields share: the label stack and the helper line
│   ├── components/      # one directory per primitive: Component.tsx, Component.css, Component.test.tsx
│   ├── roving.ts        # the arrow-key rule of a one-tab-stop group
│   ├── index.ts         # what a screen imports
│   ├── preview/         # the dev-only preview page, never a build input
│   └── testing/         # the CSS reader the design tests use; nothing in the bundle imports it
├── src/
│   ├── main.tsx         # mounts the application, loading design/base.css once
│   ├── App.tsx          # the placeholder page
│   └── api/             # the only module that may touch the network
│       └── base.ts      # the API's base address, '/api/v1'
├── scripts/             # the dependency audit (npm and fonts), and the token generator
└── tests/               # tests of the governance itself: lint rule, audit, DEPENDENCIES.md, tokens, one origin, preview
```

A component's test sits beside it, in `src/` or `design/`; a test of a rule about `ui/` sits in `tests/`. `tests/setup.ts` unmounts what each test rendered: Vitest's globals are off, so Testing Library cannot register that cleanup itself.

## The gates

`npm run check` is the one command, and CI's `ui` job, `just check-ui` and a contributor all run it. It runs, cheapest first:

| Script | What it does |
|---|---|
| `lint` | ESLint over everything, warnings are errors. |
| `typecheck` | `tsc -b` over the application and the tooling, both strict. |
| `test` | Vitest: component tests in a DOM, governance tests in Node. |
| `build` | `vite build` into `dist/`. |
| `audit` | The font licence audit, the npm licence audit, then the advisory audit (§ The dependency audit). |

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

**There is one policy and no per-package licence exception.** A licence outside the list is refused whatever the package's role, runtime or development. A dependency that needs another licence is admitted by adding that licence to `deny.toml`, for the reasons its comments give, and to the copy in the same change — as `BlueOak-1.0.0` was, for `minimatch`, which ESLint depends on unconditionally.

**Fonts** (`scripts/check-font-licenses.mjs`, logic in `scripts/font-licenses.mjs`). A font is a committed file, not a package, so no lockfile names it and the npm audit above never sees it. This check holds it to the same list. It walks all of `ui/` but `node_modules/` and `dist/` for font files, and every one must sit directly in `design/fonts/` with exactly one row in `design/fonts/LICENSES.md`, which is the manifest. The row names its licence, which must be on `LICENSE_ALLOW`; its licence text, which must sit in the same directory, named without a path, and be pinned by its own SHA-256; and the SHA-256 the font had when it was committed. A font or licence text edited after the fact fails, a row listed twice fails, and a row whose file is gone fails. It reads files only, so it needs no network. `OFL-1.1` entered `deny.toml` and its copy for the design system's typefaces, as the comment there says.

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
- **happy-dom, not jsdom**, as the test DOM. jsdom's tree carries MIT-0 and CC0-1.0 packages, off the list, and its current release declares `engines` of Node `^24.15.0`, a minor the major pin does not guarantee; happy-dom is MIT with a small tree.
- **TypeScript 6.0.** TypeScript 7 exists, but `typescript-eslint` supports up to 6.0; the lint parses with the same compiler the type check runs.
- **Vitest and Testing Library** for tests, **ESLint** with `typescript-eslint` for the lint, as the front-end design names them.

## How the assets reach the binary

`npm run build` writes `dist/`, which is ignored by git: nothing here commits built assets. Embedding `dist/` into `bin/ragondin` behind its `ui` feature, and what a build does when `dist/` is absent, belong to the `ragondin ui` subcommand's issue (#339) and to ADR-C36 § 1 and § 5; nothing on the Rust side reads `ui/` yet.

## The design system

`design/` is the design system as code, and the only place a colour, a type style, a space, a radius, a shadow or a duration is written. Issues cite `ui/design/` the way a Rust issue cites an ADR, and a change to it is reviewed that way: it is the reference every screen is checked against, not an implementation detail of one. Where the design system's source and a later need disagree, the change lands here first, with its test.

**Tokens are the only source of colour, type and spacing.** A component names a role — `var(--ink-2)`, `var(--surface)`, `var(--type-dense)`, `var(--space-3)` — never a value, and takes every colour from the token set of the surface behind it: a chip on a `surface` uses the inks and washes defined for surfaces, and the same markup under a dark subtree reads the dark values without a line changing. Tokens are named by role (`--ink-2`, not a grey step), so a theme swaps values and touches no component.

**How tokens flow into CSS.** `design/tokens.json` is the token source, carried over from the design system with the usage note of each token. `scripts/tokens.mjs` renders it into `design/tokens.css`, which is committed; `npm run tokens` regenerates it, and `tests/design-tokens.test.ts` fails when the committed file is not what tokens.json renders to, so neither can drift from the other. The same test pins the node-family pigments, the run inks and the better/worse, state and accent pairs to the design system's values in both themes. `base.css` imports `tokens.css` and `fonts.css`; `src/main.tsx` imports `base.css` once; each component imports its own stylesheet, and Vite bundles them all into the one CSS file the binary serves. Tokens that do not change with the theme — the three font families and their fallback stacks, the twelve type styles as `font` shorthands (with a `-tracking` companion), the 4 px spacing grid, radii, control and canvas sizes, durations and easings — sit in one bare `:root` block. Under `prefers-reduced-motion` every duration but `--duration-0` becomes 1 ms in `tokens.css`, and `base.css` catches any literal animation.

**The theme mechanism.** Every colour and every shadow is themed; the generator writes them three times:

- the complete light palette on `:root, [data-theme="light"]`, with `color-scheme: light` — so every token has a value on bare `:root` before any block redefines it;
- the dark palette under `@media (prefers-color-scheme: dark)`, on `:root:not([data-theme="light"])`, with `color-scheme: dark` — the system's choice, unless the page has chosen light;
- the dark palette again on `:root[data-theme="dark"], [data-theme="dark"]`, with `color-scheme: dark` — the page's choice, or a subtree's.

The test asserts that both dark blocks redefine exactly the set the light block defines. A theme applies to any element carrying `data-theme`, not only the root, which is how the preview shows both themes side by side and how a theme switch will set one; `base.css` gives every `[data-theme]` element the `--ground` background and `--ink` colour, so a themed subtree repaints its own ground and its bare text rather than inheriting the other theme's ink. A token whose role differs by theme is its own token rather than a dark-only selector, so the `prefers-color-scheme` path gets it too: `--seg-thumb` is `surface` in light and `line-strong` in dark, so the segmented thumb reads as raised above its track in both. An alias (`--focus-ring: var(--accent)`) is written in every themed block rather than once, because a custom property's `var()` resolves on the element that declares it: declared once on the root, a dark subtree would inherit the light accent.

**Styling: plain CSS, no CSS-in-JS and no component kit** (ADR-C36 § 5). Class names are global and carry the `rg-` prefix, which is their scope. A state or a variant is drawn from the element's ARIA attribute or a `data-*` attribute — `aria-pressed`, `aria-checked`, `aria-selected`, `aria-current`, `aria-invalid`, `aria-busy`, `aria-disabled`, `data-state`, `data-tone`, `data-cell`, `data-best`, `data-connected`, `data-floating`, `data-error` — never from a class, so the rule that draws a state and the attribute assistive technology reads are the same fact, and a test finds one by the other. The one attribute no product markup sets is `data-preview-state` (`hover`, `focus`, `pressed`), listed beside the pseudo-class it mirrors so the preview page can show a hover, a focus ring or a press at rest. CSS Modules were not used: the tests assert the rule that draws each state by its selector, and the selectors keep the design system's own vocabulary, so a reviewer compares a component with its reference one to one. `.rg-visually-hidden` lives in `base.css`: a component that carries its second channel as hidden text needs the base stylesheet loaded.

**Colour is never the only carrier.** A status, a family, a run or a better/worse reading always comes with a glyph, a letter, a mark or a word: `StatusChip` has its icon or meter and its word, `RunSwatch` its letter (or "baseline" and a dashed outline), `Delta` its sign, arrow and a hidden "better" or "worse", `FilterChip` a check when pressed, `RankStrip` filled versus hollow cells and a sentence, `TopBar` a hollow ring and "unreachable", `InlineMessage` a glyph named "Error", "Warning" or "Note", the best table value its weight and a hidden "(best)". Each such component's test asserts the second carrier, not only the colour.

**Components.** One directory each under `design/components/`: the component, its stylesheet, and a test per state that asserts the rendered state and, where the state is drawn by CSS (hover, pressed, focus), the rule that draws it — the test reads the stylesheet with Vite's `?raw` import, which is why `vite.config.ts` sets `css: true`. The API is small: props for state, children for content. `Progress` throws without a finite value: there is no indeterminate variant; its `progressbar` role sits on the track, named by the count, so an action beside the count stays a button. A disabled `Button` is `aria-disabled` rather than natively disabled: it stays in the tab order, refuses the click, and its reason describes it, so a keyboard or screen-reader user reaches the reason without a pointer. A disabled `FilterChip` or `Checkbox` shows its reason as visible text that describes the control rather than joining its name. The selected `Table` row carries `aria-current`, which a table row honours, and a bar on its edge beside the tint. `SegmentedControl` and `Tabs` keep one tab stop — on the first option when the value matches none — and the arrow keys move the choice without scrolling the page. `FamilyTile` (a family's pigment with its glyph) is a primitive beside `Glyph`, because the inspector's head and the canvas both need it. Two choices differ from the design system's reference markup, and why: `SegmentedControl` is a `radiogroup` of `radio`s rather than pressed buttons, since one tab stop moved by the arrow keys is the radio-group pattern assistive technology expects; `StatusChip` has a `warning` state beside the four the issue named, because the design system draws one.

**Fonts** are the upstream projects' own woff2 files, unmodified, in the upright weights the type scale uses (`design/fonts/LICENSES.md` lists them, where they came from and why each weight is there). `fonts.css` points every face at `./fonts/`, never at a host, with `font-display: swap` so the fallback stack stands in while a face loads. The wordmark's 650 resolves to the 700 file by the browser's weight matching.

**Glyphs** are one SVG file each under `design/glyphs/`, drawn on a 16 px grid in `currentColor`; the files are the source. `Glyph` inlines a drawing by name, read from the files at build time with `import.meta.glob`, so it takes the ink of its context and nothing is fetched. `GLYPH_NAMES` and the directory are held equal by a test.

**One origin** (§ The one-address rule): `tests/design-assets.test.ts` reads every stylesheet, component, SVG, HTML entry and `tokens.json` under `design/` and `src/` and fails on any `scheme://` address, whatever its host (a name, an IP, `localhost`), and any protocol-relative `//host` opening a string or a `url()` — an SVG's `xmlns` namespace aside, which nothing fetches. The design system's reference stylesheet loaded its faces from a font host; that line has no counterpart here.

**The preview page**, `design/preview/`, renders every primitive in every state, in both themes side by side. It is the reviewer's tool: `npm run dev`, then `/design/preview/`. It is not an input of the production build, whose only entry is `index.html`: `tests/design-preview.test.ts` runs the same build `npm run build` runs into a scratch directory, lists what it wrote and fails on any preview file or any file carrying the preview's markup; it also starts the dev server and fetches the page.
