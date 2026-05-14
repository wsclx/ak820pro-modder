# Reverse-engineering archive

This directory is the **local-only workspace** for vendor artefacts used to decode the AK820 Pro wire protocol. **Nothing in here is redistributed via this repository** — the files are AJAZZ-owned closed-source binaries and minified JavaScript that we do not have a licence to bundle into an MIT codebase.

Every contributor who needs to reproduce a protocol finding downloads the artefact themselves from the URL below, drops it into the path documented here, and verifies the SHA-256 against this table. The repository is gitignored to make that the path of least resistance.

```
docs/reverse-engineering/
├── README.md                       (this file — committed)
├── online-driver/
│   ├── default/                    (default web driver — covers ISO-DE source)
│   │   ├── default-protocol.js     (NOT committed)
│   │   └── …                       (other lazy-loaded chunks)
│   ├── iso-fr/                     (French regional web driver)
│   └── iso-es/                     (Spanish regional web driver)
├── tools/
│   ├── ansi-windows-driver/        (offline Windows .exe — ANSI variant)
│   └── 2.4g-upgrade/               (dongle / firmware updater)
└── captures/                       (USB pcap snapshots — NOT committed)
```

## Why gitignored

The AJAZZ web drivers and Windows tools are unlicensed redistribution risks:

1. **AJAZZ owns the copyright** on their minified JS bundles and the Windows `.exe`. MIT-relicensing them through inclusion in this repo would be incorrect.
2. **Bulk + churn**: 28 MB Windows exe + ~1 MB of JS chunks per regional driver would bloat the repo's git history.
3. **Provenance integrity**: the canonical source is `*.driveall.cn` — pinning a hash and re-fetching at need keeps us aligned with whatever firmware-version the user is actually on.

What we *do* keep in the repo:
- This README and any decoded findings (`docs/PROTOCOL.md`).
- Hex/byte excerpts of decoded protocols in source comments and tests — short, attributed, fair-use for interoperability research.
- USB pcap captures **only** when sanitised and small enough to be useful as test fixtures — and even then we prefer the structured-extract approach (export the relevant 16-byte sequences as Rust test fixtures rather than the raw pcap).

## Sources

### Web drivers

The AJAZZ "online" / web driver is a Vue + Vite SPA. The interesting protocol logic lives in lazy-loaded chunks pulled on first connection. Use the [Playwright MCP](../../.claude/settings.json) (or `curl` from any browser-dev-tools session) to download.

| Variant | URL | Used for |
|---|---|---|
| **Default** (covers ISO-DE source code) | <https://ajazz.driveall.cn> | Primary RE source. Protocol bundle is `assets/index-CGDyjcPg.js`; per-model config + macro / TFT view code is in `assets/layout-default-DElMT--A.js`. |
| **ISO-FR** | <https://a-jazz-fr.driveall.cn> | French AZERTY layout strings + per-key labels. Held in reserve for the multi-layout phase. |
| **ISO-ES** | <https://a-jazz-es.driveall.cn> | Spanish layout strings + per-key labels. Held in reserve for the multi-layout phase. |

Reproduction recipe:

```bash
# 1. Note the chunk hash from the AJAZZ index.html network panel.
# 2. Download with curl:
mkdir -p docs/reverse-engineering/online-driver/default
curl -sSL https://ajazz.driveall.cn/assets/index-CGDyjcPg.js \
  -o docs/reverse-engineering/online-driver/default/default-protocol.js
# 3. Verify the SHA-256 below.
```

### Windows tools

| Tool | Variant | Size | SHA-256 (V1.0.0.5 / current at time of writing) |
|---|---|---|---|
| `AJAZZ_AK820 Pro_三模_0.85英寸彩屏_RGB_V1.0.0.5.exe` | **ANSI** offline driver, full GUI | 28 MB | `98439ddb0b38fc1b4ee63553618ce5f2df2614be36146a7f3d8735df63753b08` |
| `2.4G_upgrade.exe` | Dongle / firmware updater for 2.4 GHz mode | 4.3 MB | `02991598db4a32a6221f95990a01472250f35eac439b6ce09f9dbbdd77e38d31` |

These are PE32 Delphi-compiled binaries (`System.Generics.Collections` signatures in the strings table). Static RE is materially harder than against the web-driver JS; preferred path is **USB pcap of a live session** instead. The exes themselves are useful primarily for:

- Confirming the wire-protocol commands (string-grep / hex pattern search).
- Watching their HID API call sequence under `pcap` while they configure a device.

### Profile exports

| File | Source | SHA-256 |
|---|---|---|
| `official-export-firmware-1.07.json` | Profile-export JSON dumped from a real AK820 Pro on firmware 1.07 via the AJAZZ web driver's "Export" button. Used to seed `src/data/layouts/iso-de.json`. | `7c04cc899c40c2c1ea24fbead8cf1cf3e885a54e58bd5f12deb51ed94b16a152` |

This one is a borderline case — it's the *user's* keymap data, but in AJAZZ's export format. We treat it as user-data + format-derived: gitignored, not redistributed, but used as a known-good fixture during development.

## Verifying a downloaded file

```bash
shasum -a 256 docs/reverse-engineering/<path-to-file>
# Compare against the table above. If it doesn't match,
# either the vendor pushed an update or the download was tampered with.
```

If the hash drifts (e.g. AJAZZ pushes a new bundle), don't update this table silently — open an issue and document the diff so we can track which protocol findings still apply.

## Foot-guns when working with these artefacts

1. **Layout vs wire protocol** — the wire format is layout-agnostic. Any *keycap data* (labels, positions, `cls` flexbox hints) extracted from one variant **must not** be mixed into another variant's layout file. See `docs/HANDOFF.md` § 6.9b.
2. **Compiled vs source** — the AJAZZ web driver ships minified, mangled bundles. Variable names like `O`, `nc`, `mc` are *not stable* across vendor releases. Always pin a SHA when documenting a specific finding so reviewers can cross-check.
3. **Network / no-network** — most contributors won't have a real keyboard to plug in. The web driver shows "no device connected" without WebHID, but its JS bundles still download. That's enough for protocol RE.
4. **Copyright risk** — never paste large blocks of decompiled / decoded vendor JS / x86 into the repo or PR descriptions. Reference by file path + line, and excerpt only the minimum needed to explain a protocol field.
