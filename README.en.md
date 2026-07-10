<div align="center">

<img src="public/paddledesk-icon.png" alt="PaddleDesk" width="96" />

# PaddleDesk

**Open-source Windows desktop client for the official PaddleOCR cloud API**

Complex documents · Structured results · Lightweight desktop experience

[![CI](https://github.com/chengbuilds/PaddleDesk/actions/workflows/ci.yml/badge.svg)](https://github.com/chengbuilds/PaddleDesk/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/chengbuilds/PaddleDesk?include_prereleases)](https://github.com/chengbuilds/PaddleDesk/releases)
![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-informational)
![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%2B%20React-orange)

English | [简体中文](README.md)

</div>

---

PaddleDesk connects to Baidu AI Studio's three official PaddleOCR cloud services and turns complex documents (tables, formulas, multi-column layouts, handwriting) into editable, exportable structured results. The client is lightweight, ships no bundled models, and accuracy improves continuously with the official cloud models.

> **Privacy notice**: images and documents are uploaded to Baidu Cloud for recognition. Use an offline OCR tool (such as [Umi-OCR](https://github.com/hiroi-sora/Umi-OCR)) if a file must remain on your device.

## Preview

![Result viewer: source document next to structured output with recognition regions](assets/screenshots/viewer.png)

| Home · drag-and-drop / capture / clipboard | Usage · per-service quota and 7-day history |
| :---: | :---: |
| ![Home](assets/screenshots/home.png) | ![Usage](assets/screenshots/usage.png) |

## Supported services

| Service | Positioning | Best for |
| --- | --- | --- |
| **PaddleOCR-VL-1.6** | Vision-language model | Complex layouts, handwriting, mixed text-figure documents |
| **PP-OCRv6** | General text recognition | Fast recognition of regular printed screenshots and single images |
| **PP-StructureV3** | Layout analysis | Table reconstruction (CSV), formulas (LaTeX), reading order |

Each service has its own quota (see the [official AI Studio page](https://aistudio.baidu.com/paddleocr) for current free tiers); the app tracks usage per service.

## Features

**Recognition & queue**
- Image/PDF drag-and-drop with automatic batch queueing; retry, cancel, and resume after app restart
- `Ctrl+Alt+S` global screen capture — recognized text is auto-copied with a native notification
- `Ctrl+V` clipboard-image recognition on Home

**Results & export**
- Four views: Markdown preview / source / JSON / plain text; page-by-page source preview (PDF.js) with recognition-region overlay
- Export Markdown, JSON, TXT; tables export to CSV, formulas copy as LaTeX
- Full-text history search backed by SQLite FTS5

**Desktop experience**
- Tray residency, autostart, single instance, light/dark/system themes
- Simplified Chinese / English / system language across UI, errors, tray, and notifications
- First-run wizard for obtaining and validating the token; signature-verified auto-updates

**Privacy & security**
- The token is stored only in Windows Credential Manager — never in SQLite, config files, logs, or frontend storage
- Privacy mode keeps task lifecycle and page counts while omitting recognition results and searchable history
- Updates install only after signature verification against the bundled public key

## Install

Download the latest version from [Releases](https://github.com/chengbuilds/PaddleDesk/releases):

| File | Description |
| --- | --- |
| `PaddleDesk_x.y.z_x64_en-US.msi` | MSI installer |
| `PaddleDesk_x.y.z_x64-setup.exe` | NSIS installer |

Requires Windows 10/11 with WebView2 (bundled with Windows 11). You can also build from source — see "Local development".

## Quick start

1. Read the cloud-recognition disclosure on first launch.
2. Open the [official PaddleOCR page](https://aistudio.baidu.com/paddleocr), sign in to Baidu AI Studio, and follow the page instructions to create and copy an access token.
3. Paste and validate it in the wizard. A valid token is stored only in Windows Credential Manager.
4. Drop files, paste a clipboard image, or press `Ctrl+Alt+S` to capture part of the screen.

## PaddleDesk vs Umi-OCR

[Umi-OCR](https://github.com/hiroi-sora/Umi-OCR) is a mature, free offline OCR application. The products serve different priorities; neither is universally better.

| Area | PaddleDesk | Umi-OCR |
| --- | --- | --- |
| Primary goal | Official cloud models, complex layouts, and structured document output | Local offline processing, broad plugins, and general OCR workflows |
| Models | Cloud PaddleOCR-VL-1.6, PP-OCRv6, and PP-StructureV3 | Local OCR plugins/models; results depend on the selected runtime |
| Accuracy claim | Designed to benefit from current cloud models on complex documents, tables, and formulas. No shared benchmark has been published, so PaddleDesk does not claim a universal win | Very capable for local general OCR; test representative documents before choosing |
| Client footprint | Lightweight client without bundled large models; requires a network connection and cloud quota | Includes or downloads local runtimes/models, so installation is generally larger; no cloud OCR quota |
| Privacy | Documents go to Baidu Cloud; the token stays in Windows Credential Manager | OCR can remain fully offline and is preferable for sensitive files |
| Platforms | Windows today | Windows and Linux according to the official project |

Choose Umi-OCR when offline processing is mandatory. Consider PaddleDesk when cloud processing is acceptable and complex-document capability plus a lightweight client matter more.

## Architecture

```
src-tauri/src/          Rust core
  api/        OcrService trait + three service implementations + unified result normalization
  queue/      Task queue, retry, resume (state persisted in SQLite)
  capture/    Screen capture, global hotkey (platform layer, trait-isolated)
  storage/    SQLite (tasks / results / usage / settings, FTS5 full-text search)
  export/     Markdown / JSON / TXT / CSV writers
src/                    React + TypeScript frontend (views / components / stores / i18n)
```

The frontend and storage consume only the unified `RecognitionResult` model, fully decoupled from the three services' raw responses.

## Local development

Requirements: Windows 10/11, WebView2, Node.js, pnpm, and the stable Rust/MSVC toolchain.

```powershell
pnpm install
pnpm tauri dev
```

To isolate a debug build from your real application data, set a temporary data directory first; release builds ignore this variable:

```powershell
$env:PADDLEDESK_TEST_DATA_DIR = Join-Path $env:TEMP "paddledesk-test-data"
pnpm tauri dev
```

Validation commands:

```powershell
pnpm test
pnpm build
Set-Location src-tauri
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

CI and unit tests use fixtures/wiremock only — no real OCR requests, no quota spend; presigned download URLs inside fixtures are sanitized.

## Build and signed updates

Use `pnpm build` for the frontend. Tauri installers and updater signatures require the local private key:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "$HOME\.tauri\paddledesk.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
pnpm tauri build
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

Never commit the private key. Public updates also require a signed installer, `.sig`, and `latest.json` in the GitHub repository configured by the app. Before a production release, generate a password-protected release key and keep both key and password in protected CI secrets.

## Contributing

Contributions of any kind are welcome:

- Open an [Issue](https://github.com/chengbuilds/PaddleDesk/issues) to report bugs or suggest features
- Submit a [Pull Request](https://github.com/chengbuilds/PaddleDesk/pulls) for code or docs — please follow Conventional Commits (`feat:` / `fix:` / `docs:` …)
- Make sure `pnpm test` and `cd src-tauri && cargo test` pass before submitting

This project is developed with the assistance of [Claude Code](https://github.com/anthropics/claude-code).

## License

Released under the [MIT License](LICENSE).

## Acknowledgements

- **[PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR)** — heartfelt thanks to the PaddleOCR team for their long-standing open-source contributions to OCR. All recognition capability in PaddleDesk comes from the PaddleOCR-VL, PP-OCRv6, and PP-StructureV3 models and their official cloud services; this project would not exist without their work.
- **[Baidu AI Studio](https://aistudio.baidu.com/paddleocr)** — provides the official PaddleOCR cloud API and free quota.
- **[Tauri](https://tauri.app/)** — the lightweight, secure desktop application framework.
- **[PDF.js](https://mozilla.github.io/pdf.js/)** — source-page preview.
- **[react-markdown](https://github.com/remarkjs/react-markdown)** and **[KaTeX](https://katex.org/)** — Markdown and formula rendering.
- **[Umi-OCR](https://github.com/hiroi-sora/Umi-OCR)** — an excellent offline OCR tool and an important reference point for this project's positioning.
