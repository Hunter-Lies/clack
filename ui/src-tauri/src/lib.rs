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
}

// 不做"装没装"的判断，全部照装。安装器本身幂等（已有会返回 1638 / 0），
// 而漏装的代价远大于多花几十秒——检测一旦判错，用户就是拿着一台依然缺库的机器。
const COMPS: &[Comp] = &[
    Comp { id: "vb5", group: "vb5", name: "Visual Basic VM 5.x", version: "5.02.8244", arch: Arch::X86, kind: Kind::Dll,
        dir: "01_VB5_VM/x86", files: &["MSVBVM50.DLL"], args: &[] },
    Comp { id: "vb6", group: "vb6", name: "Visual Basic VM 6.0", version: "6.00.9802", arch: Arch::X86, kind: Kind::Dll,
        dir: "02_VB6_VM/x86", files: &["MSVBVM60.DLL"], args: &[] },
    Comp { id: "crt70", group: "crt70", name: "C Runtime 2002", version: "7.00.9981", arch: Arch::X86, kind: Kind::Dll,
        dir: "03_CRT_2002/x86",
        files: &["MSVCR70.DLL", "MSVCP70.DLL", "atl70.dll", "mfc70.dll", "mfc70u.dll", "mfc70chs.dll"], args: &[] },
    Comp { id: "crt71", group: "crt71", name: "C Runtime 2003", version: "7.10.3078", arch: Arch::X86, kind: Kind::Dll,
        dir: "04_CRT_2003/x86",
        files: &["MSVCR71.DLL", "MSVCP71.DLL", "atl71.dll", "MFC71.DLL", "mfc71u.dll", "mfc71chs.dll"], args: &[] },
    // 2005/2008/2010：装的是"MSI + 它自己的 CAB"，由 msiexec /qn 直接跑。
    // 不走官方的自解压 exe，是因为那层壳内部还会调 msiexec 弹进度框；
    // 而且这几代的 MSI 少了同目录的 CAB 就会失败（1620/1635），所以两个文件必须一起在。
    Comp { id: "vc2005_x86", group: "vc2005", name: "Visual C++ 2005 SP1", version: "8.0.61001", arch: Arch::X86, kind: Kind::Msi,
        dir: "05_VC2005/x86", files: &["vcredist.msi"], args: &[] },
    Comp { id: "vc2005_x64", group: "vc2005", name: "Visual C++ 2005 SP1 (x64)", version: "8.0.61001", arch: Arch::X64, kind: Kind::Msi,
        dir: "05_VC2005/x64", files: &["vcredist.msi"], args: &[] },
    Comp { id: "vc2008_x86", group: "vc2008", name: "Visual C++ 2008 SP1", version: "9.0.30729.6161", arch: Arch::X86, kind: Kind::Msi,
        dir: "06_VC2008/x86", files: &["vc_red.msi"], args: &[] },
    Comp { id: "vc2008_x64", group: "vc2008", name: "Visual C++ 2008 SP1 (x64)", version: "9.0.30729.6161", arch: Arch::X64, kind: Kind::Msi,
        dir: "06_VC2008/x64", files: &["vc_red.msi"], args: &[] },
    // 2010 的 SP1 补丁（KB2565063）就在同目录，装完 MSI 会自动接着打上
    Comp { id: "vc2010_x86", group: "vc2010", name: "Visual C++ 2010 SP1", version: "10.0.40219.473", arch: Arch::X86, kind: Kind::Msi,
        dir: "07_VC2010/x86", files: &["vc_red.msi"], args: &[] },
    Comp { id: "vc2010_x64", group: "vc2010", name: "Visual C++ 2010 SP1 (x64)", version: "10.0.40219.473", arch: Arch::X64, kind: Kind::Msi,
        dir: "07_VC2010/x64", files: &["vc_red.msi"], args: &[] },
    Comp { id: "vc2012_x86", group: "vc2012", name: "Visual C++ 2012 U4", version: "11.0.61135", arch: Arch::X86, kind: Kind::Exe,
        dir: "08_VC2012/x86", files: &["vc_redist.x86.exe"], args: &["/install", "/quiet", "/norestart"] },
    Comp { id: "vc2012_x64", group: "vc2012", name: "Visual C++ 2012 U4 (x64)", version: "11.0.61135", arch: Arch::X64, kind: Kind::Exe,
        dir: "08_VC2012/x64", files: &["vc_redist.x64.exe"], args: &["/install", "/quiet", "/norestart"] },
    Comp { id: "vc2013_x86", group: "vc2013", name: "Visual C++ 2013", version: "12.0.40664", arch: Arch::X86, kind: Kind::Exe,
        dir: "09_VC2013/x86", files: &["vc_redist.x86.exe"], args: &["/install", "/quiet", "/norestart"] },
    Comp { id: "vc2013_x64", group: "vc2013", name: "Visual C++ 2013 (x64)", version: "12.0.40664", arch: Arch::X64, kind: Kind::Exe,
        dir: "09_VC2013/x64", files: &["vc_redist.x64.exe"], args: &["/install", "/quiet", "/norestart"] },
    Comp { id: "vc2022_x86", group: "vc2022", name: "Visual C++ 2015-2022", version: "14.51.36247", arch: Arch::X86, kind: Kind::Exe,
        dir: "10_VC2015-2022_14.51/x86", files: &["vc_redist.x86.exe"], args: &["/install", "/quiet", "/norestart"] },
    Comp { id: "vc2022_x64", group: "vc2022", name: "Visual C++ 2015-2022 (x64)", version: "14.51.36247", arch: Arch::X64, kind: Kind::Exe,
        dir: "10_VC2015-2022_14.51/x64", files: &["vc_redist.x64.exe"], args: &["/install", "/quiet", "/norestart"] },
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
}

// ---------- 路径 ----------

/// 正式版把整个 payload 以 zip 内嵌进 exe，这样一个文件就能发出去。
/// 开发版不嵌，直接读项目里那份活的 payload，改完立刻生效、也不用每次重编 50MB 数据。
#[cfg(not(debug_assertions))]
const PAYLOAD_ZIP: &[u8] = include_bytes!("../payload.zip");

/// 同时打到控制台（开发时看）和 %TEMP%\Click.log（用户出问题直接把文件发过来）
fn log(line: &str) {
    // 别用 println!：正式版没有控制台，stdout 是空句柄，写失败会 panic，
    // 而 release 配了 panic = "abort"，一个日志就能把整个程序带走。
    let _ = writeln!(std::io::stdout(), "[click] {line}");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("Click.log"))
    {
        let _ = writeln!(f, "[click] {line}");
    }
}

/// 把内嵌的 payload 解到 %ProgramData%\Click\payload\<版本>\，同一版本只解一次。
/// 放 ProgramData 而不是临时目录：它是持久的，重复启动不用再解一遍。
#[cfg(not(debug_assertions))]
fn ensure_payload() -> Option<PathBuf> {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
        .join("Click")
        .join("payload");
    let dir = base.join(env!("CARGO_PKG_VERSION"));
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

fn run_one(root: &Path, c: &Comp) -> Result<(), String> {
    let dir = root.join(c.dir);
    match c.kind {
        Kind::Dll => {
            let dest = target_dir(c.arch);
            for f in c.files {
                let from = dir.join(f);
                if !from.is_file() {
                    return Err(format!("缺文件 {f}"));
                }
                let to = dest.join(f);
                // 散装 dll 只补缺：正在被程序加载的系统 dll 覆盖会直接失败（共享冲突），
                // 而且这一类"文件在就能用"，存在即正确。
                if to.is_file() {
                    continue;
                }
                std::fs::copy(&from, &to).map_err(|e| format!("{f} 复制失败：{e}"))?;
            }
            Ok(())
        }
        Kind::Msi => {
            // MSI 不能直接从非 ASCII 路径装：Windows Installer 服务是独立进程，
            // 路径在里面要过一遍 ANSI，中文路径到那儿就废了，msiexec 只会回 1619（包打不开）。
            // 所以先把包和它的 cab / msp 一起拷到一个纯 ASCII 的临时目录，再从那里装。
            // 放 ProgramData 而不是 Windows\Temp：同样是纯 ASCII 路径，
            // 但普通权限读得到，出问题用户/客服不用提权就能把日志发出来
            let stage = std::env::var_os("ProgramData")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
                .join("Click")
                .join(c.id);
            std::fs::create_dir_all(&stage).map_err(|e| format!("建临时目录失败：{e}"))?;
            for e in std::fs::read_dir(&dir).map_err(|e| format!("读组件目录失败：{e}"))?.flatten() {
                let p = e.path();
                let ext = p
                    .extension()
                    .map(|x| x.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if matches!(ext.as_str(), "msi" | "cab" | "msp") {
                    let to = stage.join(p.file_name().unwrap_or_default());
                    std::fs::copy(&p, &to).map_err(|e| format!("复制 {} 失败：{e}", p.display()))?;
                }
            }
            let msi = stage.join(c.files[0]);
            let msi_log = stage.join("install.log");
            let st = Command::new("msiexec.exe")
                .arg("/i")
                .arg(&msi)
                .args(["/qn", "/norestart", "/l*v"])
                .arg(&msi_log)
                .status()
                .map_err(|e| format!("启动 msiexec 失败：{e}"))?;
            if !code_ok(st.code()) {
                log(&format!("msiexec 日志：{}", msi_log.display()));
                return Err(format!("msiexec 返回 {}", st.code().unwrap_or(-1)));
            }
            // 同目录里如果有 .msp 补丁就接着打上（2010 SP1 的 KB2565063 就是这样进来的）
            for e in std::fs::read_dir(&stage).map_err(|e| format!("读临时目录失败：{e}"))?.flatten() {
                let p = e.path();
                let is_msp = p.extension().map(|x| x.eq_ignore_ascii_case("msp")).unwrap_or(false);
                if !is_msp {
                    continue;
                }
                log(&format!("打补丁 {}", p.display()));
                let st = Command::new("msiexec.exe")
                    .arg("/p")
                    .arg(&p)
                    .args(["/qn", "/norestart"])
                    .status()
                    .map_err(|e| format!("启动 msiexec 失败：{e}"))?;
                if !code_ok(st.code()) {
                    return Err(format!("补丁返回 {}", st.code().unwrap_or(-1)));
                }
            }
            Ok(())
        }
        Kind::Exe => {
            let file = dir.join(c.files[0]);
            let st = Command::new(&file)
                .args(c.args)
                .status()
                .map_err(|e| format!("启动安装器失败：{e}"))?;
            if code_ok(st.code()) {
                Ok(())
            } else {
                Err(format!("安装器返回 {}", st.code().unwrap_or(-1)))
            }
        }
    }
}

fn run_install(app: AppHandle, rows: Vec<String>) {
    BUSY.store(true, Ordering::SeqCst);
    run_install_inner(&app, rows);
    BUSY.store(false, Ordering::SeqCst);
}

fn run_install_inner(app: &AppHandle, rows: Vec<String>) {
    let Some(root) = payload_root(app) else {
        let _ = app.emit(
            "install:done",
            Done { ok: false, failed: vec!["找不到 payload 目录".into()] },
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

    let _ = app.emit("install:done", Done { ok: failed.is_empty(), failed });
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

    unsafe {
        let r = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        // <= 32 是失败码
        r as isize > 32
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
    std::thread::spawn(move || run_install(app, rows));
}

#[tauri::command]
fn is_elevated() -> bool {
    elevated()
}

/// 兜底退出：正常关窗口在极端情况下可能不生效（窗口失焦、事件没送到），
/// 那就直接结束自己。安装器是独立进程，不受影响。
#[tauri::command]
fn quit() {
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
            open_url,
            quit
        ])
        .run(tauri::generate_context!())
        .expect("启动失败");
}
