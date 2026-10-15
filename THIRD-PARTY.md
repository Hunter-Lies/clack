# 第三方组件

本仓库自己的代码用 CC BY-NC-SA 4.0（见 [LICENSE](LICENSE)）。下面这些不属于我们，也没法由我们授权。

## 微软运行库（程序安装的对象）

程序会安装的 Microsoft Visual C++ / Visual Basic / C Runtime 等运行库，
版权归 Microsoft Corporation 所有，按微软自己的可再发行条款分发。

本程序不修改这些二进制，只按微软给出的静默参数依次调用它们的官方安装器。
最终用户拿到的授权来自微软，与本项目的许可协议无关。

**这些二进制不在本仓库里。** 它们由 `tools\stage_payload.ps1` 从微软官方地址下载，
或者由使用者自行放入 `_work\legacy\`。每个文件的来源、签名状态与 SHA256 见
[docs\组件来源与校验.md](docs/组件来源与校验.md)。

## 图标

`assets/logo.svg`、`assets/app-icon.svg` 里的拼图形状取自
[Lucide](https://lucide.dev) 的 `puzzle` 图标，ISC 许可（宽松，可自由使用）。

## 依赖的第三方库

构建时会拉取这些，它们各自的许可见 `ui/pnpm-lock.yaml`、`ui/src-tauri/Cargo.lock`：

- Vue 3、Vite（前端）
- Tauri 2、windows-sys、zip（Rust 后端）
