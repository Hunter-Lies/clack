# 把各来源的运行库统一收集到 payload\ 目录树，并生成 manifest.csv
# 目标系统：Windows 10 及以上（不含 Win7/Win8/XP）
#   · 2015-2022 只带最新版 14.51，不需要 14.44（那条是给 Win7 的）
#   · UCRT 在 Win10 里是系统组件，不需要单独的 msu
# 来源：
#   _src\2020        从「微软常用运行库合集 2020.11.11.exe」拆出来的官方安装包
#   tools\download   从微软官方直接下载的最新版
[CmdletBinding()]
param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot)
)

# 老运行库（VB5/VB6、2002/2003 的 CRT/MFC/ATL）微软官网已经下架，
# 需要使用者自己放一份到 tools\legacy\ —— 见那个目录里的说明。
# 其余组件都由本脚本从微软官方地址下载后处理。
$ErrorActionPreference = 'Stop'
$Legacy = Join-Path $Root 'tools\legacy'
$Dl     = Join-Path $Root 'tools\download'
$Out    = Join-Path $Root 'payload'

$SevenZip = @(
    'C:\Program Files\7-Zip\7z.exe'
    'C:\Program Files (x86)\7-Zip\7z.exe'
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $SevenZip) { throw '需要 7-Zip：装一个，或把 7z.exe 放到 PATH 里' }

if (Test-Path $Out) { Remove-Item -LiteralPath $Out -Recurse -Force }
New-Item -ItemType Directory -Force $Out | Out-Null

# 组件表：目标目录 / 架构 / 来源文件 / 显示名 / 版本 / 类型 / 静默参数 / 适用系统
$items = @(
    @{ Dir='01_VB5_VM';   Arch='x86'; Src="$Legacy\MSVBVM50.DLL";            Name='Visual Basic Virtual Machine 5.x'; Ver='05.02.8244';     Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='02_VB6_VM';   Arch='x86'; Src="$Legacy\MSVBVM60.DLL";            Name='Visual Basic Virtual Machine 6.0'; Ver='6.00.9802';      Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='03_CRT_2002'; Arch='x86'; Src="$Legacy\MSVCR70.DLL";             Name='Microsoft C Runtime Library 2002'; Ver='7.00.9981';      Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='03_CRT_2002'; Arch='x86'; Src="$Legacy\MSVCP70.DLL";             Name='Microsoft C++ Runtime Library 2002'; Ver='7.00.9466';    Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='03_CRT_2002'; Arch='x86'; Src="$Legacy\atl70.dll";               Name='ATL 2002'; Ver='7.00.9975';      Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='03_CRT_2002'; Arch='x86'; Src="$Legacy\mfc70.dll";               Name='MFC 2002'; Ver='7.00.9975';      Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='03_CRT_2002'; Arch='x86'; Src="$Legacy\mfc70u.dll";              Name='MFC 2002 (Unicode)'; Ver='7.00.9975'; Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='03_CRT_2002'; Arch='x86'; Src="$Legacy\mfc70chs.dll";            Name='MFC 2002 简体中文资源'; Ver='7.00.9975'; Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='04_CRT_2003'; Arch='x86'; Src="$Legacy\MSVCR71.DLL";             Name='Microsoft C Runtime Library 2003'; Ver='7.10.3078';      Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='04_CRT_2003'; Arch='x86'; Src="$Legacy\MSVCP71.DLL";             Name='Microsoft C++ Runtime Library 2003'; Ver='7.10.6052';    Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='04_CRT_2003'; Arch='x86'; Src="$Legacy\atl71.dll";               Name='ATL 2003'; Ver='7.10.6119';      Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='04_CRT_2003'; Arch='x86'; Src="$Legacy\MFC71.DLL";               Name='MFC 2003'; Ver='7.10.6119';      Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='04_CRT_2003'; Arch='x86'; Src="$Legacy\mfc71u.dll";              Name='MFC 2003 (Unicode)'; Ver='7.10.6119'; Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    @{ Dir='04_CRT_2003'; Arch='x86'; Src="$Legacy\mfc71chs.dll";            Name='MFC 2003 简体中文资源'; Ver='7.10.6119'; Kind='DLL'; Silent='复制到系统目录'; OS='XP+' }
    # 2005/2008/2010：合包里拆出来的 MSI 是"半成品"（引用外部 .msm / vc_red.cab，而合包里没有），
    # 所以换成官方自解压包，再把里面的 MSI + CAB 抠出来直接喂 msiexec /qn ——
    # 不让自解压包自己跑，是因为它内部还会调 msiexec 弹进度框；我们直接调就是全程无界面。
    @{ Dir='05_VC2005';   Arch='x86'; Src="$Dl\vcredist2005_x86.exe"; Extract=@('vcredist.msi','vcredis1.cab'); Name='Microsoft Visual C++ 2005 SP1'; Ver='8.0.61001'; Kind='MSI'; Silent='msiexec /i vcredist.msi /qn /norestart'; OS='XP+' }
    @{ Dir='05_VC2005';   Arch='x64'; Src="$Dl\vcredist2005_x64.exe"; Extract=@('vcredist.msi','vcredis1.cab'); Name='Microsoft Visual C++ 2005 SP1 (x64)'; Ver='8.0.61001'; Kind='MSI'; Silent='msiexec /i vcredist.msi /qn /norestart'; OS='XP x64+' }
    @{ Dir='06_VC2008';   Arch='x86'; Src="$Dl\vcredist2008_x86.exe"; Extract=@('vc_red.msi','vc_red.cab'); Name='Microsoft Visual C++ 2008 SP1'; Ver='9.0.30729.6161'; Kind='MSI'; Silent='msiexec /i vc_red.msi /qn /norestart'; OS='XP+' }
    @{ Dir='06_VC2008';   Arch='x64'; Src="$Dl\vcredist2008_x64.exe"; Extract=@('vc_red.msi','vc_red.cab'); Name='Microsoft Visual C++ 2008 SP1 (x64)'; Ver='9.0.30729.6161'; Kind='MSI'; Silent='msiexec /i vc_red.msi /qn /norestart'; OS='XP x64+' }
    @{ Dir='07_VC2010';   Arch='x86'; Src="$Dl\vcredist2010_x86.exe"; Extract=@('vc_red.msi','vc_red.cab','msp_kb2565063.msp'); Name='Microsoft Visual C++ 2010 SP1'; Ver='10.0.40219.473'; Kind='MSI'; Silent='msiexec /i vc_red.msi /qn，再打 msp_kb2565063.msp'; OS='XP+' }
    @{ Dir='07_VC2010';   Arch='x64'; Src="$Dl\vcredist2010_x64.exe"; Extract=@('vc_red.msi','vc_red.cab','msp_kb2565063.msp'); Name='Microsoft Visual C++ 2010 SP1 (x64)'; Ver='10.0.40219.473'; Kind='MSI'; Silent='msiexec /i vc_red.msi /qn，再打 msp_kb2565063.msp'; OS='XP x64+' }
    @{ Dir='08_VC2012';   Arch='x86'; Src="$Legacy\vc_redist2012_x86.exe";  Name='Microsoft Visual C++ 2012 U4'; Ver='11.0.61030';    Kind='EXE'; Silent='/install /quiet /norestart'; OS='Vista+' }
    @{ Dir='08_VC2012';   Arch='x64'; Src="$Legacy\vc_redist2012_x64.exe";  Name='Microsoft Visual C++ 2012 U4 (x64)'; Ver='11.0.61030'; Kind='EXE'; Silent='/install /quiet /norestart'; OS='Vista x64+' }
    @{ Dir='09_VC2013';   Arch='x86'; Src="$Legacy\vc_redist2013_x86.exe";  Name='Microsoft Visual C++ 2013'; Ver='12.0.40664';       Kind='EXE'; Silent='/install /quiet /norestart'; OS='Vista+' }
    @{ Dir='09_VC2013';   Arch='x64'; Src="$Legacy\vc_redist2013_x64.exe";  Name='Microsoft Visual C++ 2013 (x64)'; Ver='12.0.40664'; Kind='EXE'; Silent='/install /quiet /norestart'; OS='Vista x64+' }
    @{ Dir='10_VC2015-2022_14.51'; Arch='x86'; Src="$Dl\vc_redist_vs18_x86.exe"; As='vc_redist.x86.exe'; Name='Microsoft Visual C++ 2015-2022（最新）'; Ver='14.51.36247'; Kind='EXE'; Silent='/install /quiet /norestart'; OS='Win10+' }
    @{ Dir='10_VC2015-2022_14.51'; Arch='x64'; Src="$Dl\vc_redist_vs18_x64.exe"; As='vc_redist.x64.exe'; Name='Microsoft Visual C++ 2015-2022（最新, x64）'; Ver='14.51.36247'; Kind='EXE'; Silent='/install /quiet /norestart'; OS='Win10 x64+' }
    # 不含 VSTOR（VS 2010 Tools for Office）：那是老 Office 的 VSTO 插件运行时，与游戏无关；
    # 而且在 64 位系统上装不上（x64 缺 install.ini → 1308，x86 被 MSI 自己拒绝）。
)

$manifest = New-Object System.Collections.Generic.List[object]
foreach ($it in $items) {
    if (-not (Test-Path -LiteralPath $it.Src)) {
        Write-Warning ("缺少来源文件，跳过: {0}" -f $it.Src)
        continue
    }
    $dstDir = Join-Path $Out (Join-Path $it.Dir $it.Arch)
    New-Item -ItemType Directory -Force $dstDir | Out-Null

    if ($it.Extract) {
        # 自解压包：先缓存拆包结果，再把需要的文件挑出来。
        # 拆出来的是 MSI + 它的 CAB（或者自带的子安装器），这样能跳过原包的界面。
        $cache = Join-Path $Root ("tools\extracted\" + [IO.Path]::GetFileNameWithoutExtension($it.Src))
        if (-not (Test-Path $cache)) {
            New-Item -ItemType Directory -Force $cache | Out-Null
            & $SevenZip x $it.Src "-o$cache" -y | Out-Null
        }
        foreach ($f in $it.Extract) {
            $from = Join-Path $cache $f
            if (-not (Test-Path $from)) { throw "拆包结果里没有 $f（$($it.Src)）" }
            if ($from -like '*.exe') {
                # 还有一层壳（VSTOR 这类）：再拆一次，把里面的 MSI + CAB 一起拿出来
                $sub = Join-Path $Root ("tools\extracted\" + [IO.Path]::GetFileNameWithoutExtension($from))
                if (-not (Test-Path $sub)) {
                    New-Item -ItemType Directory -Force $sub | Out-Null
                    & $SevenZip x $from "-o$sub" -y | Out-Null
                }
                # 一并带上 install.ini：VSTOR 的 MSI 会去同目录找它，缺了就报 1308
                Get-ChildItem $sub -File | Where-Object { $_.Extension -in '.msi', '.cab', '.ini' } |
                    Copy-Item -Destination $dstDir -Force
            }
            else {
                Copy-Item -LiteralPath $from -Destination (Join-Path $dstDir (Split-Path -Leaf $f)) -Force
            }
        }
        $leaf = ($it.Extract | ForEach-Object { Split-Path -Leaf $_ }) -join ' + '
    }
    else {
        # 官方包的文件名各代都不一样（vc_redist_vs18_x86.exe …），按 As 统一命名
        $leaf = if ($it.As) { $it.As } else { Split-Path -Leaf $it.Src }
        Copy-Item -LiteralPath $it.Src -Destination (Join-Path $dstDir $leaf) -Force
    }
    $manifest.Add([pscustomobject]@{
        组件      = $it.Name
        版本      = $it.Ver
        架构      = $it.Arch
        类型      = $it.Kind
        目录      = "$($it.Dir)\$($it.Arch)"
        文件      = $leaf
        字节      = (Get-ChildItem -LiteralPath $dstDir | Measure-Object Length -Sum).Sum
        静默参数  = $it.Silent
        适用系统  = $it.OS
    })
}

$csv = Join-Path $Out 'manifest.csv'
$manifest | Export-Csv -LiteralPath $csv -NoTypeInformation -Encoding UTF8

Write-Host ("已收集 {0} 个组件文件 -> {1}" -f $manifest.Count, $Out)
Write-Host ("清单: {0}" -f $csv)
$manifest | Format-Table 组件, 版本, 架构, 目录 -AutoSize
