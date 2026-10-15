# 扫一遍 payload\，生成 docs\组件来源与校验.md：
# 每个文件的相对路径、大小、微软签名状态、SHA256。
# 用户审计时看这一份就够了——证明我们发的就是微软原版，没有夹带。
[CmdletBinding()]
param([string]$Root = (Split-Path -Parent $PSScriptRoot))

$ErrorActionPreference = 'Stop'
$Payload = Join-Path $Root 'payload'
$DocDir = Join-Path $Root 'docs'
$Doc = Join-Path $DocDir '组件来源与校验.md'

if (-not (Test-Path $Payload)) { throw "找不到 payload\，先跑 tools\stage_payload.ps1" }
New-Item -ItemType Directory -Force $DocDir | Out-Null

$files = Get-ChildItem $Payload -Recurse -File | Sort-Object FullName
$signed = 0
$rows = foreach ($f in $files) {
    $sig = Get-AuthenticodeSignature $f.FullName
    $isMs = $sig.Status -eq 'Valid' -and $sig.SignerCertificate.Subject -match 'Microsoft'
    if ($isMs) { $signed++ }
    '| `{0}` | {1:N0} | {2} | `{3}` |' -f `
        $f.FullName.Replace("$Payload\", ''), $f.Length,
        $(if ($isMs) { '微软签名' } else { '无签名' }),
        (Get-FileHash $f.FullName -Algorithm SHA256).Hash.ToLower()
}

$body = @"
# 组件来源与校验

这份文件由 ``tools\list_components.ps1`` 生成，数据直接读自 ``payload\``。

## 怎么自己核一遍

1. 按下面表格里的 SHA256 逐个比对 ``payload\`` 里的文件
2. 或者直接查签名：安装器（.msi / .exe / .cab）都是 Microsoft Corporation 签名的

   `````powershell
   Get-AuthenticodeSignature .\payload\10_VC2015-2022_14.51\x64\vc_redist.x64.exe
   `````

签名通过的那些不需要看来源——文件本身就是微软原版。
2002/2003 那两代 dll、以及 VB5/VB6 运行时没有签名，那个年代 Authenticode 还没普及，
微软自己发的也没有；它们带完整的微软版本资源，可以用 SHA256 跟官方包里的对照。

## 文件清单

共 $($files.Count) 个文件，其中 **$signed 个带微软签名**。

| 路径 | 字节 | 签名 | SHA256 |
|---|---|---|---|
$($rows -join "`n")

## 各组件官方来源

| 组件 | 官方来源 | 静默参数 |
|---|---|---|
| VB5 运行时 | VB5 SP3 运行时包 | 复制到 SysWOW64 |
| VB6 运行时 | VB6 SP6 运行时（vbrun60sp6.exe） | 复制到 SysWOW64 |
| C Runtime 2002 | .NET Framework 1.0 运行时 / VS2002 SP1 | 复制到 SysWOW64 |
| C Runtime 2003 | .NET Framework 1.1 运行时 / VS2003 SP1 | 复制到 SysWOW64 |
| Visual C++ 2005 SP1 | download.microsoft.com 8B42259F-5D70-43F4-AC2E-4B208FD8D66A | 拆出 MSI+CAB 后 ``msiexec /qn`` |
| Visual C++ 2008 SP1 | download.microsoft.com 5D8C65CB-C849-4025-8E95-C3966CAFD8AE | 同上 |
| Visual C++ 2010 SP1 | download.microsoft.com 165255E7-1014-4D0A-B094-B6A430A6BFFC | 同上，再打 KB2565063 |
| Visual C++ 2012 U4 | download.microsoft.com 16B06F60-3B20-4FF2-B699-5E9B7962F9AE | ``/install /quiet /norestart`` |
| Visual C++ 2013 | download.microsoft.com 2E61CFA4-993B-4DD4-91DA-3737CD5CD6E3 | 同上 |
| Visual C++ 2015-2022 | aka.ms/vs/18/release/vc_redist.x64.exe | 同上 |

老运行库（前三行）微软官网的下载页大多已经下架，可以从对应的官方运行时包里取；
取不到的话，任一微软签名的近版本运行库包里的同名文件都可以用——它们本身就是最终产物。
"@

Set-Content -LiteralPath $Doc -Value $body -Encoding UTF8
Write-Host ("已生成 {0}：{1} 个文件，其中 {2} 个微软签名" -f $Doc, $files.Count, $signed)
