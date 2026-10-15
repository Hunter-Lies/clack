# 咔哒 / Clack

一个单文件的运行库合集安装器。双击即用、不需要安装，界面做成一台掌机：一块屏幕，一个按钮。

- 目标系统：**Windows 10 及以上（x64）**
- 启动即请求管理员权限（装运行库必须有）
- 全过程静默：**不会弹出任何原版安装器的窗口**
- 安装期间锁住关闭键，同时拦截 Alt+F4 和任务栏"关闭窗口"
- 出问题可查日志：`%TEMP%\Click.log`，以及 `C:\ProgramData\Click\<组件>\install.log`（msiexec 的详细日志）

## 装了什么

10 个组件，来源与校验见 [docs/组件来源与校验.md](docs/组件来源与校验.md)。

| 组件 | 版本 | 架构 | 装法 |
|---|---|---|---|
| Visual Basic VM 5.x | 5.02.8244 | x86 | 复制 dll 到 SysWOW64 |
| Visual Basic VM 6.0 | 6.00.9802 | x86 | 同上 |
| C Runtime 2002 | 7.00.9981 | x86 | 同上 |
| C Runtime 2003 | 7.10.3078 | x86 | 同上 |
| Visual C++ 2005 SP1 | 8.0.61001 | x86 / x64 | MSI + CAB，`msiexec /qn` |
| Visual C++ 2008 SP1 | 9.0.30729.6161 | x86 / x64 | 同上 |
| Visual C++ 2010 SP1 | 10.0.40219.473 | x86 / x64 | 同上，再打 KB2565063 补丁 |
| Visual C++ 2012 U4 | 11.0.61030 | x86 / x64 | 官方 vc_redist，`/install /quiet /norestart` |
| Visual C++ 2013 | 12.0.40664 | x86 / x64 | 同上 |
| Visual C++ 2015-2022 | 14.51.36247 | x86 / x64 | 同上 |

## 为什么这么设计

这些是踩出来的，写在这里省得后来人再踩一遍。

**1. 装运行库必须管理员权限。** 所以 exe 的清单里直接写 `requireAdministrator`
（`src-tauri/build.rs`），Windows 在进程启动前就完成提权，不会出现"先开一个普通窗口、
再重启自己"的闪一下。开发构建**不加**这个清单，否则每次改代码重建都要点一次 UAC。

**2. 不做"装没装"的检测，全部照装。** 安装器本身是幂等的（已装会返回 1638），
而检测一旦判错，用户拿到的就是一台依然缺库的机器——漏装的代价远大于多花几十秒。
唯一的例外是散装 dll（VB/CRT70/71）只补缺：正在被程序加载的系统 dll 覆盖会失败（共享冲突），
而这一类"文件在就能用"。

**3. MSI 不能从非 ASCII 路径安装。** Windows Installer 服务是独立进程，包路径要在它内部
过一遍 ANSI 转换，中文路径到那儿就成乱码，`msiexec` 只会回 **1619（安装包打不开）**。
所以程序会先把 MSI 连同它的 CAB 拷到 `C:\ProgramData\Click\<组件>\` 再装——这样跟我们自己
装在哪儿、用户把程序放哪儿都无关。

**4. 每一代安装器的静默开关都不一样。** 2005 只认 `/Q`；2008/2010 是 `/Q /NORESTART`；
2012 之后才是 `/install /quiet /norestart`。参数在 `src-tauri/src/lib.rs` 的组件表里逐条配。

**5. "静默"不等于没有界面。** 老的自解压包用 `/Q` 只是自己不出声，它内部还会再调一次
`msiexec` 弹进度框。所以 2005/2008/2010 是**把包拆开、自己调 `msiexec /qn`**，
只有这样才是真的一点窗口都没有。

**6. x86 和 x64 都要装。** 32 位程序读 `SysWOW64`、64 位读 `System32`，两套是分开的。
老游戏和老工具绝大多数是 32 位，只装 64 位的话它们照样缺库。

**7. 不含 VSTOR（VS 2010 Tools for Office）。** 那是给老 Office 的 VSTO 插件用的运行时，
跟游戏无关；而且它在 64 位系统上装不上：x64 那份的 MSI 要同目录有 `install.ini`（缺了报 1308），
x86 那份的 MSI 里写死了 `CA_BlockOnAMD64` 直接拒绝在 64 位安装。

**8. 单文件。** 运行库整个以 zip 形式内嵌在 exe 里，首次运行解到
`C:\ProgramData\Click\payload\<版本>\`，之后复用。所以发出去就是一个文件，
不需要安装程序、也没有卸载器。

## 目录结构

```
ui/                            这是程序本体：Vue 3 + Vite 前端，Tauri 2 外壳
  src/                         界面（掌机外壳、屏幕、按钮、安装流程）
  src-tauri/                   Rust 后端：扫描、解包、调安装器、进度事件
     src/lib.rs                组件表 + 安装逻辑（要读代码从这里开始）
     build.rs                  release 构建时写入 requireAdministrator 清单
tools/
  stage_payload.ps1            把官方安装器整理成 _work\payload（构建前置步骤）
  list_components.ps1          生成 docs\组件来源与校验.md
  check_deps.ps1               确认 exe 自己不依赖运行库
  check_repo.ps1               确认仓库没有本机痕迹和二进制
make_release.ps1               一条命令出单文件 exe
docs/组件来源与校验.md          每个文件的来源、大小、签名、SHA256
_work/                         工作文件（不进仓库，见下）
  download/                    从微软官方下载的安装器
  legacy/                      老运行库，需要自备，见 tools\legacy\README.md
  extracted/                   自解压包拆出来的缓存
  payload/                     整理好的料，会被打进 exe
```

仓库里只有源码和脚本，几百 KB。`_work\` 整个目录在 `.gitignore` 里，
里面是微软的二进制和构建中间产物。

## 构建

前置：Node 20+、pnpm、Rust（含 MSVC 工具链）、7-Zip。

```powershell
# 1) 准备官方安装器（这一步需要联网）
#    按 docs\组件来源与校验.md 里的官方地址下载，放到 _work\download\
#    老运行库（VB5/VB6/CRT70/71、2012/2013 redist）请自行放入 _work\legacy\，
#    清单见 tools\legacy\README.md

# 2) 整理成 _work\payload
.\tools\stage_payload.ps1

# 3) 出单文件 exe（自动内嵌 payload，产物在 out\）
.\make_release.ps1

# 开发调试
cd ui ; pnpm install ; pnpm tauri dev
```

**改完图标要记得**：`icons\` 换了之后直接构建，exe 里嵌的图标**不会更新**——
tauri-build 的 build script 只监视 `tauri.conf.json`，资源脚本不会重跑。两个办法：

```powershell
# 要么 touch 一下配置，逼 build script 重跑
(Get-Item ui\src-tauri\tauri.conf.json).LastWriteTime = Get-Date

# 要么连构建产物一起清
cargo clean -p click    # 在 ui\src-tauri 下
```

判断有没有生效：从 exe 里把图标抠出来看一眼，别看资源管理器（它有图标缓存，
同一个路径会给你看旧的）。

## 它自己不需要运行库

这点很重要，也是可以自己验的：运行库安装器如果在没运行库的机器上起不来，那就没意义。

```powershell
.\tools\check_deps.ps1
```

`click.exe` 的导入表里只有系统 DLL（ntdll / kernel32 / user32 / comctl32 …）和
**UCRT**（`api-ms-win-crt-*`，Windows 10 起系统自带），没有 `VCRUNTIME140.dll`、
没有 `MSVCP140.dll`——也就是它装的那些运行库，它自己一个都不用。

唯一的运行时依赖是 **WebView2**（界面由它渲染）：Windows 11 内置，
Windows 10 随 Edge 自动装上。万一缺失，Tauri 会弹提示让你去装，不会静默失败。

## 自己核一遍

这个程序要往系统目录里装东西，所以每一条都应该是能自己验的：

```powershell
.\tools\check_deps.ps1     # 本程序自己不依赖任何运行库（解析 PE 导入表）
.\tools\check_repo.ps1     # 仓库里没有本机痕迹、没有微软二进制
```

再看这两份文档：

- **[docs\组件来源与校验.md](docs/组件来源与校验.md)** —— 每个二进制文件的来源、大小、
  签名状态和 SHA256。39 个文件里带 Microsoft Corporation 签名的都是微软原版，
  剩下的老 dll（2002/2003、VB5/VB6）那个年代 Authenticode 还没普及，微软自己发的也没签。
- **`ui\src-tauri\src\lib.rs`** —— 组件表。装哪几个、什么版本、什么参数、
  从哪个目录取，全在这一个文件里，不需要翻别处。

## 它跑在哪、写了什么

| | |
|---|---|
| 程序本体 | **单文件 exe，放哪都行**。不安装、不注册服务、不写开始菜单 |
| 解出的运行库 | `C:\ProgramData\Click\payload\<版本>\`（约 67 MB，首次运行解一次，之后复用） |
| 日志 | `%TEMP%\Click.log`；每个组件的 msiexec 详细日志在 `C:\ProgramData\Click\<组件>\install.log` |
| 对系统的改动 | **全部由微软安装器完成**：System32 / SysWOW64 / WinSxS、以及"应用和功能"里的卸载项 |
| 网络 | **不联网**。运行库整个内嵌在 exe 里，安装过程纯本地 |
| 开机自启 | 无 |
| 卸载 | 删掉 exe 和 `C:\ProgramData\Click\`；装上的运行库去"应用和功能"里逐个卸载（那是微软安装器的正常卸载） |

## 仓库里有什么、没有什么

**有**：界面源码、Rust 后端、四个 PowerShell 脚本、文档、图标。加起来几百 KB。

**没有**（都在 `.gitignore` 里）：

| 没有的东西 | 为什么 | 怎么补 |
|---|---|---|
| `_work\payload\` | 微软的可再发行组件，67 MB | 跑 `tools\stage_payload.ps1` 自动从官方地址下载并整理 |
| `_work\download\` | 官方安装器下载缓存 | 同上 |
| `_work\legacy\` | 老运行库，微软官网已下架 | 自备，清单见 `tools\legacy\README.md` |
| `_work\extracted\` | 拆包缓存 | 脚本自动生成 |
| `ui\src-tauri\payload.zip` | 内嵌进 exe 的料，构建时生成 | 跑 `make_release.ps1` 自动生成 |
| `ui\node_modules`、`ui\src-tauri\target` | 依赖和构建产物，4 GB | `pnpm install` / `cargo build` |

所以这个仓库本身**不含任何可执行文件**：读者读到的全是源码，要二进制就去 Release 拿，
或者自己按上面的步骤构建。

## 注意

- 未签名，首次运行会被 SmartScreen 拦一下（"更多信息 → 仍要运行"）。
- 启动即请求管理员权限，因为写系统目录是必须的；UAC 会问一次。
- 本程序只做两件事：把缺的运行库装上、把过程写进日志。不联网、不驻留、不自启。
  系统层面的改动（系统目录、WinSxS、注册表卸载项）都是微软安装器写的，
  装完能在"应用和功能"里看到它们，也能正常卸载。

## 许可证

本项目代码 MIT。安装的微软运行库版权归 Microsoft Corporation，按其可再发行条款分发。
详见 [LICENSE](LICENSE)。

图标：[Lucide](https://lucide.dev) 的 `puzzle`（ISC 许可，无需署名；底色与配色自绘）。
