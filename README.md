# PaddleDesk

[English](README.en.md) | 简体中文

PaddleDesk 是一款 Windows 桌面 OCR 客户端，接入百度 AI Studio 的 PaddleOCR-VL-1.6、PP-OCRv6 和 PP-StructureV3。它侧重复杂文档精度、结构化结果和轻量桌面体验。

> 隐私提示：图片和文档会上传至百度云端识别。若文件不能离开本机，请使用离线 OCR 工具。

## 功能

- 图片/PDF 拖放、批量队列、失败重试、取消和重启续跑
- Markdown、文本、表格、公式和识别区域查看；PDF.js 原页预览
- Markdown、JSON、TXT、CSV 导出及 LaTeX 复制
- SQLite FTS5 历史搜索、分服务用量统计和隐私模式
- `Ctrl+Alt+S` 截图识别，完成后自动复制文字并发送系统通知
- 主页按 `Ctrl+V` 识别剪贴板图片
- 托盘常驻、开机启动、单实例、浅色/深色主题
- 简体中文/English/跟随系统，界面、错误、托盘和通知同步切换
- 首启向导、Windows 凭据管理器 Token、签名自动更新

## 安装与使用

本仓库目前只在本地构建，尚未发布公开安装包。构建完成后的文件位于：

- MSI：`src-tauri/target/release/bundle/msi/PaddleDesk_0.1.0_x64_en-US.msi`
- NSIS 安装器：`src-tauri/target/release/bundle/nsis/PaddleDesk_0.1.0_x64-setup.exe`
- 便携主程序：`src-tauri/target/release/paddledesk.exe`

首次启动时：

1. 阅读云端识别说明。
2. 打开 [PaddleOCR AI Studio 任务页](https://aistudio.baidu.com/paddleocr/task) 获取 Access Token。
3. 在向导中验证；成功后 Token 只写入 Windows 凭据管理器。
4. 拖入文件、粘贴剪贴板图片，或按 `Ctrl+Alt+S` 截图识别。

Token 不会进入 SQLite、配置文件、日志或前端存储。开启隐私模式后，应用仍保留任务生命周期和页数用量，但不保存识别结果与可搜索历史。

## PaddleDesk 与 Umi-OCR

[Umi-OCR](https://github.com/hiroi-sora/Umi-OCR) 是成熟的免费离线 OCR 工具。两者定位不同，不存在对所有用户都更好的选择。

| 维度 | PaddleDesk | Umi-OCR |
| --- | --- | --- |
| 核心取向 | 官方云模型、复杂版面和结构化文档结果 | 本地离线、丰富插件和通用 OCR 工作流 |
| 识别模型 | PaddleOCR-VL-1.6、PP-OCRv6、PP-StructureV3 云服务 | 本地 OCR 插件/模型，效果取决于所选运行库 |
| 精度说明 | 目标是利用最新云模型提升复杂文档、表格和公式表现；本项目未发布双方统一基准，因此不宣称全面胜出 | 本地通用 OCR 很实用；具体文档上的结果应以实测为准 |
| 客户端体积 | 客户端较轻，不随包分发大模型；必须联网并受云端额度约束 | 需要本地运行库/模型，安装体积通常更大；无需云端 OCR 额度 |
| 隐私 | 文档上传百度云；Token 仅存 Windows 凭据管理器 | OCR 可完全离线，敏感文档更合适 |
| 平台 | 当前面向 Windows | 官方支持 Windows 与 Linux |

需要完全离线时选 Umi-OCR；接受云端处理、优先复杂文档能力和轻量客户端时再考虑 PaddleDesk。

## 本地开发

要求：Windows 10/11、WebView2、Node.js、pnpm、稳定版 Rust/MSVC 工具链。

```powershell
pnpm install
pnpm tauri dev
```

验证命令：

```powershell
pnpm test
pnpm build
Set-Location src-tauri
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

## 构建与签名更新

普通前端构建使用 `pnpm build`。Tauri 安装包和更新签名需要本机私钥：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "$HOME\.tauri\paddledesk.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
pnpm tauri build
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

私钥永远不能提交到 Git。公开更新还需要在配置所指向的 GitHub 仓库发布已签名安装包、`.sig` 和 `latest.json`；本仓库不会自动创建远端或发布 Release。正式发布前建议使用带密码的独立发布密钥，并把密钥与密码放入受保护的 CI secrets。

## 安全边界

- 根目录 `test/` 含本地真实 Token，仅用于一次性契约验证，已被 Git 忽略；禁止提交。
- CI 和单元测试只使用 fixture/mock，不发送真实 OCR 请求。
- 前端和存储只消费统一的 `RecognitionResult`，不依赖三个服务的原始响应。
- 更新包只有通过内置公钥验签后才会安装。
