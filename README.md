# MyAIPDF 中文 macOS 版

基于 [PrintCraft](https://github.com/storytold/printcraft)（上游 commit `1c78ff479ce5cf5cbcf7af5100378c9311b95162`），保留 MIT OR Apache-2.0 开源许可和作者署名。本改版与上游作者无隶属关系，已移除上游专属品牌标识。

## 安装

本次构建面向 Apple Silicon（M 系列）Mac。打开 MyAIPDF.dmg，将 MyAIPDF 拖到 Applications。使用前建议保留 PDF 原件，修改后使用“另存为”。

这是本地签名构建，没有 Apple 开发者公证。首次启动如被 macOS 提醒，请在 Finder 中右键应用选择“打开”，或按照“系统设置 → 隐私与安全性”中的提示操作；不要关闭系统安全保护。

## AI 助手

左侧“所有工具 → AI 助手”，或顶部 AI 按钮：

1. 新增接口并填写 API 地址和密钥。支持 OpenAI 兼容的 Models / Chat Completions 协议；远程接口须 HTTPS，本机接口可用 HTTP。
2. 点击“拉取模型”，选择支持对话的模型。也可手动填写模型 ID。
3. “保存接口设置”保存地址与模型；密钥默认仅留在当前会话，勾选“保存密钥到 macOS 钥匙串”才持久保存密钥。普通配置文件不保存密钥。
4. 输入 PDF 处理要求并发送。默认不发送 PDF 文字；勾选发送文字后，可选择当前页或全文（前 150 页、最多 6 万字）。
5. AI 可总结、翻译、提取信息，并提议旋转／移动／删除／插入空白页、修改文档属性、填写表单、添加批注和书签。
6. 修改方案需点击“确认执行”。文档切换或版本改变后拒绝旧方案。执行不会自动保存原文件，可撤销后另存为。

AI 输出仅供参考。真实服务商的模型权限、收费和数据政策由用户确认；不同兼容服务的行为可能有差异。对话内容不写入配置文件；已经发出的内容由服务商按其政策处理。应用的崩溃恢复备份属于本地 PDF 数据，与 AI 对话无关。

## 能力边界

继承上游当前已经实现的 PDF 阅读、整理、批注、表单、合并拆分、密码保护、导出等能力。规划功能继续明确标注未实现，本版不是 Adobe Acrobat 的完整替代品。

- 中文界面嵌入 OFL 许可的 IBM Plex Sans SC 黑体，含常规／中粗／半粗字重，不依赖本机中文字体；不会更改 PDF 原有字体。
- 上游既有文字编辑仍有西文／CJK 字体限制，不能保证任意 PDF 中文原文可编辑。
- AI 总结和翻译以可提取文字为准，不会将 PDF 翻译自动排版写回原文件。
- 自带上游 OCR 模型时，仅支持英文／基础拉丁字母，不支持中文扫描件 OCR。
- 没有内置 AI 密钥或付费额度。

## 构建

Rust 稳定工具链，Apple Command Line Tools；`CRAFT_FONTS_DIR` 指向独立 craft-fonts 构建输入，IBM Plex Sans SC 及各字体许可／哈希见 manifest 和 ATTRIBUTION.toml。

```sh
cargo xtask myaipdf-fonts
cargo xtask models
cargo xtask myaipdf-icon
cargo xtask assets --write
CRAFT_FONTS_DIR=/absolute/path/to/craft-fonts cargo build --locked --release -p printcraft -p printcraft-cli -p myaipdf-updater
bash packaging/macos/myaipdf.sh /absolute/path/MyAIPDF.dmg
```

## 更新与版本

当前 MyAIPDF 版本 **0.1.1**（底层 PrintCraft 0.2.1）。红紫色原创 DPF 图标；中文界面采用 OFL 中文黑体，包含常规、中等、半粗字重，仅改变界面字体，不替换 PDF 的字体。

“菜单 → 帮助 → 关于 MyAIPDF”底部点击“检查更新”，只查询 [ddxmu/MyAIPDF 正式版本](https://github.com/ddxmu/MyAIPDF/releases)。有新版时点击“下载安装包”，通过 GitHub 的 SHA-256 校验后再点击“安装并重启”。有未保存 PDF 时不会退出安装。更新保留原程序备份及个人设置；没有启动时自动检查、遥测或静默更新，不会安装上游 PrintCraft 或其他仓库的程序。

发布新版需同步 `crates/update/src/lib.rs` 的 APP_VERSION、关于页面和 macOS Info.plist 版本，运行测试、构建并打包后，以 `v版本号` 创建 GitHub Release，附件名称必须为 `MyAIPDF.dmg`。GitHub 提供的附件 digest 用于客户端校验；缺少 digest 时客户端只提供手动下载入口。源码由每个版本的 tag 留存。0.1.0 为本地早期版，0.1.1 是本仓库首个源码发布版。

核心 AI 客户端在 `crates/ai`；真实 PDF 操作复用 `crates/automation` 的验证工具；界面在 `crates/ui-egui/src/ai_ui.rs`。不默认监听端口，不默认启用远程控制。

## 许可与来源

[LICENSE-MIT](LICENSE-MIT)、[LICENSE-APACHE](LICENSE-APACHE)、[NOTICE](NOTICE)、[ATTRIBUTION.md](ATTRIBUTION.md)。新增图标为原创 MIT 资产；字体各自遵循 OFL。其余底层 PDF 引擎、图标、依赖及 OCR 模型遵循各自许可。字体和 OCR 模型来自各自上游，不属于 MyAIPDF 专有资产。
