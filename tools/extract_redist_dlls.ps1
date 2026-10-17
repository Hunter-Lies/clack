# 从官方自解压安装器（burn 打包的 vc_redist*.exe）里把运行时 dll 抠成散装文件，
# 放到 payload 的 <组件>\<架构>\_lib\ 下。
#
# 为什么需要这份东西：
#   官方安装器遇到"这台机器上已经登记过更高版本"或者"安装记录坏了（1612）"时
#   直接跳过，说一句"这次没动它"就走了；文件本身被改坏了它也不认。老合集的
#   做法是硬往系统目录里拷 dll，但拷的是 2015 年那批，会把新系统顶回旧版。
#   我们只拷 14.x / 2012 / 2013 的运行库本体（官方最新版，不会比系统里的旧），
#   不碰 ucrtbase.dll 和 api-ms-win-* —— 那些在 Win10 以上是系统自己的组件。
#
# burn 包的结构：exe 壳 + UX cab + N 个附加 cab；附加 cab 里装着 MSI 和真正的
# 文件，有的还要再钻一层。7-Zip 只看得到最外面那层，所以这里按 MSCF 签名自己切。
#
# 用法：.\tools\extract_redist_dlls.ps1
#
# 现状：这一步是**独立工具**，stage_payload.ps1 不会自动调用它，exe 里也还没有
#       消费 _lib 的代码。留着是因为抽链已经验证可用；哪天要接"安装器说已装、
#       文件其实坏了"的兜底，直接拿这份产物接上就行。
[CmdletBinding()]
param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = 'Stop'
$Payload = Join-Path $Root '_work\payload'
$Tmp     = Join-Path $Root '_work\extracted\_lib'

$SevenZip = @(
    'C:\Program Files\7-Zip\7z.exe'
    'C:\Program Files (x86)\7-Zip\7z.exe'
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $SevenZip) { throw '需要 7-Zip' }

# 要留下哪些文件。只认运行库本体，不拉 MFC、不拉调试版（xxxd.dll）、不拉 arm64。
# cab 里的成员名有两种写法，脚本会先规范化成真实文件名再对这张表：
#   F_CENTRAL_msvcr120_x64        （2012/2013 那两代）
#   msvcp140.dll_amd64            （14.x 那两代）
$items = @(
    @{ Dir = '08_VC2012';           Arch = 'x86'; Exe = 'vc_redist2012_x86.exe'; Keep = '^(msvcr110|msvcp110|vcamp110|vccorlib110|vcomp110)\.dll$' }
    @{ Dir = '08_VC2012';           Arch = 'x64'; Exe = 'vc_redist2012_x64.exe'; Keep = '^(msvcr110|msvcp110|vcamp110|vccorlib110|vcomp110)\.dll$' }
    @{ Dir = '09_VC2013';           Arch = 'x86'; Exe = 'vc_redist2013_x86.exe'; Keep = '^(msvcr120|msvcp120|vcamp120|vccorlib120|vcomp120)\.dll$' }
    @{ Dir = '09_VC2013';           Arch = 'x64'; Exe = 'vc_redist2013_x64.exe'; Keep = '^(msvcr120|msvcp120|vcamp120|vccorlib120|vcomp120)\.dll$' }
    @{ Dir = '10_VC2015-2022_14.51'; Arch = 'x86'; Exe = 'vc_redist.x86.exe';    Keep = '^(concrt140|msvcp140(_1|_2|_atomic_wait|_codecvt_ids)?|vcamp140|vccorlib140|vcomp140|vcruntime140(_1|_threads)?)\.dll$' }
    @{ Dir = '10_VC2015-2022_14.51'; Arch = 'x64'; Exe = 'vc_redist.x64.exe';    Keep = '^(concrt140|msvcp140(_1|_2|_atomic_wait|_codecvt_ids)?|vcamp140|vccorlib140|vcomp140|vcruntime140(_1|_threads)?)\.dll$' }
)

New-Item -ItemType Directory -Force $Tmp | Out-Null
$script:seq = 0

function New-Scratch {
    $script:seq++
    $p = Join-Path $Tmp "s$($script:seq)"
    New-Item -ItemType Directory -Force $p | Out-Null
    $p
}

# 扫 MSCF 签名，把这条文件里所有内嵌的 cab 切出来（burn 包的附加容器就靠这个看见）
function Split-Cabs([string]$Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    $text  = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
    $out = @()
    $pos = 0
    while ($true) {
        $p = $text.IndexOf('MSCF', $pos)
        if ($p -lt 0) { break }
        $size = [BitConverter]::ToUInt32($bytes, $p + 8)
        if ($size -le 8 -or $p + $size -gt $bytes.Length) { break }
        $cab = Join-Path (New-Scratch) 'c.cab'
        [IO.File]::WriteAllBytes($cab, $bytes[$p..($p + $size - 1)])
        $out += $cab
        $pos = $p + $size
    }
    $out
}

# 递归钻：cab 里的成员如果是 dll 就收下，如果是个没后缀的容器就继续钻
function Find-Dlls([string]$Path, [int]$Depth) {
    if ($Depth -gt 4) { return @() }
    $found = @()
    foreach ($cab in (Split-Cabs $Path)) {
        $dst = New-Scratch
        & $SevenZip x $cab "-o$dst" -y | Out-Null
        foreach ($m in (Get-ChildItem $dst -File -ErrorAction SilentlyContinue)) {
            # 成员名有三种写法，都算候选：
            #   msvcp140.dll_amd64 / msvcp140.dll      （14.x）
            #   F_CENTRAL_msvcr120_x64                 （2012/2013，MSI 内部的 File 键名）
            # 剩下的没后缀成员才是内层容器，继续钻
            $isDll = ($m.Name -match '\.dll(_[a-zA-Z0-9]+)*$') -or ($m.Name -match '^F_CENTRAL_')
            if ($isDll) {
                $found += , @{ Cab = $cab; Name = $m.Name; Path = $m.FullName }
            }
            elseif ($m.Extension -eq '') {
                $found += @(Find-Dlls $m.FullName ($Depth + 1))
            }
        }
    }
    $found
}

$total = 0
foreach ($it in $items) {
    $exe = Join-Path $Payload "$($it.Dir)\$($it.Arch)\$($it.Exe)"
    if (-not (Test-Path -LiteralPath $exe)) {
        Write-Warning "跳过（没有这份安装器）：$exe"
        continue
    }
    $lib = Join-Path $Payload "$($it.Dir)\$($it.Arch)\_lib"
    New-Item -ItemType Directory -Force $lib | Out-Null

    # 同名的取一份就够：x64 包里还附带 arm64 的那套，按架构后缀过滤掉
    $seen = @{}
    $got = 0
    foreach ($hit in (Find-Dlls $exe 0)) {
        $raw = $hit.Name
        if ($raw -match '_(arm64)$') { continue }
        if ($it.Arch -eq 'x86' -and $raw -match '_(amd64|x64)$') { continue }
        if ($it.Arch -eq 'x64' -and $raw -match '_x86$') { continue }
        $name = $raw -replace '^F_CENTRAL_', ''
        if ($name -match '\.dll') {
            # msvcp140.dll_amd64 -> msvcp140.dll
            $name = $name -replace '\.dll(_[a-zA-Z0-9]+)*$', '.dll'
        } else {
            # F_CENTRAL_msvcr120_x64 -> msvcr120.dll
            $name = ($name -replace '_(x64|x86|amd64|x86_amd64)$', '') + '.dll'
        }
        if ($seen.ContainsKey($name)) { continue }
        if ($name -notmatch $it.Keep) { continue }
        Copy-Item -LiteralPath $hit.Path -Destination (Join-Path $lib $name) -Force
        $seen[$name] = $true
        $got++
    }
    $names = ($seen.Keys | Sort-Object) -join ', '
    if ($got -eq 0) {
        Write-Warning "$($it.Dir)\$($it.Arch) 一个 dll 都没抽到"
    } else {
        Write-Host ("{0}\{1}: {2} 个 -> {3}" -f $it.Dir, $it.Arch, $got, $names)
    }
    $total += $got
}
Write-Host "共抽出 $total 个文件"
