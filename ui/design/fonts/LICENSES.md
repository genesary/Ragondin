# Fonts

The three typefaces of the design system, committed so the binary serves them from its own origin (ADR-C36 § 5): nothing fetches a font at run time. Each file is the upstream project's own web font, **byte for byte**: no subsetting, no instancing, no conversion. Only the upright weights the type scale in `../tokens.json` uses are kept.

The table below is also the manifest `npm run audit` reads (`ui/scripts/font-licenses.mjs`): every font file in this directory needs a row, its licence must be on the allow list `deny.toml` and `ui/scripts/audit-policy.mjs` share, its licence text must sit beside it, and the file must still match the SHA-256 recorded here. A row whose file is gone fails too.

## Families

| Family | Role | Licence | Copyright | Obtained from |
|---|---|---|---|---|
| Wix Madefor Display | `--font-display`, 17 px and up | SIL Open Font License 1.1 (`OFL-1.1`), text in `OFL-WixMadefor.txt` | Copyright 2021 The Wix Madefor Project Authors | The upstream project's release `3.100`, archive `wixmadefor-fonts.zip` (SHA-256 `7fdbd012ca9e245d7c177a341bdbdf789521590e175322a9013c035981138f1c`), directory `fonts/webfonts/`; licence text from the same project at commit `85646f130c8d3edffe66c4d8755c3f9f7abfa877` |
| Wix Madefor Text | `--font-sans`, the UI face | as above | as above | as above |
| Atkinson Hyperlegible Mono | `--font-mono`, hashes, ids, parameters | SIL Open Font License 1.1 (`OFL-1.1`), text in `OFL-AtkinsonHyperlegibleMono.txt` | Copyright 2020-2024 The Atkinson Hyperlegible Mono Project Authors | The upstream project's repository at commit `154d50362016cc3e873eb21d242cd0772384c8f9`, directory `fonts/webfonts/`, and its `OFL.txt` |

The OFL permits bundling a font with software under any licence. Its conditions — the font may not be sold by itself, and a modified font may not keep its Reserved Font Name — bind the font files, not the Apache-2.0 code beside them. Keeping the files unmodified is what the digests below check.

## Files

| File | Family | Weight | Licence | Licence text | SHA-256 |
|---|---|---|---|---|---|
| `WixMadeforDisplay-Medium.woff2` | Wix Madefor Display | 500 | OFL-1.1 | `OFL-WixMadefor.txt` | `9ddcaa52d4a20b9736c39de2f2155e9e9f8133361888f0b43f1005b56a6b5a37` |
| `WixMadeforDisplay-SemiBold.woff2` | Wix Madefor Display | 600 | OFL-1.1 | `OFL-WixMadefor.txt` | `7ca962035026852dccc594b911569cccbce826cd59167f7a8d95d4fd7b5c3286` |
| `WixMadeforDisplay-Bold.woff2` | Wix Madefor Display | 700 | OFL-1.1 | `OFL-WixMadefor.txt` | `d271833346289a4b8656f3943cba2e384c2442c905afae56c4984e37cfa33d29` |
| `WixMadeforText-Regular.woff2` | Wix Madefor Text | 400 | OFL-1.1 | `OFL-WixMadefor.txt` | `0daef7ea53e4525f532bfb4ba79246a76279cf3221c505eaa73b42e0b7bca454` |
| `WixMadeforText-Medium.woff2` | Wix Madefor Text | 500 | OFL-1.1 | `OFL-WixMadefor.txt` | `bed19d27ec250f9d9f7627a0e67d5883ae91436b2780116580b29702fd21cea7` |
| `WixMadeforText-SemiBold.woff2` | Wix Madefor Text | 600 | OFL-1.1 | `OFL-WixMadefor.txt` | `214aaf7b95b891ff79d8dcc68cef31314b50ee0347fc38e3d213fe47a2f249d3` |
| `WixMadeforText-Bold.woff2` | Wix Madefor Text | 700 | OFL-1.1 | `OFL-WixMadefor.txt` | `1160efa6a6588d74244cf251be999b46451b7eaa6d6323ca9cc80710f2890ad5` |
| `AtkinsonHyperlegibleMono-Regular.woff2` | Atkinson Hyperlegible Mono | 400 | OFL-1.1 | `OFL-AtkinsonHyperlegibleMono.txt` | `3b916de5c5247c5fa9736aad6c2673f1b0b0381ab306dcddfcee4a184a842a51` |
| `AtkinsonHyperlegibleMono-Medium.woff2` | Atkinson Hyperlegible Mono | 500 | OFL-1.1 | `OFL-AtkinsonHyperlegibleMono.txt` | `a9572b3a95d6e20bfdf08c4e351069416c0f07880435ef0822819e4f0410b459` |

## Why these weights

The type scale sets Display at 500 (`type-verdict`) and 600 (`type-title`, `type-figure`, `type-cover`); the wordmark is set at 650, which the browser's font matching resolves to the 700 file. Text is set at 400, 500 and 600, and 700 marks the best value in a table or a metric chip. Mono is set at 400 (`type-mono`) and 500 (`type-hash`). No style in the scale is italic, so no italic is committed. A weight added to the scale adds its file here, with its row.
