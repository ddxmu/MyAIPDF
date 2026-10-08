# MyAIPDF 中文 macOS 版

基于 [PrintCraft](https://github.com/storytold/printcraft)（上游 commit `1c78ff479ce5cf5cbcf7af5100378c9311b95162`），保留 MIT OR Apache-2.0 开源许可和作者署名。本改版与上游作者无隶属关系，已移除上游专属品牌标识。

## 安装

本次构建面向 Apple Silicon（M 系列）Mac。发布完整安装包以及版本匹配的增量更新；本版增量包以未修改的 0.1.6 为基础。使用前建议保留 PDF 原件，修改后使用“另存为”。

本地交付目录统一为“桌面 / MyAIPDF / 版本号 /”。每个新版分别留存 `MyAIPDF.app`、完整 `MyAIPDF.dmg`、增量 `.delta.dmg`、`SHA256SUMS.txt` 和使用说明，不覆盖旧版本。日常使用建议将应用安装到 Applications，桌面文件夹用于保存各版本。

这是本地签名构建，没有 Apple 开发者公证。首次启动如被 macOS 提醒，请在 Finder 中右键应用选择“打开”，或按照“系统设置 → 隐私与安全性”中的提示操作；不要关闭系统安全保护。

## AI 助手

左侧“所有工具 → AI 助手”，或顶部 AI 按钮：

1. 点击“接口设置”打开独立弹窗，新增接口并填写 API 地址和密钥。输入框、下拉框和按钮有清晰边界；对话记录与输入框保留在侧栏。支持 OpenAI 兼容的 Models / Chat Completions 协议；远程接口须 HTTPS，用户自行运行的本机接口可用 HTTP。MyAIPDF 不安装、不启动任何本机 AI 模型，默认 API 地址为空。
2. 点击“拉取模型”，选择支持对话的模型。也可手动填写模型 ID。
3. “保存接口设置”保存地址与模型；密钥默认仅留在当前会话，勾选“保存密钥到 macOS 钥匙串”才持久保存密钥。普通配置文件不保存密钥。
4. 输入 PDF 处理要求并发送。默认不发送 PDF 文字；勾选发送文字后，可选择当前页或全文（前 150 页、最多 6 万字）。
5. AI 可总结、翻译、提取信息，并提议旋转／移动／删除／插入空白页、修改文档属性、填写表单、添加批注和书签。
6. 修改方案需点击“确认执行”。文档切换或版本改变后拒绝旧方案。执行不会自动保存原文件，可撤销后另存为。

AI 输出仅供参考。真实服务商的模型权限、收费和数据政策由用户确认；不同兼容服务的行为可能有差异。对话内容不写入配置文件；已经发出的内容由服务商按其政策处理。应用的崩溃恢复备份属于本地 PDF 数据，与 AI 对话无关。

## 能力边界

继承上游当前已经实现的 PDF 阅读、整理、批注、表单、合并拆分、密码保护、导出等能力。规划功能继续明确标注未实现，本版不是 Adobe Acrobat 的完整替代品。

- 中文界面使用 OFL 中文黑体，常规／中粗／半粗字重的中英文共用基线。界面用的派生字体仅校正行距，按原许可将保留名称 Plex 改为 MyAI，版权和许可不变；文档字体不受界面设置影响。
- 点击顶部“编辑”，再点击页面文字框，可修改内容、字号、颜色、加粗等；点击“应用修改”或 ⌘Enter 提交，⌘S 会先提交输入草稿再保存。建议先“另存为”保留原件。
- 优先复用 PDF 自身字体。原字体缺少新增汉字或切换中文字重时，嵌入 IBM Plex Sans SC 的 TrueType 字形子集，保留 Unicode，可保存重开后继续编辑；中文按框宽换行，手动换行不会被吞掉。格式调整以整个文字框为单位，尚不支持框内逐字混合格式。
- 不是所有 PDF 文字都能编辑：扫描图片、文字轮廓、复杂嵌套 Form 内容和缺少可靠字符映射的文档仍有边界。检测不到可编辑文字时明确提示，不伪装成功；暂不支持任意本机字体选择或中文扫描件原文编辑。
- AI 总结和翻译以可提取文字为准，不会将 PDF 翻译自动排版写回原文件。
- 自带上游 OCR 模型时，仅支持英文／基础拉丁字母，不支持中文扫描件 OCR。
- 没有内置 AI 密钥或付费额度。

## PDF 转 Word

“导出 PDF → Microsoft Word”先打开转换方式对话框，再由用户导出。转换在本机进行，不上传 PDF、不改动原文件，也不会默默把水印删除。

- **原样保真（页面图像）**：默认方式，每个 PDF 页面对应一页 Word，200 dpi 图像保留字体外观、表格、Logo、位置和分页；正文不能逐字编辑。
- **可编辑文字（定位文本框）**：可识别的水平正文按 PDF 坐标放入 Word 文本框，保留原字体名称、字号、字重和颜色。需在 Word 电脑上安装原字体，否则会被替代；复杂、旋转、透明、裁切、符号字体、扫描及未识别内容保留为图像背景。表格线不是原生 Word 表格，输入长文字不会自动重排其他文本框。

不再把 PDF 的零散文字强行猜成会撑宽、换行、错页的 Word 表格。两种模式均保留页面尺寸，超出 Word 0.1–22 英寸限制时提示失败，不悄悄缩放。一次最多 500 页，图像总量最多 256 MB；保存后请核对。保真指固定页面外观，不保证不同 PDF 渲染器逐像素一致。

自动化工具 `doc_export_office` 支持 DOCX 的 `word_mode: "preserve" | "editable"`，默认 `preserve`；非 DOCX 拒绝此参数。可编辑模式不是完全可重排的原生 Word 正文，不能承诺任意 PDF 的原字体和内容全部可编辑。

## 构建

Rust 稳定工具链，Apple Command Line Tools；`CRAFT_FONTS_DIR` 指向独立 craft-fonts 构建输入，IBM Plex Sans SC 及各字体许可／哈希见 manifest 和 ATTRIBUTION.toml。

```sh
cargo xtask myaipdf-fonts
cargo xtask models
cargo xtask myaipdf-icon
cargo xtask assets --write
CRAFT_FONTS_DIR=/absolute/path/to/craft-fonts cargo build --locked --release -p printcraft -p printcraft-cli -p myaipdf-updater
bash packaging/macos/myaipdf.sh /absolute/path/MyAIPDF.dmg
cargo build -p xtask --release
# 增量脚本使用 target/release/xtask；也可通过 MYAIPDF_XTASK 指定已构建的 xtask。
bash packaging/macos/myaipdf-delta.sh /absolute/base/MyAIPDF.app /absolute/new/MyAIPDF.app /absolute/MyAIPDF-0.1.7-from-0.1.6.delta.dmg
```

## 更新与版本

当前 MyAIPDF 版本 **0.1.7**（底层 PrintCraft 0.2.1）。重点修复 PDF 转 Word 的版式、表格、分页和字体外观，新增明确区分保真与可编辑的导出对话框。保留此前水印分析与选择删除、应用内 AI 工具、证书安全、PS/EPS、中文排版及文字编辑、红紫 DPF 图标。见 [0.1.7 更新说明](packaging/macos/MyAIPDF-0.1.7-release.md)及[验证记录](packaging/macos/MyAIPDF-0.1.7-verification.md)。

去水印：左侧“水印去除” → 选择分析范围 → “分析水印” → 预览位置并勾选 → “删除所选水印” → 确认 → 手动保存或另存为。默认不勾选任何对象；文档更改后必须重新分析。候选不等于已确认水印，也可能是正文、标题或页眉 Logo。扫描图片内的水印不能独立删除，整页扫描背景不会被当作删除目标；这不是保密涂黑，增量 PDF 仍有旧修订数据。

灰色工具本次补齐页面切换、多个 PDF／图片／文本创建、Excel 文字／表格导出、PPT 页面图片导出、扫描背景增强、距离测量与可保存标注、文字／矢量转灰度、裁切标记、显式细线修复和基础 PDF/A 预检。云形、印章、附件和脚本等入口状态修正。严格标准转换、证书加密、可信时间戳、自动标签等尚无完整实现的工具仍明确标示；“可用”不是 Acrobat 等价或标准认证。

“菜单 → 帮助 → 关于 MyAIPDF”底部点击“检查更新”，只查询 [ddxmu/MyAIPDF 正式版本](https://github.com/ddxmu/MyAIPDF/releases)。点击“下载更新”，通过 GitHub SHA-256 校验后再点击“安装并重启”。显示包类型及大小，优先精确版本的增量包，否则选可校验的完整 `MyAIPDF.dmg`。两种安装均校验应用标识、版本和签名，在副本上准备后可恢复地替换，保留旧程序和个人设置。增量模式还检查完整的基础版本、数据和合成结果；基础文件不匹配时停止，不绕过校验。

0.1.1／0.1.2 更新器只识别完整 `MyAIPDF.dmg`，本版以原文件名发布兼容包，重新检查即可显示旧版原有的下载及安装按钮。0.1.3 仍只认识增量格式，因此同时发布 `MyAIPDF-0.1.4-from-0.1.3.delta.dmg`，供其使用原“下载增量更新”入口升级。有未保存 PDF 时不能退出安装；没有启动时自动检查、遥测或静默更新。

发布新版需同步 `crates/update/src/lib.rs` 的 APP_VERSION、macOS 应用及增量助手 Info.plist，运行测试、构建并在真实旧版副本上验证两种升级。以 `v版本号` 创建 GitHub Release；完整包必须命名 `MyAIPDF.dmg` 以兼容旧更新器，增量包命名 `MyAIPDF-新版本-from-基础版本.delta.dmg`。上传校验清单，验证 GitHub digest 后才发布；没有 digest 时禁止自动安装。二进制差分由仅构建期的 MIT qbsdiff 生成，客户端通过系统 bspatch 合成，不执行包内脚本。源码由每个版本的 tag 留存；桌面按版本文件夹保留历史产物。

核心 AI 客户端在 `crates/ai`；真实 PDF 操作复用 `crates/automation` 的验证工具；界面在 `crates/ui-egui/src/ai_ui.rs`。不默认监听端口，不默认启用远程控制。

## 许可与来源

[LICENSE-MIT](LICENSE-MIT)、[LICENSE-APACHE](LICENSE-APACHE)、[NOTICE](NOTICE)、[ATTRIBUTION.md](ATTRIBUTION.md)。新增图标为原创 MIT 资产；字体各自遵循 OFL。其余底层 PDF 引擎、图标、依赖及 OCR 模型遵循各自许可。字体和 OCR 模型来自各自上游，不属于 MyAIPDF 专有资产。
