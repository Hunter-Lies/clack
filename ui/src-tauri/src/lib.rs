// 咔哒 / Clack —— 运行库合集的后端。
// 只做三件事：扫描已装情况、按顺序静默安装缺的那些、把进度推给前端。
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

/// 安装进行中。这段时间不允许关窗口——半截被打断最难收拾。
static BUSY: AtomicBool = AtomicBool::new(false);

/// 正在用管理员身份重新拉起自己。这时候别清 payload——下一个进程还要用。
static RELAUNCHING: AtomicBool = AtomicBool::new(false);

/// 这一轮里是不是已经重新解包过了。一次运行最多重解一次：
/// 59 MB 解一遍不便宜，多个组件同时缺文件时没必要每个都重解。
static REEXTRACTED: AtomicBool = AtomicBool::new(false);

/// 这次运行有没有安排"重启后再替换"的东西（老 dll 被占用时会走这条路）。
/// 有的话，界面上要提示用户重启电脑之后才生效。
static REBOOT_NEEDED: AtomicBool = AtomicBool::new(false);

/// 这一轮开始之前，系统里已经挂着的"待重启文件操作"。
/// 用来区分安装器回的 3010 到底是不是我们造成的（见 reboot_is_ours）。
static PENDING_BEFORE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// 这次运行是不是第一个实例。用户双击两遍时，第二个实例不负责清 payload
/// ——它要是清了，正在装的那个实例会当场缺文件。
static FIRST_INSTANCE: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Arch {
    X86,
    X64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Msi,
    Exe,
    /// 老运行库是散装 dll，直接复制进系统目录
    Dll,
}

struct Comp {
    id: &'static str,
    /// 产品分组：x86/x64 是同一行，用户看到的是产品数，不是文件数
    group: &'static str,
    name: &'static str,
    version: &'static str,
    arch: Arch,
    kind: Kind,
    /// payload 下的子目录
    dir: &'static str,
    /// Msi/Exe 取第一个；Dll 是全部要复制的文件
    files: &'static [&'static str],
    /// Exe 的静默参数。几代安装器的开关都不一样：2005 只认 /Q，
    /// 2008/2010 是 /Q /NORESTART，2012 之后才是 /install /quiet /norestart
    args: &'static [&'static str],
    /// MSI 那一类的 ProductCode（照我们包里那份 MSI 的属性表抄的，固定不变）。
    /// 只在一处用：1612 时把这个目录补进 Windows 的安装源列表，见 msi_retry_with_source()。
    /// 非 MSI 的组件留空。
    pcode: &'static str,
}

// 不做"装没装"的判断，全部照装。安装器本身幂等（已有会返回 1638 / 0），
// 而漏装的代价远大于多花几十秒——检测一旦判错，用户就是拿着一台依然缺库的机器。
const COMPS: &[Comp] = &[
    Comp { id: "vb5", group: "vb5", name: "Visual Basic VM 5.x", version: "5.02.8244", arch: Arch::X86, kind: Kind::Dll,
        dir: "01_VB5_VM/x86", files: &["MSVBVM50.DLL"], args: &[], pcode: "" },
    Comp { id: "vb6", group: "vb6", name: "Visual Basic VM 6.0", version: "6.00.9802", arch: Arch::X86, kind: Kind::Dll,
        dir: "02_VB6_VM/x86", files: &["MSVBVM60.DLL"], args: &[], pcode: "" },
    Comp { id: "crt70", group: "crt70", name: "C Runtime 2002", version: "7.00.9981", arch: Arch::X86, kind: Kind::Dll,
        dir: "03_CRT_2002/x86",
        files: &["MSVCR70.DLL", "MSVCP70.DLL", "atl70.dll", "mfc70.dll", "mfc70u.dll", "mfc70chs.dll"], args: &[], pcode: "" },
    Comp { id: "crt71", group: "crt71", name: "C Runtime 2003", version: "7.10.3078", arch: Arch::X86, kind: Kind::Dll,
        dir: "04_CRT_2003/x86",
        files: &["MSVCR71.DLL", "MSVCP71.DLL", "atl71.dll", "MFC71.DLL", "mfc71u.dll", "mfc71chs.dll"], args: &[], pcode: "" },
    // 2005/2008/2010：装的是"MSI + 它自己的 CAB"，由 msiexec /qn 直接跑。
    // 不走官方的自解压 exe，是因为那层壳内部还会调 msiexec 弹进度框；
    // 而且这几代的 MSI 少了同目录的 CAB 就会失败（1620/1635），所以两个文件必须一起在。
    Comp { id: "vc2005_x86", group: "vc2005", name: "Visual C++ 2005 SP1", version: "8.0.61001", arch: Arch::X86, kind: Kind::Msi,
        dir: "05_VC2005/x86", files: &["vcredist.msi"], args: &[], pcode: "{710f4c1c-cc18-4c49-8cbf-51240c89a1a2}" },
    Comp { id: "vc2005_x64", group: "vc2005", name: "Visual C++ 2005 SP1 (x64)", version: "8.0.61001", arch: Arch::X64, kind: Kind::Msi,
        dir: "05_VC2005/x64", files: &["vcredist.msi"], args: &[], pcode: "{ad8a2fa1-06e7-4b0d-927d-6e54b3d31028}" },
    Comp { id: "vc2008_x86", group: "vc2008", name: "Visual C++ 2008 SP1", version: "9.0.30729.6161", arch: Arch::X86, kind: Kind::Msi,
        dir: "06_VC2008/x86", files: &["vc_red.msi"], args: &[], pcode: "{9BE518E6-ECC6-35A9-88E4-87755C07200F}" },
    Comp { id: "vc2008_x64", group: "vc2008", name: "Visual C++ 2008 SP1 (x64)", version: "9.0.30729.6161", arch: Arch::X64, kind: Kind::Msi,
        dir: "06_VC2008/x64", files: &["vc_red.msi"], args: &[], pcode: "{5FCE6D76-F5DC-37AB-B2B8-22AB8CEDB1D4}" },
    // 2010 的 SP1 补丁（KB2565063）就在同目录，装完 MSI 会自动接着打上
    Comp { id: "vc2010_x86", group: "vc2010", name: "Visual C++ 2010 SP1", version: "10.0.40219.473", arch: Arch::X86, kind: Kind::Msi,
        dir: "07_VC2010/x86", files: &["vc_red.msi"], args: &[], pcode: "{F0C3E5D1-1ADE-321E-8167-68EF0DE699A5}" },
    Comp { id: "vc2010_x64", group: "vc2010", name: "Visual C++ 2010 SP1 (x64)", version: "10.0.40219.473", arch: Arch::X64, kind: Kind::Msi,
        dir: "07_VC2010/x64", files: &["vc_red.msi"], args: &[], pcode: "{1D8E6291-B0D5-35EC-8441-6616F567A0F7}" },
    // 注意文件名：2012/2013 这两代官方包就叫 vc_redist2012_x86.exe 这种，
    // 不是 2015-2022 那份的 vc_redist.x86.exe。写错的表现是
    // "启动安装器失败：系统找不到指定的文件 (os error 2)"——文件其实在，只是名字对不上。
    Comp { id: "vc2012_x86", group: "vc2012", name: "Visual C++ 2012 U4", version: "11.0.61030", arch: Arch::X86, kind: Kind::Exe,
        dir: "08_VC2012/x86", files: &["vc_redist2012_x86.exe"], args: &["/install", "/quiet", "/norestart"], pcode: "" },
    Comp { id: "vc2012_x64", group: "vc2012", name: "Visual C++ 2012 U4 (x64)", version: "11.0.61030", arch: Arch::X64, kind: Kind::Exe,
        dir: "08_VC2012/x64", files: &["vc_redist2012_x64.exe"], args: &["/install", "/quiet", "/norestart"], pcode: "" },
    Comp { id: "vc2013_x86", group: "vc2013", name: "Visual C++ 2013", version: "12.0.40664", arch: Arch::X86, kind: Kind::Exe,
        dir: "09_VC2013/x86", files: &["vc_redist2013_x86.exe"], args: &["/install", "/quiet", "/norestart"], pcode: "" },
    Comp { id: "vc2013_x64", group: "vc2013", name: "Visual C++ 2013 (x64)", version: "12.0.40664", arch: Arch::X64, kind: Kind::Exe,
        dir: "09_VC2013/x64", files: &["vc_redist2013_x64.exe"], args: &["/install", "/quiet", "/norestart"], pcode: "" },
    Comp { id: "vc2022_x86", group: "vc2022", name: "Visual C++ 2015-2022", version: "14.51.36247", arch: Arch::X86, kind: Kind::Exe,
        dir: "10_VC2015-2022_14.51/x86", files: &["vc_redist.x86.exe"], args: &["/install", "/quiet", "/norestart"], pcode: "" },
    Comp { id: "vc2022_x64", group: "vc2022", name: "Visual C++ 2015-2022 (x64)", version: "14.51.36247", arch: Arch::X64, kind: Kind::Exe,
        dir: "10_VC2015-2022_14.51/x64", files: &["vc_redist.x64.exe"], args: &["/install", "/quiet", "/norestart"], pcode: "" },
    // VSTOR 也是缺 cab 的那一类（1619），用官方 vstor_redist 里自带的安装器
    // 不做 VSTOR（VS 2010 Tools for Office）：那是给老 Office 的 VSTO 插件用的运行时，
    // 跟游戏无关。而且它在 64 位系统上要么缺 install.ini（1308）、要么被 MSI 自己拒绝
    // （x86 版写死了 CA_BlockOnAMD64），留着只是给自己添两个红色。
];

#[derive(Serialize)]
pub struct GroupInfo {
    id: &'static str,
    name: &'static str,
    version: &'static str,
    /// 这一组下面具体要跑哪几个安装器（x86 / x64）
    ids: Vec<&'static str>,
}

#[derive(Clone, Serialize)]
struct Progress {
    /// 界面上的那一行（产品），不是具体文件
    row: String,
    id: String,
    state: &'static str,
    index: usize,
    total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Clone, Serialize)]
struct Done {
    ok: bool,
    failed: Vec<String>,
    /// 兜底：有东西要重启电脑之后才生效（3010 / 1641，或某个 dll 只能安排到重启时替换）
    reboot: bool,
}

// ---------- 路径 ----------

/// 正式版把整个 payload 以 zip 内嵌进 exe，这样一个文件就能发出去。
/// 开发版不嵌，直接读项目里那份活的 payload，改完立刻生效、也不用每次重编 50MB 数据。
#[cfg(not(debug_assertions))]
const PAYLOAD_ZIP: &[u8] = include_bytes!("../payload.zip");

/// 日志放 `C:\ProgramData\Click\Click.log`，不用 `%TEMP%`。
/// 标准账户 + UAC 里输管理员密码时，提权进程的 `%TEMP%` 是那个管理员账户的，
/// 日志会溜到别的用户目录去，用户"把日志发过来"发来的就不是这份。
/// 这里路径固定、纯 ASCII、普通权限也读得到。
fn log_path() -> PathBuf {
    click_dir().join("Click.log")
}

/// 每次启动清空日志：一次运行只留一份，用户发过来的就是这一次的完整记录。
fn reset_log() {
    let _ = std::fs::create_dir_all(click_dir());
    // 提权起来的那个进程（带 --elevated）是接着同一个进程的活干的，别把它前面写的内容抹掉
    if !std::env::args().any(|a| a == "--elevated") {
        let _ = std::fs::write(log_path(), b"");
    }
}

/// 同时打到控制台（开发时看）和 Click.log（用户出问题直接把文件发过来）
fn log(line: &str) {
    // 别用 println!：正式版没有控制台，stdout 是空句柄，写失败会 panic，
    // 而 release 配了 panic = "abort"，一个日志就能把整个程序带走。
    let _ = writeln!(std::io::stdout(), "[click] {line}");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = writeln!(f, "[click] {line}");
    }
}

/// 程序自己的工作目录。放在 ProgramData 而不是 %TEMP%：
/// 这里是纯 ASCII 路径（msiexec 吃不消中文路径），普通权限也读得到，
/// 用户/客服不用提权就能把日志发出来。退出时会清掉解出来的东西。
fn click_dir() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
        .join("Click")
}

/// 解出来的运行库放 %ProgramData%\Click\payload\<版本>\，同一次运行里只解一遍。
#[cfg(not(debug_assertions))]
fn payload_dir() -> PathBuf {
    click_dir()
        .join("payload")
        .join(env!("CARGO_PKG_VERSION"))
}

/// Windows 安装器那一类文件的后缀。这几个是"源码"：装完之后得留在原地。
const MSI_EXTS: &[&str] = &["msi", "cab", "msp"];

fn has_ext(p: &Path, exts: &[&str]) -> bool {
    p.extension()
        .map(|x| x.to_string_lossy().to_lowercase())
        .map(|x| exts.contains(&x.as_str()))
        .unwrap_or(false)
}

/// 退出前把解出来的 payload 收走：它只为这一次运行存在，下次启动重新解一遍。
/// 顺手清掉组件表里已经没有的目录（老版本留下的残渣）。
///
/// 但 **2005 / 2008 / 2010 这三代的 MSI + CAB 必须留下**。它们是
/// "MSI 引用同目录 CAB"的结构，Windows 在装的时候把这个目录记成这批运行库的
/// 安装源；以后系统做修复、重装、打补丁都要回这里取 CAB。删掉它，那台机器上
/// 这几代就会回 `msiexec 返回 1612`——用户装的第一次是好的，之后系统一修就坏。
/// install.log / install-retry.log 也留在这儿，出问题要看的就是它。
///
/// 散装 dll 和 2012 之后的自解压安装器没有这个问题（它们各自带全了文件），
/// 一并不留，省那几十 MB。
///
/// 强杀进程（任务管理器）时来不及跑，那种情况 payload 会剩下：再正常开关一次即可。
fn cleanup() {
    if RELAUNCHING.load(Ordering::SeqCst)
        || BUSY.load(Ordering::SeqCst)
        || !FIRST_INSTANCE.load(Ordering::SeqCst)
    {
        return;
    }
    let base = click_dir();

    // 1) 解出来的那份整个删掉（连旧版本的目录一起），下次启动重解一遍就是了
    let payload = base.join("payload");
    if payload.is_dir() && std::fs::remove_dir_all(&payload).is_err() {
        log(&format!("清理 {} 失败（多半是有文件被占着），下次再试", payload.display()));
    }

    // 2) 组件目录里那些"这一代用不上"的残留（比如 1.0.0 留下的 vstor_x86），
    //    组件表里已经没有它们了，不会有人再往里写。MSI / CAB 与日志留着。
    let known: Vec<&str> = COMPS.iter().map(|c| c.id).collect();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let p = e.path();
            if !p.is_dir() || name == "payload" || known.contains(&name.as_str()) {
                continue;
            }
            if std::fs::remove_dir_all(&p).is_ok() {
                log(&format!("清掉用不上的目录 {name}"));
            }
        }
    }
}

/// payload 里某个文件被删了（杀毒软件干的、解包被中断、用户清过目录）：
/// 把解出来的那份整个撤掉重解一遍。返回 true = 重解成功，可以重试。
#[cfg(not(debug_assertions))]
fn refresh_payload() -> bool {
    let dir = payload_dir();
    let _ = std::fs::remove_file(dir.join(".ok"));
    let _ = std::fs::remove_dir_all(&dir);
    ensure_payload().is_some()
}

/// 开发时 payload 就是源码目录里的那份，没什么可重解的
#[cfg(debug_assertions)]
fn refresh_payload() -> bool {
    false
}

#[cfg(not(debug_assertions))]
fn ensure_payload() -> Option<PathBuf> {
    let dir = payload_dir();
    if dir.join(".ok").is_file() {
        return Some(dir);
    }
    log(&format!("释放内置运行库到 {}", dir.display()));
    std::fs::create_dir_all(&dir).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(PAYLOAD_ZIP)).ok()?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).ok()?;
        let rel = f.enclosed_name()?.to_path_buf();
        let out = dir.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out).ok()?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).ok()?;
        }
        let mut w = std::fs::File::create(&out).ok()?;
        std::io::copy(&mut f, &mut w).ok()?;
    }
    std::fs::write(dir.join(".ok"), env!("CARGO_PKG_VERSION")).ok();
    Some(dir)
}

fn windir() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
}

fn is_64bit_os() -> bool {
    std::env::var_os("ProgramFiles(x86)").is_some()
}

/// 32 位组件放 SysWOW64（x64 系统上 32 位程序读的就是那里），64 位组件放 System32
fn target_dir(arch: Arch) -> PathBuf {
    let w = windir();
    match arch {
        Arch::X64 => w.join("System32"),
        Arch::X86 => {
            if is_64bit_os() {
                w.join("SysWOW64")
            } else {
                w.join("System32")
            }
        }
    }
}

/// 读文件的固定版本（VS_FIXEDFILEINFO 的四段数字）。
/// 不用 FileVersion 字符串：那玩意带 `(WGGT_OCT06.Release)` 这种后缀，比不了。
/// 读不出来（0 字节、被截断、被换成别的东西、本来就没版本资源）返回 None。
#[cfg(windows)]
fn file_version(path: &Path) -> Option<[u32; 4]> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    // VS_FIXEDFILEINFO 在 windows-sys 里没导出，按官方布局自己定义
    #[repr(C)]
    struct FixedFileInfo {
        signature: u32,
        struc_version: u32,
        file_version_ms: u32,
        file_version_ls: u32,
        product_version_ms: u32,
        product_version_ls: u32,
        file_flags_mask: u32,
        file_flags: u32,
        file_os: u32,
        file_type: u32,
        file_subtype: u32,
        file_date_ms: u32,
        file_date_ls: u32,
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut handle: u32 = 0;
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, buf.as_mut_ptr() as *mut _) == 0 {
            return None;
        }
        let sub: Vec<u16> = "\\".encode_utf16().chain(std::iter::once(0)).collect();
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len: u32 = 0;
        if VerQueryValueW(buf.as_ptr() as *const _, sub.as_ptr(), &mut ptr, &mut len) == 0
            || ptr.is_null()
            || (len as usize) < std::mem::size_of::<FixedFileInfo>()
        {
            return None;
        }
        let info = &*(ptr as *const FixedFileInfo);
        if info.signature != 0xFEEF_04BD {
            return None;
        }
        let v = [
            info.file_version_ms >> 16,
            info.file_version_ms & 0xFFFF,
            info.file_version_ls >> 16,
            info.file_version_ls & 0xFFFF,
        ];
        if v == [0, 0, 0, 0] {
            return None;
        }
        Some(v)
    }
}

#[cfg(not(windows))]
fn file_version(_path: &Path) -> Option<[u32; 4]> {
    None
}

/// 请占用这些文件的程序退出，并且**等它们真的退出**。
///
/// 这里踩过一次：只调 `RmShutdown` 就往下走，而它只是"发出关闭请求"、不等进程结束。
/// 安装器一抬手就看见文件还在被用，于是把替换推给重启（3010），界面就老是提示
/// "重启电脑后修复才生效"。所以现在是：请求 → 等最多 8 秒 → 还赖着的直接结束它 → 再等 3 秒。
/// 用 Windows 自己的重启管理器拿名单（安装程序那句"关掉正在使用本程序的应用"就是这套 API），
/// 比自己枚举句柄稳。需要管理员权限（我们启动时就要了）。
#[cfg(windows)]
fn kill_lockers(paths: &[PathBuf]) -> Vec<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::RestartManager::{
        RmEndSession, RmForceShutdown, RmGetList, RmRegisterResources, RmShutdown, RmStartSession,
        RM_PROCESS_INFO,
    };

    let mut names: Vec<String> = Vec::new();
    let mut pids: Vec<u32> = Vec::new();
    let wide: Vec<Vec<u16>> = paths
        .iter()
        .map(|p| p.as_os_str().encode_wide().chain(std::iter::once(0)).collect())
        .collect();
    let ptrs: Vec<*const u16> = wide.iter().map(|w| w.as_ptr()).collect();

    unsafe {
        let mut session: u32 = 0;
        let mut key = [0u16; 33]; // CCH_RM_SESSION_KEY + 1
        if RmStartSession(&mut session, 0, key.as_mut_ptr()) != 0 {
            return names;
        }
        if RmRegisterResources(
            session,
            ptrs.len() as u32,
            ptrs.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
        ) == 0
        {
            let mut need: u32 = 0;
            let mut count: u32 = 0;
            let mut reasons: u32 = 0;
            // 第一遍只为问出"要几个 RM_PROCESS_INFO"（返回 ERROR_MORE_DATA）
            RmGetList(session, &mut need, &mut count, std::ptr::null_mut(), &mut reasons);
            if need > 0 {
                let mut infos: Vec<RM_PROCESS_INFO> = (0..need).map(|_| std::mem::zeroed()).collect();
                count = need;
                if RmGetList(session, &mut need, &mut count, infos.as_mut_ptr(), &mut reasons) == 0 {
                    for info in infos.iter().take(count as usize) {
                        let name: String = info
                            .strAppName
                            .iter()
                            .take_while(|c| **c != 0)
                            .map(|c| char::from_u32(*c as u32).unwrap_or('?'))
                            .collect();
                        if !name.is_empty() {
                            names.push(name);
                        }
                        pids.push(info.Process.dwProcessId);
                    }
                    // 强关：这些进程正拿着我们要写的 dll，不结束没法替换。
                    // 用户通常还开着游戏，这一步就是把游戏关掉。
                    RmShutdown(session, RmForceShutdown as u32, None);
                }
            }
        }
        RmEndSession(session);
    }

    names.sort();
    names.dedup();
    if pids.is_empty() {
        return names;
    }

    // 等它们真的走掉再返回——不然上面的请求等于白发
    let waited = wait_gone(&pids, 8_000);
    if waited > 0 {
        log(&format!("等了 {waited} 毫秒，{} 个进程还没退出，直接结束它们", pids.len()));
        hard_kill(&pids);
    }
    names
}

/// 等这些进程退出，返回实际等待的毫秒数（0 = 等之前就都没了）。
#[cfg(windows)]
fn wait_gone(pids: &[u32], max_ms: u64) -> u64 {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE,
    };

    let me = unsafe { GetCurrentProcessId() };
    let step = 200u64;
    let mut waited = 0u64;
    while waited < max_ms {
        let mut alive = 0;
        for pid in pids {
            // 自己也在名单里的话别等自己（理论上不会），pid 0/4 是空闲和 System
            if *pid == 0 || *pid == 4 || *pid == me {
                continue;
            }
            unsafe {
                let h = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, *pid);
                if h.is_null() {
                    continue; // 打不开：多半已经没了
                }
                let exited = WaitForSingleObject(h, 0) == 0;
                CloseHandle(h);
                if !exited {
                    alive += 1;
                }
            }
        }
        if alive == 0 {
            return waited;
        }
        std::thread::sleep(std::time::Duration::from_millis(step));
        waited += step;
    }
    waited
}

/// 宽限期过了还活着的那几个，直接结束。
#[cfg(windows)]
fn hard_kill(pids: &[u32]) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE,
    };

    let me = unsafe { GetCurrentProcessId() };
    for pid in pids {
        if *pid == 0 || *pid == 4 || *pid == me {
            continue;
        }
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, *pid);
            if h.is_null() {
                continue;
            }
            if WaitForSingleObject(h, 0) != 0 {
                log(&format!("{pid} 没自己退，直接结束"));
                TerminateProcess(h, 1);
                WaitForSingleObject(h, 3_000);
            }
            CloseHandle(h);
        }
    }
}

#[cfg(not(windows))]
fn kill_lockers(_paths: &[PathBuf]) -> Vec<String> {
    Vec::new()
}

/// 这一代运行库的核心文件。装之前拿这些路径去问"谁正加载着它"，
/// 有程序在用就先请它让路——否则安装器一看文件占用，就把替换推到重启（返回 3010），
/// 界面上就只能提示"重启电脑后修复才生效"。
fn hot_files(id: &str) -> &'static [&'static str] {
    match id {
        "vc2005_x86" | "vc2005_x64" => &["msvcr80.dll", "msvcp80.dll"],
        "vc2008_x86" | "vc2008_x64" => &["msvcr90.dll", "msvcp90.dll", "msvcm90.dll"],
        "vc2010_x86" | "vc2010_x64" => &["msvcr100.dll", "msvcp100.dll"],
        "vc2012_x86" | "vc2012_x64" => &["msvcr110.dll", "msvcp110.dll"],
        "vc2013_x86" | "vc2013_x64" => &["msvcr120.dll", "msvcp120.dll"],
        "vc2022_x86" | "vc2022_x64" => &["vcruntime140.dll", "msvcp140.dll"],
        _ => &[],
    }
}

/// 装这一类之前，先关掉正加载它那几个文件的程序。返回被关掉的进程名。
fn clear_holders(c: &Comp) -> Vec<String> {
    let dir = target_dir(c.arch);
    let paths: Vec<PathBuf> = hot_files(c.id)
        .iter()
        .map(|n| dir.join(n))
        .filter(|p| p.is_file())
        .collect();
    if paths.is_empty() {
        return Vec::new();
    }
    kill_lockers(&paths)
}

/// 换不了又急着要的那一手：新文件先放旁边，让 Windows 下次重启时替换掉。
/// 安装程序那句"重启后生效"就是这么来的。
#[cfg(windows)]
fn schedule_replace_on_reboot(from: &Path, to: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};

    let staging = to.with_extension("dll.new");
    if std::fs::copy(from, &staging).is_err() {
        return false;
    }
    let wide = |p: &Path| -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    };
    let (a, b) = (wide(&staging), wide(to));
    unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), MOVEFILE_DELAY_UNTIL_REBOOT) != 0 }
}

#[cfg(not(windows))]
fn schedule_replace_on_reboot(_from: &Path, _to: &Path) -> bool {
    false
}

/// 把一个 dll 放进系统目录。顺序：
/// 1. 直接复制；
/// 2. 被占用 → 强关占用它的进程 → 再复制；
/// 3. 还写不进去 → 把旧文件改名让位，新文件放到原名上（不用重启）；
/// 4. 最后才安排到下次重启替换。
/// 三条都不通的报错文案里带上两个完整路径。
fn copy_into_place(from: &Path, to: &Path) -> Result<(), String> {
    match std::fs::copy(from, to) {
        Ok(_) => Ok(()),
        Err(e) => {
            let who = kill_lockers(&[to.to_path_buf()]);
            if !who.is_empty() {
                log(&format!("{} 被 {} 占用，先关掉它们", to.display(), who.join("、")));
            }
            if std::fs::copy(from, to).is_ok() {
                return Ok(());
            }
            // 还有程序把 dll 映射着（关不掉，或者关掉会影响用户正在做的事）。
            // NTFS 允许改名一个已被加载的文件：映射照旧读老文件，新加载的程序读新的。
            // 所以换名让位就行，不需要等重启。
            if rename_aside_and_copy(from, to) {
                log(&format!("{} 换不了，已把旧文件改名让位，新文件直接生效（不用重启）", to.display()));
                return Ok(());
            }
            if schedule_replace_on_reboot(from, to) {
                log(&format!("{} 当前换不了，已安排下次重启时替换", to.display()));
                REBOOT_NEEDED.store(true, Ordering::SeqCst);
                return Ok(());
            }
            Err(format!(
                "{} 复制到 {} 失败：{e}（占用它的进程也没关掉，关掉程序或重启后再运行一次）",
                from.display(),
                to.display()
            ))
        }
    }
}

/// 让路式替换：`msvcr71.dll` → `msvcr71.dll.old`，然后把新的写到原名上。
/// 旧的让位副本挂着没人再映射之后会自己可用，但正在被映射时删不掉，
/// 那就登记"下次重启时删掉"，不留垃圾在系统目录里。
fn rename_aside_and_copy(from: &Path, to: &Path) -> bool {
    let Some(name) = to.file_name() else {
        return false;
    };
    let mut aside_name = name.to_os_string();
    aside_name.push(".old");
    let aside = to.with_file_name(aside_name);

    // 上一轮留下的 .old 还在（还没重启过）：先清掉，不然改名会失败
    let _ = std::fs::remove_file(&aside);
    let _ = schedule_delete_on_reboot(&aside);

    if std::fs::rename(to, &aside).is_err() {
        return false;
    }
    match std::fs::copy(from, to) {
        Ok(_) => true,
        Err(_) => {
            // 放不进去就把旧的改回来，别把系统目录弄成"文件不见了"
            let _ = std::fs::rename(&aside, to);
            false
        }
    }
}

/// 让 Windows 下次重启时把这条路删掉（MoveFileEx 的新名字给 NULL 就是"删"）。
fn schedule_delete_on_reboot(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
        let old: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        return unsafe {
            MoveFileExW(old.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) != 0
        };
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// 打包后从资源目录找；开发时直接指项目里的 payload（也能用 CLACK_PAYLOAD 覆盖）
fn payload_root(app: &AppHandle) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CLACK_PAYLOAD") {
        let p = PathBuf::from(p);
        if p.is_dir() {
            return Some(p);
        }
    }
    #[cfg(not(debug_assertions))]
    if let Some(p) = ensure_payload() {
        return Some(p);
    }
    if let Ok(dir) = app.path().resource_dir() {
        let p = dir.join("payload");
        if p.is_dir() {
            return Some(p);
        }
    }
    for rel in ["../../_work/payload", "../../payload", "../payload", "payload"] {
        let p = PathBuf::from(rel);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

// ---------- 安装 ----------

/// msiexec / vc_redist 的成功码：0 成功、3010/1641 成功待重启、1638 已有别的版本
fn code_ok(code: Option<i32>) -> bool {
    matches!(code, Some(0) | Some(3010) | Some(1641) | Some(1638))
}

/// 失败码翻译成人话，贴在界面那行后面。数字本身说明不了任何事，
/// 而这几条恰恰是网上答案最统一的一类故障，先给用户一句结论。
fn hint(code: i32) -> &'static str {
    match code {
        1612 => "（这台机器上还留着旧的安装记录，当初那个安装源已经不在了）",
        1610 => "（已装那套运行库的注册信息坏了）",
        1619 => "（安装包打不开：路径或权限）",
        1603 => "（安装过程报错，明细在这个组件的 install.log 里）",
        1605 => "（系统里没有这个产品，只在卸载时才会遇到）",
        1618 => "（这台机器上还有另一个安装正在进行，关掉它再试）",
        1730 => "（包里的一个文件对不上，多半是源目录不全）",
        1925 => "（要管理员权限才能装）",
        _ => "",
    }
}

/// vc_redist 那三个自解压安装器（2012 / 2013 / 2015-2022）失败时的说法和自己一套。
/// 它们的日志写在 `%TEMP%\dd_vcredist*.log`，不在组件目录里。
fn hint_exe(code: i32) -> &'static str {
    match code {
        1603 => "（安装器报致命错误，明细见 %TEMP%\\dd_vcredist_*.log）",
        1638 => "（这台机器上已经装了更新或同版本，这次没动它）",
        1925 => "（要管理员权限才能装）",
        _ => hint(code),
    }
}

/// 跑一次 msiexec。verb 是 `/i`（装）、`/f`（修复）或 `/p`（打补丁），
/// 日志固定写到 log_path。返回 msiexec 的退出码；进程都起不来才算 Err。
/// `target` 可以是一个包（`.msi` / `.msp`），也可以是产品码（`/X` 卸载时用）。
fn msiexec(target: &std::ffi::OsStr, verb: &str, props: &[&str], log_path: &Path) -> Result<i32, String> {
    let st = Command::new("msiexec.exe")
        .arg(verb)
        .arg(target)
        .args(["/qn", "/norestart"])
        .args(props)
        .arg("/l*v")
        .arg(log_path)
        .status()
        .map_err(|e| format!("启动 msiexec 失败：{e}"))?;
    Ok(st.code().unwrap_or(-1))
}

/// 1612：Windows 记着这套运行库"已经装过"，但缓存包（`C:\Windows\Installer\*.msi`）
/// 和记录里的源目录都不在了。此后的重装是"修旧记录"——它按记录里的源找安装包，
/// 不看我们命令行上给的这份，所以稳定失败。
///
/// 用户那台的现场日志把顺序写得很清楚：
/// ```text
/// Warning: Local cached package 'C:\Windows\Installer\10125216.msi' is missing.
/// SOURCEMGMT: Attempting to use LastUsedSource from source list.
/// SOURCEMGMT: Trying source d:\fa9f15c337616bda2f4885\.
/// MSI (s): Source is invalid due to missing/inaccessible package.
/// MSI (s): SOURCEMGMT: Failed to resolve source
/// MSI (s): MainEngineThread is returning 1612
/// ```
/// 注意它**先**认 `LastUsedSource`（注册表 `…\Installer\Products\<键>\SourceList\LastUsedSource`，
/// 形如 `n;4;C:\ProgramData\Click\vc2008_x86\`；本地目录是登记在 `Net` 列表里的，
/// 不是 `Media`）。所以两条路，按命中率排：
///
/// 1. 把包放回记录里那个目录（它要的就是这个，不需要跟它讲道理）；
/// 2. 退一步用官方 API 追加源、并把 `LastUsedSource` 改写指向我们的目录，再 `/f` 强装。
///
/// 每一步的返回码都写进日志：这件事只能靠现场日志判断成没成。
fn msi_retry_with_source(c: &Comp, msi: &Path, stage: &Path, first_log: &Path, code: i32) -> Result<i32, String> {
    log(&format!(
        "msiexec 返回 {code}（旧的安装记录还在，但缓存包/安装源已经不在了）。日志：{}",
        first_log.display()
    ));
    if c.pcode.is_empty() {
        return Ok(code);
    }

    let recorded = msi_last_used_source(c.pcode);
    match &recorded {
        Some(v) => log(&format!("这台机器记的安装源：{v}")),
        None => log("读不出这台机器记的安装源"),
    }

    // 路 1：把包放回它记的那个目录
    if let Some(dir) = recorded.as_deref().and_then(path_of_source_record) {
        if stage.is_dir() {
            match restore_source_dir(&dir, stage) {
                Ok(()) => {
                    log(&format!("已把安装包放回 {}", dir.display()));
                    let rc = msiexec(msi.as_os_str(), "/i", &[], &stage.join("install-retry.log"))?;
                    if code_ok(Some(rc)) {
                        log("把包放回原来那个源目录之后装上了");
                        return Ok(rc);
                    }
                    log(&format!("把包放回去还是 {rc}，日志：{}", stage.join("install-retry.log").display()));
                }
                Err(e) => log(&format!("往 {} 里放安装包没成功：{e}", dir.display())),
            }
        }
    }

    // 路 2：追加源 + 改写 LastUsedSource，然后强制重装
    let src = format!("{}\\", stage.display());
    if !add_msi_source(c.pcode, &src) {
        log("往安装源列表里加这一条没成功，还是照试");
    }
    msi_set_last_used_source(c.pcode, &src);
    if let Some(now) = msi_last_used_source(c.pcode) {
        log(&format!("改写之后记的安装源：{now}"));
    }

    // /f 的 pkg 位置参数就是"从哪儿读安装包"，这是最后能自己动手的一步；
    // REINSTALLMODE=vomus 覆盖全部文件，REINSTALL=ALL 覆盖全部组件
    let retry_log = stage.join("install-retry2.log");
    let retry_code = msiexec(
        msi.as_os_str(),
        "/f",
        &["REINSTALL=ALL", "REINSTALLMODE=vomus", "USING_EXUIH_SILENT=1"],
        &retry_log,
    )?;
    if code_ok(Some(retry_code)) {
        log(&format!("强制重装修复成功，{} 装上了", c.id));
    } else {
        log(&format!(
            "强制重装修复还是 {retry_code}，日志：{}",
            retry_log.display()
        ));
    }
    Ok(retry_code)
}

/// 最后一条路：把这套运行库的旧记录摘掉，再从我们的包全新装一遍。
///
/// 学的是那些老合集的套路（`MsiExec /qn /X{产品码}` 然后重装）：全新安装不需要去翻旧安装源，
/// 1612 那条死路自然就绕开了。代价是这一代运行库会先被摘掉，所以顺序排在最后——
/// 前面那些"不动现状"的补救（补源、放回原目录、强制重装）都失败才轮得到它。
fn remove_then_install(c: &Comp, msi: &Path, stage: &Path, first_code: i32) -> Result<i32, String> {
    let remove_log = stage.join("remove.log");
    let rc = msiexec(std::ffi::OsStr::new(c.pcode), "/X", &[], &remove_log)?;
    log(&format!(
        "摘掉 {} 的旧记录：msiexec /X 返回 {rc}（日志 {}）",
        c.id,
        remove_log.display()
    ));
    // 0 = 摘掉了；1605 = 本来就没装；3010 / 1641 = 摘掉了但要求重启。这几种都可以接着装
    if !matches!(rc, 0 | 1605 | 3010 | 1641) {
        log("旧记录没摘掉，这条路放弃");
        return Ok(first_code);
    }
    let install_log = stage.join("install-after-remove.log");
    let after = msiexec(msi.as_os_str(), "/i", &[], &install_log)?;
    if code_ok(Some(after)) {
        log(&format!("摘掉旧记录之后装上了（返回 {after}）"));
    } else {
        log(&format!(
            "摘掉旧记录之后再装还是 {after}，日志：{}。这一代现在是被摘掉的状态，让用户重启之后再跑一次",
            install_log.display()
        ));
    }
    Ok(after)
}

/// 从来源记录 `n;4;C:\dir\` 里取出那个目录。不是本地盘路径（UNC、URL）就不碰。
fn path_of_source_record(record: &str) -> Option<PathBuf> {
    let path = record.rsplit(';').next()?.trim();
    let bytes = path.as_bytes();
    // 只认 `X:\...` 这种本地绝对路径，别的（`\\server\share`、http）留给别的办法
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return None;
    }
    let drive = PathBuf::from(&path[..2]);
    if !drive.is_dir() {
        // 记录里的盘不在（U 盘拔了、分区删了），这条路走不通
        return None;
    }
    Some(PathBuf::from(path))
}

/// 把 MSI / CAB / MSP 原样放到那个目录里——Windows 要的就是"记录里的源目录里有这个包"。
fn restore_source_dir(dir: &Path, stage: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for e in std::fs::read_dir(stage)?.flatten() {
        let p = e.path();
        if p.is_file() && has_ext(&p, MSI_EXTS) {
            std::fs::copy(&p, dir.join(e.file_name()))?;
        }
    }
    Ok(())
}

/// 这台机器为这个产品记着的"上次用的安装源"。
fn msi_last_used_source(product: &str) -> Option<String> {
    use windows_sys::Win32::System::ApplicationInstallationAndServicing::{
        MsiSourceListGetInfoW, MSIINSTALLCONTEXT_MACHINE,
    };
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let (product, prop) = (wide(product), wide("LastUsedSource"));
    unsafe {
        let mut need: u32 = 0;
        // 先问长度（返回 ERROR_MORE_DATA，need 里是字符数）
        MsiSourceListGetInfoW(
            product.as_ptr(),
            std::ptr::null(),
            MSIINSTALLCONTEXT_MACHINE,
            0,
            prop.as_ptr(),
            std::ptr::null_mut(),
            &mut need,
        );
        if need == 0 {
            return None;
        }
        let mut buf = vec![0u16; need as usize + 1];
        let mut len = buf.len() as u32;
        let rc = MsiSourceListGetInfoW(
            product.as_ptr(),
            std::ptr::null(),
            MSIINSTALLCONTEXT_MACHINE,
            0,
            prop.as_ptr(),
            buf.as_mut_ptr(),
            &mut len,
        );
        if rc != 0 {
            return None;
        }
        let end = buf.iter().position(|c| *c == 0).unwrap_or(len as usize);
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

/// 把 `LastUsedSource` 改成我们的目录。
/// 取值形式先按 `MsiSourceListGetInfo` 读出来那样给（就是一条纯路径），
/// 不行再试注册表里那种带类型的写法。
fn msi_set_last_used_source(product: &str, source_dir: &str) {
    use windows_sys::Win32::System::ApplicationInstallationAndServicing::{
        MsiSourceListSetInfoW, MSIINSTALLCONTEXT_MACHINE,
    };
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let (product, prop) = (wide(product), wide("LastUsedSource"));
    for (form, text) in [("纯路径", source_dir.to_string()), ("n;1; 带类型", format!("n;1;{source_dir}"))] {
        let value = wide(&text);
        let rc = unsafe {
            MsiSourceListSetInfoW(
                product.as_ptr(),
                std::ptr::null(),
                MSIINSTALLCONTEXT_MACHINE,
                0,
                prop.as_ptr(),
                value.as_ptr(),
            )
        };
        log(&format!("把 LastUsedSource 改写成 {source_dir}（{form}）：返回 {rc}"));
        if rc == 0 {
            return;
        }
    }
}

/// 把我们的目录补进这个产品的"安装源列表"（官方 API：MsiSourceListAddSourceEx）。
///
/// 为什么需要它：Windows 对"已经装过"的 MSI 产品，重装时会先去记录的源目录找安装包，
/// 而不是用我们命令行上给的这个包。机器上那个源目录要是没了（别人家的合集从临时目录装的、
/// U 盘、解压完就删的临时文件夹），就会一直回 1612；把我们的目录加进去，它才找得到。
///
/// 上下文必须给对：vcredist 都是按"每台机器"装的，源列表按上下文分开存，机器上下文是 4，
/// 当前用户那份是 2（实测这类产品在 2 上直接回 1605"没这个产品"，所以只写机器那份）。
/// 返回码原样写进日志——返回码是排查这件事唯一的线索。
fn add_msi_source(product: &str, source_dir: &str) -> bool {
    use windows_sys::Win32::System::ApplicationInstallationAndServicing::{
        MsiSourceListAddSourceExW, MSIINSTALLCONTEXT_MACHINE,
    };
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let (product, source) = (wide(product), wide(source_dir));
    // dwOptions = 0：只往上加，不动已有的条目
    let rc = unsafe {
        MsiSourceListAddSourceExW(
            product.as_ptr(),
            std::ptr::null(),
            MSIINSTALLCONTEXT_MACHINE,
            0,
            source.as_ptr(),
            0,
        )
    };
    log(&format!("把 {source_dir} 加进安装源列表：返回 {rc}"));
    rc == 0
}

/// 系统里挂着的"下次重启时才做的文件操作"（MSI 和 MoveFileEx 都往这里写）。
/// 读 `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\PendingFileRenameOperations[2]`。
fn pending_file_ops() -> Vec<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::{
            RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE,
        };
        let mut out = Vec::new();
        let sub: Vec<u16> = "SYSTEM\\CurrentControlSet\\Control\\Session Manager\0"
            .encode_utf16()
            .collect();
        unsafe {
            let mut key = std::ptr::null_mut();
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, sub.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) != 0 {
                return out;
            }
            for entry in ["PendingFileRenameOperations", "PendingFileRenameOperations2"] {
                let name: Vec<u16> = entry.encode_utf16().chain(std::iter::once(0)).collect();
                let mut len: u32 = 0;
                // 先问长度
                if RegQueryValueExW(
                    key,
                    name.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut len,
                ) != 0
                    || len == 0
                {
                    continue;
                }
                let mut buf = vec![0u8; len as usize];
                if RegQueryValueExW(
                    key,
                    name.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    buf.as_mut_ptr(),
                    &mut len,
                ) != 0
                {
                    continue;
                }
                let units: Vec<u16> = buf
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                for part in units.split(|c| *c == 0) {
                    if !part.is_empty() {
                        out.push(String::from_utf16_lossy(part));
                    }
                }
            }
            RegCloseKey(key);
        }
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// 这一轮里"待重启文件操作"变了吗。没变 = 机器本来就那样。
fn reboot_is_ours() -> bool {
    let before = PENDING_BEFORE.get_or_init(pending_file_ops);
    pending_file_ops() != *before
}

/// 安装器回 3010 / 1641 时，决定要不要在完成屏上提示重启。
///
/// 3010 有两个来源：安装器真的把替换推到了重启（这是我们的事），
/// 和这台机器**本来就**挂着别的待重启操作（Windows 更新、游戏服务、别的安装器
/// 都会往 `PendingFileRenameOperations` 里塞；只要它非空，后面装的 MSI 一律回 3010）。
/// 后者提示用户没有意义，还会让人以为咔哒装坏了，所以按"本轮前后有没有变化"区分。
fn flag_reboot(code: i32) {
    if code == 1641 {
        // 安装器已经把机器带进重启流程了，这个必须说
        REBOOT_NEEDED.store(true, Ordering::SeqCst);
        return;
    }
    if reboot_is_ours() {
        REBOOT_NEEDED.store(true, Ordering::SeqCst);
    } else {
        log("安装器说需要重启，但那是机器上本来就挂着的待重启操作（不是这次装出来的），不提示用户");
    }
}

/// 系统 UCRT / API Set 能不能用。
///
/// 注意判据不能是"文件在不在"：Win10/11 正常情况下**不需要**真有 `api-ms-win-*.dll`
/// （由系统的 API Set 表解析），所以只能真的试着加载一次。
/// 加载不了 = 这台机器的系统组件被精简或损坏了，装多少运行库都白搭，得走 sfc / DISM。
#[cfg(windows)]
fn ucrt_problem() -> Option<String> {
    use windows_sys::Win32::Foundation::FreeLibrary;
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;

    let mut bad: Vec<&str> = Vec::new();
    for name in [
        "ucrtbase.dll",
        "api-ms-win-crt-runtime-l1-1-0.dll",
        "api-ms-win-crt-stdio-l1-1-0.dll",
    ] {
        let w: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let h = unsafe { LoadLibraryW(w.as_ptr()) };
        if h.is_null() {
            bad.push(name);
        } else {
            unsafe { FreeLibrary(h) };
        }
    }
    if bad.is_empty() {
        None
    } else {
        Some(bad.join("、"))
    }
}

#[cfg(not(windows))]
fn ucrt_problem() -> Option<String> {
    None
}

fn run_one(root: &Path, c: &Comp) -> Result<(), String> {
    let dir = root.join(c.dir);
    if !dir.is_dir() {
        return Err(format!(
            "组件目录不存在（payload 可能被清理或被杀毒软件删了）：{}",
            dir.display()
        ));
    }
    match c.kind {
        Kind::Dll => {
            let dest = target_dir(c.arch);
            for f in c.files {
                let from = dir.join(f);
                if !from.is_file() {
                    return Err(format!("payload 里缺 {}", from.display()));
                }
                let to = dest.join(f);
                // 强校验（默认就做，不留开关）：散装 dll 我们手里那份永远是最终版
                // （VB5/VB6 停在 1998-2004，CRT 70/71 停在 2003），所以直接按"内容对不对"判：
                //   · 系统里没有 → 复制
                //   · 版本比我们旧 → 覆盖
                //   · 版本一样但大小不一样 = 文件被改坏了 → 覆盖   ← 这一步就是"强检测"
                //   · 版本一样、大小一样 → 校验通过，不动
                //   · 版本比我们新 → 不动（微软后的更新、别的程序钉住的版本留着）
                // 正在被加载的 dll 直接覆盖会共享冲突，那就让占用它的进程先退出再试一次，
                // 还不行就安排到下次重启时替换——用户那边顶多是"这次没生效"。
                let want_size = std::fs::metadata(&from).map(|m| m.len()).unwrap_or(0);
                let got_size = std::fs::metadata(&to).map(|m| m.len()).unwrap_or(0);
                let verdict = match (file_version(&from), file_version(&to)) {
                    (Some(pv), Some(tv)) if tv > pv => "更新",
                    (Some(pv), Some(tv)) if tv == pv => {
                        if want_size == got_size {
                            "同版本同大小"
                        } else {
                            "同版本但内容不对"
                        }
                    }
                    (Some(_), Some(_)) => "旧版",
                    _ => "读不出内容",
                };
                if verdict == "更新" || verdict == "同版本同大小" {
                    log(&format!("{} 校验通过（{verdict}）", to.display()));
                    continue;
                }
                let existed = to.is_file();
                if let Err(e) = copy_into_place(&from, &to) {
                    return Err(e);
                }
                if existed {
                    log(&format!("{} {verdict}，已按强检测覆盖", to.display()));
                }
                // 核一下大小，别把"半截文件"当成装好了（杀毒软件拦下来的常见表现）
                let got = std::fs::metadata(&to).map(|m| m.len()).unwrap_or(0);
                if want_size != got {
                    return Err(format!(
                        "{} 复制后大小不对（{got} != {want_size}），可能被杀毒软件拦了",
                        to.display()
                    ));
                }
            }
            Ok(())
        }
        Kind::Msi => {
            let pkg = dir.join(c.files[0]);
            if !pkg.is_file() {
                return Err(format!(
                    "安装包缺失（多半被杀毒软件删了）：{}",
                    pkg.display()
                ));
            }
            // MSI 不能直接从非 ASCII 路径装：Windows Installer 服务是独立进程，
            // 路径在里面要过一遍 ANSI，中文路径到那儿就废了，msiexec 只会回 1619（包打不开）。
            // 所以先把包和它的 cab / msp 一起拷到一个纯 ASCII 的临时目录，再从那里装。
            // 放 ProgramData 而不是 Windows\Temp：同样是纯 ASCII 路径，
            // 但普通权限读得到，出问题用户/客服不用提权就能把日志发出来
            let stage = click_dir().join(c.id);
            std::fs::create_dir_all(&stage).map_err(|e| format!("建临时目录失败：{e}"))?;
            for e in std::fs::read_dir(&dir).map_err(|e| format!("读组件目录失败：{e}"))?.flatten() {
                let p = e.path();
                if has_ext(&p, MSI_EXTS) {
                    let to = stage.join(p.file_name().unwrap_or_default());
                    std::fs::copy(&p, &to).map_err(|e| format!("复制 {} 失败：{e}", p.display()))?;
                }
            }
            let msi = stage.join(c.files[0]);
            let mut msi_log = stage.join("install.log");
            // USING_EXUIH_SILENT=1 是官方自解压包跑 MSI 时会设的属性。VC++ 2008 那份
            // vc_red.msi 里有个守卫动作，条件是
            //   NOT( (USING_EXUIH = 1 OR USING_EXUIH_SILENT = 1 OR ProductState >= 1) )
            // 命中就弹"请运行 Setup.exe"然后 1603 —— 干净机器上（这套运行库还没装过、
            // ProductState 为 0）必然命中。别的几代不认这个属性，给了也不碍事。
            const SILENT_PROPS: &[&str] = &["USING_EXUIH_SILENT=1"];
            let mut code = msiexec(msi.as_os_str(), "/i", SILENT_PROPS, &msi_log)?;

            // 1612 = 记录还在、缓存包/安装源没了；1610 = 记录坏了；1728 = 包里的
            // 某个文件找不到（同样是源那一侧的问题）。都走"补源 + 从包本身强制重装"。
            if !code_ok(Some(code)) && matches!(code, 1612 | 1610 | 1728) {
                msi_log = stage.join("install-retry.log");
                code = msi_retry_with_source(c, &msi, &stage, &stage.join("install.log"), code)?;
            }

            // 最后一条路，学那些老合集的：`MsiExec /qn /X{产品码}` 把这套运行库的旧记录摘掉，
            // 再从我们手上的包做一次全新安装——全新安装不会去翻旧安装源，1612 自然没有了。
            // 放在最后是因为它会把这套运行库先从机器上摘掉：前面那些不破坏现状的补救都试过才轮得到它。
            if !code_ok(Some(code)) && matches!(code, 1612 | 1610) && !c.pcode.is_empty() {
                code = remove_then_install(c, &msi, &stage, code)?;
                msi_log = stage.join("install-after-remove.log");
            }

            // 1603 是 MSI 的"通用致命错误"：文件写不进去、组件注册不上、被占用……死因只在那份
            // install.log 里。这里按顺序补两枪：
            //   1) 强制重装：REINSTALLMODE=vomus 会把所有文件重写一遍，绕过版本和哈希判断
            //      （"某个文件写不进去"这一类最常见的就是它治好的）；
            //   2) 还不行就摘掉旧记录再来一次。
            if matches!(code, 1603) {
                let killed = clear_holders(c);
                if !killed.is_empty() {
                    log(&format!("1603：先关掉占用它的 {}，再强制重写一遍所有文件", killed.join("、")));
                }
                msi_log = stage.join("install-forced.log");
                code = msiexec(
                    msi.as_os_str(),
                    "/f",
                    &["REINSTALL=ALL", "REINSTALLMODE=vomus", "USING_EXUIH_SILENT=1"],
                    &msi_log,
                )?;
                if code_ok(Some(code)) {
                    log("1603：强制重装之后装上了");
                } else if !c.pcode.is_empty() {
                    code = remove_then_install(c, &msi, &stage, code)?;
                    msi_log = stage.join("install-after-remove.log");
                }
            }

            if !code_ok(Some(code)) {
                log(&format!("msiexec 日志：{}", msi_log.display()));
                if code == 1612 {
                    log("1612 = 系统里还留着旧的安装记录，但当初的安装源已经不在。\
                         补源重装也没成的话，先用微软官方的「程序安装和卸载疑难解答」清掉那几条旧记录，再跑一次");
                }
                return Err(format!("msiexec 返回 {code}{}", hint(code)));
            }
            // 3010 = "装好了，但要重启才落位"。这一轮要是我们确实往"待重启操作"里加了东西，
            // 说明安装器真把替换推给了重启（多半还有人加载着这一代的 dll）。
            // 那就再杀一轮占用、重装一次：能当场落位就别麻烦用户重启。
            if code == 3010 && reboot_is_ours() {
                let killed = clear_holders(c);
                if !killed.is_empty() {
                    log(&format!(
                        "{} 说要把替换留到重启，先把占用它的 {} 关掉，再装一次",
                        c.id,
                        killed.join("、")
                    ));
                }
                msi_log = stage.join("install-after-kill.log");
                code = msiexec(msi.as_os_str(), "/i", &[], &msi_log)?;
            }
            // 成功也分几种：0 是真装上了；1638 是"这台机器上已经有一套，这次没动"；
            // 3010 / 1641 是"装上了但要重启才落位"，界面得说一声
            if code != 0 {
                log(&format!(
                    "msiexec 返回 {code}：{}",
                    match code {
                        1638 => "这台机器上已经有这套运行库的另一个版本，这次没动它",
                        3010 | 1641 => "装上了，但系统要求重启之后才生效",
                        _ => "按成功处理",
                    }
                ));
                if matches!(code, 3010 | 1641) {
                    flag_reboot(code);
                }
            }
            // 强检测修复（默认就做）：装完再让安装器自己把每个文件过一遍——
            // 版本、语言、大小，必要时按包里的哈希重写。这一步专治"文件坏了或被顶替，
            // 但安装器认为已经装好所以跳过"的情况，连 SxS 装配件和 policy 文件一起修，
            // 比自己往系统目录里拷 dll 干净得多。
            if code == 0 {
                let verify_log = stage.join("install-verify.log");
                let vc = msiexec(
                    msi.as_os_str(),
                    "/f",
                    &["REINSTALL=ALL", "REINSTALLMODE=vomus", "USING_EXUIH_SILENT=1"],
                    &verify_log,
                )?;
                if code_ok(Some(vc)) {
                    log(&format!("{} 强校验修复完成（返回 {vc}）", c.id));
                } else {
                    log(&format!(
                        "{} 强校验修复返回 {vc}，日志：{}",
                        c.id,
                        verify_log.display()
                    ));
                }
            }
            // 同目录里如果有 .msp 补丁就接着打上（2010 SP1 的 KB2565063 就是这样进来的）。
            // 只在基础包真装上了（code == 0）才打：1638 说明这次什么都没动，
            // 往上打补丁没有意义；而补丁本身回 1638 / 1642 只说明"已经打过 / 被更高版本
            // 顶掉了"，那是正常情况，不该把整行报红。
            if code == 0 {
                for e in std::fs::read_dir(&stage).map_err(|e| format!("读临时目录失败：{e}"))?.flatten() {
                    let p = e.path();
                    if !p.extension().map(|x| x.eq_ignore_ascii_case("msp")).unwrap_or(false) {
                        continue;
                    }
                    log(&format!("打补丁 {}", p.display()));
                    let patch_code = msiexec(p.as_os_str(), "/p", &[], &stage.join("patch.log"))?;
                    match patch_code {
                        0 => {}
                        1638 | 1642 => log(&format!(
                            "补丁返回 {patch_code}：已经打过，或被更高版本替代，跳过"
                        )),
                        3010 | 1641 => {
                            log("补丁装上了，但要求重启之后才生效");
                            flag_reboot(patch_code);
                        }
                        _ => return Err(format!("补丁返回 {patch_code}{}", hint(patch_code))),
                    }
                }
            }
            Ok(())
        }
        Kind::Exe => {
            let file = dir.join(c.files[0]);
            // 文件不见了就别让 Command 去猜：用户看到的"系统找不到指定的文件"就是这么来的
            if !file.is_file() {
                return Err(format!(
                    "安装器文件缺失（多半被杀毒软件删了）：{}",
                    file.display()
                ));
            }
            let st = Command::new(&file)
                .args(c.args)
                .status()
                .map_err(|e| format!("启动安装器失败：{e}（{}）", file.display()))?;
            // vc_redist 这种自解压包回的是 HRESULT（0x80070643 之类），低 16 位才是 MSI 那套码：
            // 0x80070666 其实就是 1638（机器上已经装了更高版本），不归一化就会把
            // "本来就装好了"报成红。
            let raw = st.code().unwrap_or(-1);
            let mut code = if (raw as u32) >= 0x8007_0000 { raw & 0xFFFF } else { raw };
            // 1603：vc_redist 是 burn 打包的，支持 `/repair`。先请占用让路，再让它自己修一遍
            if code == 1603 {
                let killed = clear_holders(c);
                if !killed.is_empty() {
                    log(&format!("1603：先关掉 {} 再让安装器修一遍", killed.join("、")));
                }
                let mut repair: Vec<&str> = c.args.to_vec();
                if let Some(first) = repair.first_mut() {
                    *first = "/repair";
                }
                if let Ok(st) = Command::new(&file).args(&repair).status() {
                    let raw = st.code().unwrap_or(-1);
                    code = if (raw as u32) >= 0x8007_0000 { raw & 0xFFFF } else { raw };
                    log(&format!("1603：/repair 之后返回 {code}"));
                }
            }
            // 和 MSI 那边一个道理：能当场落位就别让用户重启
            if code == 3010 && reboot_is_ours() {
                let killed = clear_holders(c);
                if !killed.is_empty() {
                    log(&format!("{} 要把替换留到重启，先关掉 {} 再装一次", c.id, killed.join("、")));
                }
                if let Ok(st) = Command::new(&file).args(c.args).status() {
                    let raw = st.code().unwrap_or(-1);
                    code = if (raw as u32) >= 0x8007_0000 { raw & 0xFFFF } else { raw };
                }
            }
            if code_ok(Some(code)) {
                if matches!(code, 1638) {
                    log(&format!("{}：这台机器上已经装了更新或同版本，这次没动它", c.id));
                }
                if matches!(code, 3010 | 1641) {
                    flag_reboot(code);
                }
                // 强检测修复（默认就做）：自解压包支持 /repair，让它把自己那份里的
                // 每个 MSI 和文件都校验一遍，坏了的重写回来。
                let mut repair: Vec<&str> = c.args.to_vec();
                if let Some(first) = repair.first_mut() {
                    *first = "/repair";
                }
                if let Ok(st) = Command::new(&file).args(&repair).status() {
                    let raw = st.code().unwrap_or(-1);
                    let rc = if (raw as u32) >= 0x8007_0000 { raw & 0xFFFF } else { raw };
                    log(&format!("{} 强校验修复（/repair）返回 {rc}", c.id));
                    if matches!(rc, 3010 | 1641) {
                        flag_reboot(rc);
                    }
                }
                Ok(())
            } else {
                Err(format!("安装器返回 {code}{}", hint_exe(code)))
            }
        }
    }
}

fn run_install(app: AppHandle, rows: Vec<String>) {
    run_install_inner(&app, rows);
    BUSY.store(false, Ordering::SeqCst);
}

fn run_install_inner(app: &AppHandle, rows: Vec<String>) {
    // 没有管理员权限就别往下跑：msiexec 会回 1925、复制进 System32 会拒绝访问，
    // 最后用户只看到一堆数字，猜不到"是没给管理员权限"。
    if !elevated() {
        log("没有管理员权限，拒绝开始安装");
        let _ = app.emit(
            "install:done",
            Done {
                ok: false,
                failed: vec!["需要管理员权限：关掉重开一次，在弹窗里点「是」".into()],
                reboot: false,
            },
        );
        return;
    }
    // 记下动手之前系统里已有的"待重启文件操作"：后面区分 3010 是不是我们造成的
    PENDING_BEFORE.get_or_init(pending_file_ops);
    let Some(root) = payload_root(app) else {
        let _ = app.emit(
            "install:done",
            Done {
                ok: false,
                failed: vec!["找不到 payload 目录".into()],
                reboot: false,
            },
        );
        return;
    };
    // 展开成具体组件，全部照装（安装器自己幂等）
    let list: Vec<(&str, &Comp)> = COMPS
        .iter()
        .filter(|c| rows.iter().any(|r| r == c.group))
        .map(|c| (c.group, c))
        .collect();
    let total = list.len();
    let mut failed = Vec::new();

    for (index, (row, c)) in list.iter().enumerate() {
        log(&format!("开始 {} ({})", c.id, c.name));
        let _ = app.emit(
            "install:progress",
            Progress {
                row: (*row).into(),
                id: c.id.into(),
                state: "run",
                index,
                total,
                message: None,
            },
        );
        // 要用的文件不在就先自愈一次（杀毒删文件、解包被打断都见过），重新解包再跑。
        // 一轮里只重解一次：多个组件同时缺的时候，重解一次就该全好了。
        if !root.join(c.dir).join(c.files[0]).is_file()
            && !REEXTRACTED.swap(true, Ordering::SeqCst)
            && refresh_payload()
        {
            log(&format!("{} 的文件缺失，已重新解包 payload", c.id));
        }

        match run_one(&root, c) {
            Ok(()) => {
                log(&format!("完成 {}", c.id));
                let _ = app.emit(
                    "install:progress",
                    Progress {
                        row: (*row).into(),
                        id: c.id.into(),
                        state: "done",
                        index,
                        total,
                        message: None,
                    },
                );
            }
            Err(message) => {
                log(&format!("失败 {}: {message}", c.id));
                failed.push(c.id.to_string());
                let _ = app.emit(
                    "install:progress",
                    Progress {
                        row: (*row).into(),
                        id: c.id.into(),
                        state: "fail",
                        index,
                        total,
                        message: Some(message),
                    },
                );
            }
        }
    }

    // 系统组件体检（默认就做）：UCRT / API Set 解析不了的话，装多少运行库都没用，
    // 得在界面上明说，别让用户以为工具修好了。
    if let Some(names) = ucrt_problem() {
        log(&format!("系统 UCRT/API Set 加载失败：{names}"));
        failed.push(format!(
            "系统组件损坏：{names} 加载不了。请用管理员命令行运行 sfc /scannow 和 DISM /Online /Cleanup-Image /RestoreHealth"
        ));
    }
    let _ = app.emit(
        "install:done",
        Done {
            ok: failed.is_empty(),
            failed,
            reboot: REBOOT_NEEDED.load(Ordering::SeqCst),
        },
    );
}

// ---------- 权限 ----------

#[cfg(windows)]
fn elevated() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut info = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && info.TokenIsElevated != 0
    }
}

/// 用 runas 重新拉起自己。返回 true 表示已经拉起来了，调用方应当退出。
#[cfg(windows)]
fn relaunch_elevated() -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let verb = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let params = wide("--elevated");

    // 拉起新进程后本进程就要退场了，这时候不能清目录：新进程还要用
    RELAUNCHING.store(true, Ordering::SeqCst);
    unsafe {
        let r = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        // <= 32 是失败码。失败说明 UAC 被拒 / 被策略拦，本进程还得继续活着，
        // 那就别再拦着自己的清理逻辑——不然这次关窗口 payload 会一直留着
        let ok = r as isize > 32;
        if !ok {
            RELAUNCHING.store(false, Ordering::SeqCst);
        }
        ok
    }
}

// ---------- 命令 ----------

#[tauri::command]
fn scan(app: AppHandle) -> Vec<GroupInfo> {
    let _ = payload_root(&app);
    let mut out: Vec<GroupInfo> = Vec::new();
    for c in COMPS {
        match out.iter_mut().find(|g| g.id == c.group) {
            Some(g) => {
                g.ids.push(c.id);
            }
            None => out.push(GroupInfo {
                // 每组的第一个是 x86，名字不带后缀，正好当这一组的显示名
                id: c.group,
                name: c.name,
                version: c.version,
                ids: vec![c.id],
            }),
        }
    }
    out
}

#[tauri::command]
fn install(app: AppHandle, rows: Vec<String>) {
    // 连点两下的守卫：第二次点击直接丢掉。
    // 两个线程一起装的话，进度事件会互相覆盖，先结束的那个还会把 BUSY 归零，
    // 让用户在另一个线程还在跑的时候关窗——退出时的清理会把它正在用的文件删掉。
    if BUSY.swap(true, Ordering::SeqCst) {
        log("安装已经在跑了，忽略这次重复的请求");
        return;
    }
    std::thread::spawn(move || run_install(app, rows));
}

#[tauri::command]
fn is_elevated() -> bool {
    elevated()
}

/// 完成屏上的「立刻重启」：先把自己摊出来的东西收走，再让 Windows 重启。
///
/// 走 API 而不是 `shutdown.exe`。现场踩到过：从我们这个（GUI、已提权的）进程里
/// 把 shutdown.exe 当子进程拉起来，它会以 `0xc0000142`（STATUS_DLL_INIT_FAILED）
/// 挂在加载阶段——继承的句柄加上杀毒软件对子进程的钩子都可能让它这样。
/// 调 `ExitWindowsEx` 没有这一层，也少开一个控制台窗口。
#[tauri::command]
fn restart_now() -> bool {
    log("用户选了立刻重启");
    cleanup();
    if reboot_now() {
        return true;
    }
    // 兜底：API 那步不成（权限被策略收走之类），再试一次 shutdown.exe
    log("ExitWindowsEx 没成，退回 shutdown.exe 再试一次");
    let ok = Command::new("shutdown.exe")
        .args(["/r", "/t", "0"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        log("重启没能自动完成，让用户自己重启");
    }
    ok
}

/// 打开当前进程令牌里的 `SeShutdownPrivilege`。管理员令牌默认带着它，但是禁用状态，
/// `ExitWindowsEx` 要求它是打开状态。
#[cfg(windows)]
fn enable_shutdown_privilege() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        ) == 0
        {
            log("拿不到自己的令牌，重启这步跳过");
            return false;
        }
        let mut luid = std::mem::zeroed();
        if LookupPrivilegeValueW(std::ptr::null(), SE_SHUTDOWN_NAME, &mut luid) == 0 {
            CloseHandle(token);
            log("查不到 SeShutdownPrivilege");
            return false;
        }
        let mut tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        // 这个 API 就算没成也可能返回 TRUE，所以要再看一次 GetLastError
        let ok = AdjustTokenPrivileges(
            token,
            0,
            &mut tp,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
            && GetLastError() == 0;
        if !ok {
            log("SeShutdownPrivilege 没能打开，重启大概率会失败");
        }
        CloseHandle(token);
        ok
    }
}

#[cfg(not(windows))]
fn enable_shutdown_privilege() -> bool {
    false
}

/// 立刻重启。
#[cfg(windows)]
fn reboot_now() -> bool {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Shutdown::{
        ExitWindowsEx, EWX_FORCEIFHUNG, EWX_REBOOT, SHTDN_REASON_MAJOR_APPLICATION,
    };

    unsafe {
        let _ = enable_shutdown_privilege();
        // EWX_FORCEIFHUNG：只在有程序卡住时才强关，正常情况下还是走正常的关机流程
        if ExitWindowsEx(EWX_REBOOT | EWX_FORCEIFHUNG, SHTDN_REASON_MAJOR_APPLICATION) != 0 {
            log("已请求系统重启");
            true
        } else {
            log(&format!("ExitWindowsEx 失败：{}", GetLastError()));
            false
        }
    }
}

#[cfg(not(windows))]
fn reboot_now() -> bool {
    false
}

/// 兜底退出：正常关窗口在极端情况下可能不生效（窗口失焦、事件没送到），
/// 那就直接结束自己。安装器是独立进程，不受影响。
#[tauri::command]
fn quit() {
    cleanup();
    std::process::exit(0);
}

/// 用系统默认程序打开链接。界面上的作者 / 仓库两个跳转键走这里。
/// 不用 WebView 的 window.open：那个在 WebView2 里要额外配权限，还不一定出得去。
#[tauri::command]
fn open_url(url: String) -> bool {
    // 只放行 http(s)。参数来自前端，别让这个命令变成"随便启动什么"的入口
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        log(&format!("拒绝打开非 http(s) 链接：{url}"));
        return false;
    }
    log(&format!("打开链接 {url}"));

    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
        let verb = wide("open");
        let target = wide(&url);
        let ok = unsafe {
            let r = ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
            // <= 32 是失败码
            r as isize > 32
        };
        if !ok {
            log("系统没能打开这个链接");
        }
        return ok;
    }

    #[cfg(not(windows))]
    {
        false
    }
}

/// 没管理员权限就重新以管理员身份拉起自己，返回 true 表示已经重启过了。
/// interactive = 用户按下了「安装」：开发模式下启动阶段不弹 UAC（每次重建都弹一次太烦），
/// 只在这一刻弹；正式版启动时就弹，用户看不到第二次点击。
#[tauri::command]
fn request_elevation(interactive: bool) -> bool {
    if elevated() {
        return false;
    }
    if cfg!(debug_assertions) && !interactive {
        return false;
    }
    relaunch_elevated()
}

pub fn run() {
    // 日志每次启动重写一份；然后认一下自己是不是第一个实例
    reset_log();
    claim_single_instance();
    log(&format!("咔哒 v{} 启动", env!("CARGO_PKG_VERSION")));

    tauri::Builder::default()
        .on_window_event(|_window, event| {
            // 安装中把关闭请求挡回去：Alt+F4、任务栏"关闭窗口"、我们自己的关闭键都会走到这里
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if BUSY.load(Ordering::SeqCst) {
                    api.prevent_close();
                    log("安装进行中，已忽略关闭请求");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            scan,
            install,
            is_elevated,
            request_elevation,
            restart_now,
            open_url,
            quit
        ])
        .build(tauri::generate_context!())
        .expect("启动失败")
        .run(|_app, event| {
            // 正常关窗口也走到这儿——把解出来的东西收走
            if let tauri::RunEvent::Exit = event {
                cleanup();
            }
        });
}

/// 抢一个全局互斥体，判断自己是不是这次运行里"第一个"实例。
/// 用户双击两遍 exe 时，第二个实例照样能用（他只是多点了一下），但**不负责清 payload**：
/// 它一清，正在装的那个实例就会当场"组件目录不存在"。
fn claim_single_instance() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::CreateMutexW;
        // ERROR_ALREADY_EXISTS
        const ALREADY_EXISTS: u32 = 183;
        let name: Vec<u16> = "Global\\ClackRun\0".encode_utf16().collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() || unsafe { windows_sys::Win32::Foundation::GetLastError() } == ALREADY_EXISTS
        {
            FIRST_INSTANCE.store(false, Ordering::SeqCst);
            log("已经有一个咔哒在跑：这次不负责清理解包目录");
        }
        // 句柄故意不关：进程活着期间这个锁就得在
    }
}

// 组件表 vs payload 的一致性自测（cargo test）
#[cfg(test)]
mod payload_test;
