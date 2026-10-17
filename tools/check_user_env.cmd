@echo off
chcp 65001 >nul
title Clack - environment check
if not "%~1"=="" set "CLACK_GAME=%~1"
echo.
echo   Clack environment check - please wait, about 10-60 seconds.
echo   A report file will be written to your Desktop.
echo.
powershell -NoProfile -ExecutionPolicy Bypass -Command "$c = Get-Content -LiteralPath '%~f0' -Raw -Encoding UTF8; $p = $c -split '(?m)^#>>>PS\r?\n', 2; Invoke-Expression $p[1]"
echo.
echo   Done. Press any key to close.
pause >nul
exit /b
#>>>PS
# 咔哒 · 环境体检
# 目的：一次把"缺什么、坏什么"看清楚，用户把生成的报告发回来就能定位。
# 只读，不改任何东西。管理员权限不是必须，但不是管理员会有几项读不到。

$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'
$lines = New-Object System.Collections.Generic.List[string]
function Say([string]$s = '') { Write-Host $s; $script:lines.Add([string]$s) }

$S32 = Join-Path $env:SystemRoot 'System32'
$W64 = Join-Path $env:SystemRoot 'SysWOW64'

function GetVer([string]$p) {
    try { return (Get-Item -LiteralPath $p -ErrorAction Stop).VersionInfo.FileVersion } catch { return '' }
}

# 可执行映像头校验：0 字节 / 被截断 / 被别的东西覆盖，这里都能看出来。
# 要认两种：PE（现代）和 NE（Windows 还留着的 Win16 兼容 dll，比如 ole2.dll）——
# 只认 PE 会把那批老文件全报到"损坏"里。
function PeOk([string]$p) {
    try {
        $fs = [IO.File]::Open($p, 'Open', 'Read', 'ReadWrite')
        $br = New-Object IO.BinaryReader($fs)
        $mz = $br.ReadUInt16()
        if ($mz -ne 0x5A4D) { $br.Close(); $fs.Close(); return $false }
        $fs.Position = 0x3C
        $off = $br.ReadInt32()
        if ($off -le 0 -or $off -ge ($fs.Length - 2)) { $br.Close(); $fs.Close(); return $false }
        $fs.Position = $off
        $sig = [Text.Encoding]::ASCII.GetString($br.ReadBytes(2))
        $br.Close(); $fs.Close()
        return ($sig -eq 'PE' -or $sig -eq 'NE')
    } catch { return $false }
}

# 文件状态：ok / 缺失 / 空文件 / PE 头损坏 / 无版本信息
function Probe([string]$p) {
    if (-not (Test-Path -LiteralPath $p)) { return '缺失' }
    try { $len = (Get-Item -LiteralPath $p).Length } catch { return '读不了' }
    if ($len -eq 0) { return '空文件' }
    if (-not (PeOk $p)) { return 'PE 头损坏' }
    if ([string]::IsNullOrWhiteSpace((GetVer $p))) { return '无版本信息' }
    return 'ok'
}

Add-Type -Namespace Clack -Name N -MemberDefinition @'
[DllImport("kernel32", SetLastError = true, CharSet = CharSet.Unicode, EntryPoint = "LoadLibraryW")]
public static extern IntPtr Load(string p);
[DllImport("kernel32", SetLastError = true)]
public static extern bool FreeLibrary(IntPtr h);
'@

# 真加载一次。加载不进去就是有问题，错误码还能分出是"文件坏了"还是"它自己缺依赖"
function LoadTest([string]$p) {
    $h = [Clack.N]::Load($p)
    if ($h -ne [IntPtr]::Zero) { [void][Clack.N]::FreeLibrary($h); return 'ok' }
    switch ([Runtime.InteropServices.Marshal]::GetLastWin32Error()) {
        126  { '加载失败：它依赖的 dll 找不到' }
        193  { '加载失败：文件已损坏' }
        1114 { '加载失败：初始化失败（多半损坏）' }
        5    { '加载失败：拒绝访问' }
        default { '加载失败' }
    }
}

# ---------- 开头 ----------
$os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
Say '================ 咔哒 · 环境体检 ================'
Say ("时间   " + (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Say ("系统   " + $os.Caption + "   " + $os.Version)
Say ("机器   " + $env:COMPUTERNAME + "   管理员权限：" + $(if ($admin) { '有' } else { '无（有几项读不到）' }))
Say ("芯片   " + $env:PROCESSOR_ARCHITECTURE)
Say ''

# ---------- 1. 运行库 dll ----------
Say '--------------- 1. 运行库 dll ----------------'
$x64 = @(
    'ucrtbase.dll', 'vcruntime140.dll', 'vcruntime140_1.dll', 'vcruntime140_threads.dll',
    'msvcp140.dll', 'msvcp140_1.dll', 'msvcp140_2.dll', 'msvcp140_atomic_wait.dll', 'msvcp140_codecvt_ids.dll',
    'concrt140.dll', 'vccorlib140.dll', 'vcamp140.dll', 'vcomp140.dll',
    'msvcr120.dll', 'msvcp120.dll', 'vccorlib120.dll',
    'msvcr110.dll', 'msvcp110.dll', 'vccorlib110.dll',
    'msvcr100.dll', 'msvcp100.dll'
)
$x86 = @(
    'ucrtbase.dll', 'vcruntime140.dll', 'vcruntime140_threads.dll',
    'msvcp140.dll', 'msvcp140_1.dll', 'msvcp140_2.dll', 'msvcp140_atomic_wait.dll', 'msvcp140_codecvt_ids.dll',
    'concrt140.dll', 'vccorlib140.dll', 'vcamp140.dll', 'vcomp140.dll',
    'msvcr120.dll', 'msvcp120.dll', 'vccorlib120.dll',
    'msvcr110.dll', 'msvcp110.dll', 'vccorlib110.dll',
    'msvcr100.dll', 'msvcp100.dll',
    'msvcr71.dll', 'msvcp71.dll', 'msvcr70.dll', 'msvcp70.dll', 'msvbvm60.dll', 'MSVBVM50.DLL'
)
$bad = New-Object System.Collections.Generic.List[string]
foreach ($set in @(@{ dir = $S32; names = $x64; tag = 'System32 (64 位)' }, @{ dir = $W64; names = $x86; tag = 'SysWOW64 (32 位)' })) {
    $okN = 0; $miss = @()
    Say ("[" + $set.tag + "]  " + $set.dir)
    foreach ($n in $set.names) {
        $p = Join-Path $set.dir $n
        $st = Probe $p
        if ($st -eq 'ok') {
            # 32 位那份在 64 位进程里加载不了，只能看文件本身
            if ($set.dir -eq $S32) {
                $lt = LoadTest $p
                if ($lt -ne 'ok') { $bad.Add("$n  $lt  ($p)") } else { $okN++ }
            } else { $okN++ }
        } else {
            if ($st -eq '缺失') { $miss += $n } else { $bad.Add("$n  $st  ($p)") }
        }
    }
    Say ("  正常 $okN / " + $set.names.Count + "   缺 " + $miss.Count)
    if ($miss.Count) { Say ("  缺的是: " + ($miss -join ', ')) }
}
if ($bad.Count) {
    Say '  !! 有问题的文件:'
    foreach ($b in $bad) { Say ("     " + $b) }
} else {
    Say '  ✓ 上面这些文件本身都完好'
}
Say '  （2005/2008 那两代（msvcr80/90）平时只装在 WinSxS 里，System32 里没有是正常的，所以没列）'
Say ''

# ---------- 2. 系统 UCRT / API Set ----------
Say '--------------- 2. 系统组件能不能加载 ----------------'
# 这里必须按"名字"加载，不能按路径：api-ms-win-* 是虚拟名，系统按一张表把它指到真身，
# 拿路径去加载反而会失败（这是正常的，不代表系统坏了）
foreach ($n in @('ucrtbase.dll', 'api-ms-win-crt-runtime-l1-1-0.dll', 'api-ms-win-crt-stdio-l1-1-0.dll', 'api-ms-win-crt-heap-l1-1-0.dll', 'api-ms-win-core-file-l1-2-0.dll')) {
    Say ("  " + $n.PadRight(44) + (LoadTest $n))
}
Say '  （这五项正常就说明系统 UCRT/API Set 没坏；坏了只能靠 sfc /scannow 修）'
Say ''

# ---------- 3. 注册表里登记的运行库 ----------
Say '--------------- 3. 注册表里登记的 VC++ / VB ----------'
$arp = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
$rows = Get-ItemProperty $arp -ErrorAction SilentlyContinue |
Where-Object { $_.DisplayName -match 'Visual C\+\+|Visual Basic|Microsoft Visual Studio 2010 Tools' } |
Sort-Object DisplayName
if ($rows) {
    foreach ($r in $rows) { Say ("  " + $r.DisplayName + "   [" + $r.DisplayVersion + "]") }
} else {
    Say '  （一条都没有：这台机器上没有任何 VC++ 运行库记录）'
}
Say '  （"登记了但第 1 节显示文件缺失" 就是最常见的"损坏"——安装器会因此跳过不装）'
Say ''

# ---------- 4. DirectX ----------
Say '--------------- 4. DirectX 运行库 ----------------'
foreach ($set in @(@{ dir = $S32; tag = 'System32' }, @{ dir = $W64; tag = 'SysWOW64' })) {
    $d3 = Get-ChildItem -LiteralPath $set.dir -Filter 'd3dx9_*.dll' -File -ErrorAction SilentlyContinue
    $have = @()
    foreach ($i in 24..43) { if (Test-Path (Join-Path $set.dir "d3dx9_$i.dll")) { $have += $i } }
    $miss = (24..43 | Where-Object { $have -notcontains $_ })
    Say ("  [" + $set.tag + "]  d3dx9: " + $have.Count + "/20" + $(if ($miss.Count) { "   缺 " + ($miss -join ',') } else { "   ✓ 齐" }))
    $others = @('d3dx10_43.dll', 'd3dx11_43.dll', 'd3dcompiler_43.dll', 'd3dcompiler_47.dll', 'xinput1_3.dll', 'xinput1_4.dll', 'xinput9_1_0.dll', 'xaudio2_7.dll', 'xaudio2_8.dll', 'xaudio2_9.dll')
    $gone = @()
    foreach ($n in $others) { if (-not (Test-Path (Join-Path $set.dir $n))) { $gone += $n } }
    if ($gone.Count) { Say ("   另外缺: " + ($gone -join ', ')) } else { Say '   另外那批（d3dx10/d3dx11/d3dcompiler/xinput/xaudio2）都在 ✓' }
}
Say '  （DirectX 那批归 咔哒DX 管，这里只是顺手看一眼）'
Say ''

# ---------- 5. 咔哒 的安装日志 ----------
Say '--------------- 5. 咔哒 上次安装的记录 ----------------'
$clickLog = 'C:\ProgramData\Click\Click.log'
if (Test-Path -LiteralPath $clickLog) {
    $tail = Get-Content -LiteralPath $clickLog -Encoding UTF8 -ErrorAction SilentlyContinue | Select-Object -Last 40
    Say ("  " + $clickLog + "（最后 40 行）")
    foreach ($t in $tail) { Say ("  " + $t) }
} else {
    Say '  （没有这份日志：这台机器上还没跑过 咔哒）'
}
Say ''

# ---------- 6. 全系统 dll 完整性 ----------
Say '--------------- 6. 全系统 dll 完整性速查 ----------------'
$seen = 0
$badDll = New-Object System.Collections.Generic.List[string]
foreach ($d in @($S32, $W64)) {
    foreach ($f in (Get-ChildItem -LiteralPath $d -Filter '*.dll' -File -ErrorAction SilentlyContinue)) {
        $seen++
        if ($f.Length -eq 0) { $badDll.Add("0 字节       " + $f.FullName); continue }
        if (-not (PeOk $f.FullName)) { $badDll.Add("PE 头损坏    " + $f.FullName + "   (" + $f.Length + " 字节)") }
    }
}
Say ("  扫了 " + $seen + " 个 dll（System32 + SysWOW64 全量）")
if ($badDll.Count -eq 0) {
    Say '  ✓ 一个坏的都没有（没有 0 字节的，PE 头也全对）'
} else {
    Say ("  !! 发现 " + $badDll.Count + " 个坏的：")
    foreach ($b in ($badDll | Select-Object -First 40)) { Say ("     " + $b) }
}
Say ''

# ---------- 7. 游戏目录 / 模组链 ----------
Say '--------------- 7. 游戏目录里的插件 ----------------'
$game = $null
if ($env:CLACK_GAME -and (Test-Path -LiteralPath $env:CLACK_GAME)) {
    $game = (Get-Item -LiteralPath $env:CLACK_GAME).FullName
} else {
    $skip = '\Windows\', '\Program Files\', '\Program Files (x86)\', '$Recycle.Bin', '\ProgramData\', '\AppData\', '\System Volume Information\', '\node_modules\'
    function Find-Named([string]$name) {
        $out = @()
        foreach ($r in ((Get-PSDrive -PSProvider FileSystem).Root | Where-Object { $_ -match '^[A-Z]:\\$' })) {
            try {
                $out += @(Get-ChildItem -LiteralPath $r -Recurse -Depth 5 -File -Filter $name -Force -ErrorAction SilentlyContinue |
                    Where-Object { $p = $_.DirectoryName; -not ($skip | Where-Object { $p -like "*$_*" }) } |
                    Select-Object -First 20 -ExpandProperty DirectoryName)
            } catch { }
        }
        return @($out | Sort-Object -Unique)
    }
    $cand = Find-Named 'ScriptHookV.log'
    if ($cand.Count -eq 0) { $cand = Find-Named 'GTA5.exe' }
    # 先认真的游戏目录：有 GTA5.exe 的 → 有 ScriptHookV.dll 的 → 剩下随便取一个
    $game = @($cand | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'GTA5.exe') })[0]
    if (-not $game) { $game = @($cand | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'ScriptHookV.dll') })[0] }
    if (-not $game -and $cand.Count) { $game = @($cand)[0] }
}
if (-not $game) {
    Say '  没自动找到游戏目录。把游戏目录拖到本脚本上再跑一次就有了。'
} else {
    Say ("  游戏目录: " + $game)
    foreach ($n in @('ScriptHookV.dll', 'dinput8.dll', 'AdvancedHookV.dll', 'ELS.asi', 'ELS.ini', 'ELS\ELS.ini', 'ScriptHookVDotNet.asi', 'ScriptHookV.log')) {
        $p = Join-Path $game $n
        Say ("    " + $n.PadRight(26) + $(if (Test-Path -LiteralPath $p) { '有' } else { '没有' }))
    }
    $vcf = Join-Path $game 'ELS\vcf'
    if (Test-Path -LiteralPath $vcf) {
        Say ("    ELS\vcf 里的车辆配置: " + (Get-ChildItem -LiteralPath $vcf -File -ErrorAction SilentlyContinue).Count + " 个")
    }
    $shv = Join-Path $game 'ScriptHookV.log'
    if (Test-Path -LiteralPath $shv) {
        Say '    ---- ScriptHookV.log 最后 30 行（最关键：能看出插件到底加载了没）----'
        foreach ($t in (Get-Content -LiteralPath $shv -Encoding UTF8 -ErrorAction SilentlyContinue | Select-Object -Last 30)) { Say ("    " + $t) }
    }
}
Say ''
Say '================ 报告结束 ================'

$out = Join-Path ([Environment]::GetFolderPath('Desktop')) ("咔哒体检-" + $env:COMPUTERNAME + ".txt")
try {
    [IO.File]::WriteAllLines($out, $lines, (New-Object Text.UTF8Encoding($true)))
    Write-Host ''
    Write-Host ("报告已保存: " + $out) -ForegroundColor Green
} catch {
    Write-Host ("报告写不出来: " + $_.Exception.Message) -ForegroundColor Red
}
