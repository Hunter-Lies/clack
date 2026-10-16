<p align="center">
  <img src="assets/logo.png" width="96" alt="咔哒 Clack">
</p>

<h1 align="center">咔哒 / Clack</h1>

<p align="center">Windows 单文件运行库合集</p>

<p align="center">
  <img src="https://img.shields.io/badge/License-CC%20BY--NC--SA%204.0-e9e1cf?style=flat-square" alt="License: CC BY-NC-SA 4.0">
  <img src="https://img.shields.io/badge/Windows-10%20%7C%2011%20x64-e9e1cf?style=flat-square" alt="Windows 10 / 11 x64">
</p>

<table>
<tr>
<td width="230" valign="top"><img src="assets/screenshot.png" width="210" alt="界面"></td>
<td valign="top">

用于补齐 Windows 上缺失的 Microsoft 运行库。程序启动报 `缺少 msvcr100.dll`、`缺少 msvcp140.dll`，通常由运行库缺失引起。

x86 与 x64 一并安装：32 位程序读取 `SysWOW64`，64 位程序读取 `System32`，两套文件互不通用。

全程离线运行，不驻留、不自启、不注册服务。

</td>
</tr>
</table>

## 组件

| 组件 | 版本 | 架构 | 安装方式 |
|---|---|---|---|
| Visual Basic VM 5.x | 5.02.8244 | x86 | 复制 DLL |
| Visual Basic VM 6.0 | 6.00.9802 | x86 | 复制 DLL |
| C Runtime 2002 | 7.00.9981 | x86 | 复制 DLL |
| C Runtime 2003 | 7.10.3078 | x86 | 复制 DLL |
| Visual C++ 2005 SP1 | 8.0.61001 | x86 + x64 | 微软安装器 |
| Visual C++ 2008 SP1 | 9.0.30729.6161 | x86 + x64 | 微软安装器 |
| Visual C++ 2010 SP1 | 10.0.40219.473 | x86 + x64 | 微软安装器，含 KB2565063 |
| Visual C++ 2012 U4 | 11.0.61135 | x86 + x64 | 微软安装器 |
| Visual C++ 2013 | 12.0.40664 | x86 + x64 | 微软安装器 |
| Visual C++ 2015-2022 | 14.51.36247 | x86 + x64 | 微软安装器 |

## 使用

1. 从 [Releases](https://github.com/Hunter-Lies/clack/releases) 下载 `Clack-1.1.0-x64.exe`
2. 双击运行，在 UAC 提示中允许提权
3. 点击安装，等待十项组件全部完成

删除 exe 即为卸载程序本体，已安装的运行库走正常卸载流程。

## 系统改动

| 位置 | 说明 |
|---|---|
| `C:\ProgramData\Click\payload\<版本>\` | 解出的运行库，约 60 MB；只在运行期间存在，关窗口时删掉 |
| `C:\ProgramData\Click\<组件>\` | 安装时临时拷过去的 MSI / CAB，退出时删掉，只留 `install.log` |
| `%TEMP%\Click.log` | 运行日志，每次启动重写一份 |
| `System32` / `SysWOW64` / `WinSxS` | 由微软安装器写入 |

退出时会连临时安装包一起清掉，合计约 100 MB；各组件目录里的 `install.log` 留着备查
——那是 msiexec 的日志。被任务管理器强杀时来不及清，重开一次再正常关掉即可。

彻底清理：删除 exe 与 `C:\ProgramData\Click\`，已安装的运行库在「应用和功能」中卸载。

## 常见问题

| 现象 | 说明 |
|---|---|
| 提示「已保护你的电脑」 | 未使用代码签名证书，选择「更多信息」→「仍要运行」 |
| 是否需要重启 | 通常不需要 |
| 安装过程中无法关闭窗口 | 设计如此；进程无响应时用任务管理器结束，再查看 `%TEMP%\Click.log` |
| 系统要求 | Windows 10 / 11 x64，界面依赖 WebView2 |

## 构建

依赖 Node 20+、pnpm、Rust（含 MSVC 工具链）、7-Zip。

```powershell
# 官方安装器放入 _work\download\，历史运行库放入 _work\legacy\（清单见 tools\legacy\README.md）
.\tools\stage_payload.ps1     # 整理为 _work\payload
.\make_release.ps1            # 生成单文件 exe，产物在 out\

cd ui ; pnpm install ; pnpm tauri dev     # 开发调试
```

发布前自检：`tools\check_deps.ps1`（确认 exe 不依赖运行库）、`tools\check_repo.ps1`（确认仓库无本机信息与二进制）。

组件清单见 [ui/src-tauri/src/lib.rs](ui/src-tauri/src/lib.rs)，各文件来源与 SHA256 见 [docs/组件来源与校验.md](docs/组件来源与校验.md)，设计取舍见 [docs/设计取舍.md](docs/设计取舍.md)。

## 技术栈

Tauri 2 + Rust 后端，WebView2 承载 Vue 3 界面，Vite 构建：Tauri `2.12.4`（MIT / Apache-2.0）、Vue `3.5.43`（MIT）、Vite `8.3.4`（MIT，仅构建期）。

第三方组件的许可与版权见 [THIRD-PARTY.md](THIRD-PARTY.md)。

## 许可

本仓库代码采用 [CC BY-NC-SA 4.0](LICENSE)：署名、非商业、相同方式共享。

注意这是**源码公开**，不是 OSI 认可的开源许可——非商业性使用这条限制，让它不符合开源定义。

图标取自 [Lucide](https://lucide.dev)（ISC）；程序安装的微软运行库版权归 Microsoft，不在本仓库内。

只想补 DirectX 的话看另一个：[咔哒 DX / Clack DX](https://github.com/Hunter-Lies/clack-dx)。
