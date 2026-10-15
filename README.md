<p align="center">
  <img src="assets/logo.png" width="96" alt="咔哒 Clack">
</p>

<h1 align="center">咔哒 / Clack</h1>

<p align="center">Windows 单文件运行库合集<br>双击就能用，不用安装，用完删掉就行</p>

<p align="center">
  <img src="https://img.shields.io/badge/License-MIT-e9e1cf?style=flat-square" alt="License: MIT">
  <img src="https://img.shields.io/badge/Windows-10%20%7C%2011%20x64-e9e1cf?style=flat-square" alt="Windows 10 / 11 x64">
  <img src="https://img.shields.io/badge/%E5%8D%95%E6%96%87%E4%BB%B6-63.6%20MB-e9e1cf?style=flat-square" alt="63.6 MB">
</p>

<table>
<tr>
<td width="230" valign="top"><img src="assets/screenshot.png" width="210" alt="界面"></td>
<td valign="top">

### 这是什么

一个 exe，把 Windows 上那些"缺了就打不开游戏"的运行库补齐。老游戏报 `缺少 msvcr100.dll`、`缺少 msvcp140.dll`，基本都是这个原因。

十个组件：Visual Basic VM 5.x / 6.0、C Runtime 2002 / 2003、Visual C++ 2005 SP1、2008 SP1、2010 SP1（含 KB2565063）、2012 U4、2013、2015-2022（14.51）。

**x86 和 x64 都装**。32 位程序读的是 `SysWOW64`、64 位读 `System32`，两套是分开的，而老游戏大多是 32 位——只装 64 位它照样缺库。

安装过程完全静默，不会弹出原版安装器那些窗口。

</td>
</tr>
</table>

## 怎么用

1. 去 [Releases](https://github.com/Hunter-Lies/clack/releases) 下载 `Clack-1.0.0-x64.exe`
2. 双击。UAC 会问一次，点"是"——往系统目录里写东西，管理员权限是跑不掉的
3. 按那个橙色按钮，等十项全部打上勾，按「完成」收工

删掉 exe 就算卸载干净了。程序不驻留、不开机自启、不注册服务。

## 它动了什么

| | |
|---|---|
| 程序自己 | 就一个 exe，放哪都行 |
| 解出来的运行库 | `C:\ProgramData\Click\payload\<版本>\`，60 多 MB，第一次运行解一次，之后复用 |
| 日志 | `%TEMP%\Click.log`；每个组件的 msiexec 详细日志在 `C:\ProgramData\Click\<组件>\install.log` |
| 系统改动 | 全部由微软安装器完成：System32 / SysWOW64 / WinSxS，以及「应用和功能」里能看到、能正常卸载的那些项 |
| 网络 | 不用。运行库整个内嵌在 exe 里，安装过程纯本地 |

想彻底清掉：删 exe + 删 `C:\ProgramData\Click\`，装上的运行库去「应用和功能」里卸。

## 常见问题

**Windows 提示"已保护你的电脑"** —— 没买代码签名证书，这个提示消不掉。点「更多信息」→「仍要运行」。

**装完要重启吗** —— 一般不用。个别组件（2010 那个补丁）可能会要求重启，重启一下更保险。

**安装中关不掉窗口** —— 这是故意的。真卡死了用任务管理器结束进程，然后看 `%TEMP%\Click.log` 停在哪一项。

**支持哪些系统** —— 只做 Windows 10 / 11 x64。界面用 WebView2 渲染：Win11 自带，Win10 跟着 Edge 一起装上了。

## 自己核一遍

要往系统目录写东西的程序，跑之前自己验一遍才踏实：

```powershell
.\tools\check_deps.ps1     # 本程序自己不依赖任何运行库（解析 PE 导入表）
.\tools\check_repo.ps1     # 仓库里没有本机痕迹、没有二进制
```

- [docs/组件来源与校验.md](docs/组件来源与校验.md)：每个文件的来源、签名状态、SHA256
- [ui/src-tauri/src/lib.rs](ui/src-tauri/src/lib.rs)：组件表——装哪几个、什么版本、什么参数，全在这一个文件里

仓库里只有源码，没有任何可执行文件：微软的运行库都在 `.gitignore` 排除的工作目录里，需要时按下面的步骤重新下载。

## 从源码构建

需要 Node 20+、pnpm、Rust（含 MSVC 工具链）、7-Zip。

```powershell
# 官方安装器下到 _work\download\，老运行库自备到 _work\legacy\（清单见 tools\legacy\README.md）
.\tools\stage_payload.ps1     # 整理成 _work\payload
.\make_release.ps1            # 出单文件 exe，产物在 out\

# 开发调试
cd ui ; pnpm install ; pnpm tauri dev
```

设计上的取舍——为什么不做"已装"检测、为什么 MSI 要先拷到纯 ASCII 路径、为什么不带 VSTOR——记在 [docs/设计取舍.md](docs/设计取舍.md)。

## 许可证

本仓库代码 [MIT](LICENSE)。

图标是 [Lucide](https://lucide.dev) 的 `puzzle`（ISC 许可）。

程序安装的 Microsoft Visual C++ / Visual Basic 运行库，版权归 Microsoft Corporation，按其可再发行条款分发；这些二进制不包含在本仓库里。
