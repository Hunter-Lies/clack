// 组件表 vs payload 的一致性自测：表里写的每个文件名，payload 目录里都得真有。
//
// 这条测试是为一个真事故加的：VC++ 2012/2013 那四条写成了 vc_redist.x86.exe
// （那是 2015-2022 那份的名字），包里的文件实际叫 vc_redist2012_x86.exe。
// 用户看到的是"启动安装器失败：系统找不到指定的文件 (os error 2)"，而那两代运行库
// 从发布起就没装上过。跑一遍这个测试就不会再犯。
//
// 跑法：cargo test --manifest-path ui\src-tauri\Cargo.toml -- --nocapture
#[cfg(test)]
mod tests {
    use crate::COMPS;
    use std::path::Path;

    #[test]
    fn every_component_file_exists_in_payload() {
        let payload = Path::new(r"..\..\_work\payload");
        if !payload.is_dir() {
            println!("跳过：找不到 payload 目录 {payload:?}（先跑 tools\\stage_payload.ps1）");
            return;
        }
        let mut bad = 0;
        for c in COMPS {
            let f = payload.join(c.dir).join(c.files[0]);
            if !f.is_file() {
                println!(
                    "缺文件：{}\\{} 里的 {} 不存在（表里这条是 {}）",
                    c.dir,
                    c.id,
                    c.files[0],
                    c.name
                );
                bad += 1;
            }
        }
        assert_eq!(bad, 0, "{bad} 个组件的文件名和 payload 对不上");
    }

    /// 1612 补救要靠"读出来的安装源路径"去把包放回原处。
    /// 这个 API 在真机上返回的是纯路径（`C:\dir\`），注册表里存的是带类型索引的形式
    /// （`n;4;C:\dir\`），UNC 和 http 都不能当本地目录去写。
    #[test]
    fn source_record_parsing() {
        use crate::path_of_source_record;
        use std::path::PathBuf;

        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let local = here.to_string_lossy().trim_end_matches('\\').to_string() + "\\";
        // 纯路径（MsiSourceListGetInfo 就是这么给的）
        assert_eq!(path_of_source_record(&local), Some(PathBuf::from(&local)));
        // 带类型和索引（注册表里的形式）
        assert_eq!(
            path_of_source_record(&format!("n;4;{local}")),
            Some(PathBuf::from(&local))
        );
        // 盘不存在的、UNC、http 一律不碰
        assert_eq!(path_of_source_record("n;1;Q:\\nope\\"), None);
        assert_eq!(path_of_source_record("n;1;\\\\server\\share\\"), None);
        assert_eq!(path_of_source_record("http://example.com/x"), None);
    }

    /// 「立刻重启」的前提是能把 SeShutdownPrivilege 打开。跑这条只动自己进程的令牌，
    /// **不会**真的重启机器；但普通用户/受限账户的令牌里可能没有这个权限，
    /// 所以默认忽略，需要时手动跑：`cargo test -- --ignored`。
    #[test]
    #[ignore = "与账户权限有关，按需手动跑"]
    fn shutdown_privilege_can_be_enabled() {
        assert!(
            crate::enable_shutdown_privilege(),
            "打开 SeShutdownPrivilege 失败，立刻重启那一步会失败"
        );
    }

    /// 读"待重启文件操作"这一步：MSI 回 3010 时要靠它判断是我们造成的还是机器本来就有的。
    /// 不断言数量（每台机器不一样），断言读出来的项都是像样的路径。
    #[test]
    fn pending_file_ops_reads() {
        let ops = crate::pending_file_ops();
        println!("待重启文件操作 {} 项：{:?}", ops.len(), ops);
        for op in ops {
            assert!(op.len() > 4, "读出来的项不像路径：{op}");
        }
    }
}
