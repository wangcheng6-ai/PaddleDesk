# PaddleDesk

English | [简体中文](README.md)

PaddleDesk is a Windows OCR desktop client for Baidu AI Studio's PaddleOCR-VL-1.6, PP-OCRv6, and PP-StructureV3 services. It focuses on complex-document accuracy, structured results, and a lightweight desktop experience.

> Privacy notice: images and documents are uploaded to Baidu Cloud for recognition. Use an offline OCR tool if a file must remain on your device.

## Features

- Image/PDF drag-and-drop, batch queues, retry, cancel, and restart recovery
- Markdown, text, table, formula, and bounding-box views with PDF.js source preview
- Markdown, JSON, TXT, and CSV export, plus LaTeX copy
- SQLite FTS5 history, per-service usage, and privacy mode
- `Ctrl+Alt+S` screen capture with automatic result copy and a native notification
- `Ctrl+V` clipboard-image recognition on Home
- Tray residency, autostart, single instance, and light/dark themes
- Simplified Chinese, English, and system-language modes across UI, errors, tray, and notifications
- First-run setup, Windows Credential Manager token storage, and signed updates

## Install and use

This repository is currently built locally and has no public release. Build outputs are placed under:

- MSI: `src-tauri/target/release/bundle/msi/PaddleDesk_0.1.0_x64_en-US.msi`
- NSIS installer: `src-tauri/target/release/bundle/nsis/PaddleDesk_0.1.0_x64-setup.exe`
- Portable executable: `src-tauri/target/release/paddledesk.exe`

On first launch:

1. Read the cloud-recognition disclosure.
2. Open the [PaddleOCR AI Studio task page](https://aistudio.baidu.com/paddleocr/task) and obtain an access token.
3. Validate it in the wizard. A valid token is stored only in Windows Credential Manager.
4. Drop files, paste a clipboard image, or press `Ctrl+Alt+S` to capture part of the screen.

The token never enters SQLite, configuration files, logs, or frontend storage. Privacy mode keeps task lifecycle and page usage while omitting recognition results and searchable history.

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

## Local development

Requirements: Windows 10/11, WebView2, Node.js, pnpm, and the stable Rust/MSVC toolchain.

```powershell
pnpm install
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

## Build and signed updates

Use `pnpm build` for the frontend. Tauri installers and updater signatures require the local private key:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "$HOME\.tauri\paddledesk.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
pnpm tauri build
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

Never commit the private key. Public updates also require a signed installer, `.sig`, and `latest.json` in the GitHub repository configured by the app. This repository does not create a remote or publish a Release automatically. Before a production release, generate a password-protected release key and keep both key and password in protected CI secrets.

## Security boundaries

- The root `test/` directory contains a real local token used once for contract verification. Git ignores it, and it must never be committed.
- CI and unit tests use fixtures/mocks only and never call the real OCR API.
- The frontend and storage consume only the unified `RecognitionResult`, not raw service responses.
- An update is installed only after its signature passes verification against the bundled public key.
