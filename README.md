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
ui/                        Vue 3 + Vite 前端，Tauri 2 外壳
  src/                     界面（掌机外壳、屏幕、按钮、安装流程）
  src-tauri/               Rust 后端：扫描、解包、调安装器、进度事件
     src/lib.rs            组件表 + 安装逻辑（要读代码从这里开始）
     build.rs              release 构建时写入 requireAdministrator 清单
tools/
  stage_payload.ps1        把官方安装器整理成 payload\（构建前置步骤）
  list_components.ps1      生成 docs\组件来源与校验.md
make_release.ps1           一条命令出单文件 exe
docs/组件来源与校验.md      每个文件的来源、大小、签名、SHA256
```

## 构建

前置：Node 20+、pnpm、Rust（含 MSVC 工具链）、7-Zip。

```powershell
# 1) 准备官方安装器（这一步需要联网）
#    按 docs\组件来源与校验.md 里的官方地址下载，放到 tools\download\
#    老运行库（VB5/VB6/CRT70/71）请自行放入 tools\legacy\，见该文档末尾

# 2) 整理成 payload
.\tools\stage_payload.ps1

# 3) 出单文件 exe（自动内嵌 payload，产物在桌面）
.\make_release.ps1

# 开发调试
cd ui ; pnpm install ; pnpm tauri dev
```

## 注意

- 未签名，首次运行会被 SmartScreen 拦一下（"更多信息 → 仍要运行"）。
- 本程序只做两件事：把缺的运行库装上、把过程写进日志。不联网、不驻留、不写注册表
  以外的系统状态（安装器自己会写注册表，那是微软安装器的正常行为）。

## 许可证

本项目代码 MIT。安装的微软运行库版权归 Microsoft Corporation，按其可再发行条款分发。
详见 [LICENSE](LICENSE)。
