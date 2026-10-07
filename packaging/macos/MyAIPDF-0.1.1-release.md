# MyAIPDF 中文 macOS 版 0.1.1

面向 Apple Silicon（M 系列）Mac，底层 PrintCraft 0.2.1。本仓库首个源码及安装包发布版本。

- 原创红紫渐变 DPF 应用图标，统一 Dock、启动界面与关于页面。
- 中文界面改用开源 IBM Plex Sans SC 黑体，按常规／中等／半粗字重显示。不会替换 PDF 本身的字体。
- “菜单 → 帮助 → 关于 MyAIPDF”底部新增检查更新、下载安装包、安装并重启。
- 更新只获取 ddxmu/MyAIPDF 的正式发布版本，验证 GitHub SHA-256、应用标识、版本及签名后安装。未保存 PDF 会阻止安装；保留旧程序备份与个人设置。无启动时检查或静默安装。
- 保留自定义 AI 接口、密钥、模型列表、AI 对话及经确认后执行的 PDF 操作；AI 不会自行保存原文件。

首次安装或从本地早期版 0.1.0 升级：打开 `MyAIPDF.dmg`，保存并退出旧程序，将 MyAIPDF 拖入 Applications。内置更新从 0.1.1 开始提供。

本包为本地签名版本，尚无 Apple 开发者公证。若 macOS 提醒无法验证开发者，请使用系统允许的“打开”／“隐私与安全性”流程，不要关闭系统安全保护。

本应用不是 Adobe Acrobat 全功能替代品；上游部分工具尚在开发。OCR 仅支持英文／基础拉丁字母；AI 服务、密钥与额度由用户自行提供。

源码：MIT OR Apache-2.0，保留上游作者与许可。IBM Plex 字体：OFL-1.1。原创图标：MIT。其他资产及依赖许可见应用内 ATTRIBUTION.md、Licenses 与源码仓库 NOTICE。
