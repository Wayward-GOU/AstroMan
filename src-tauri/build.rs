fn main() {
    // tauri-build 默认只在桌面端二进制里嵌入 Common-Controls v6 清单，`cargo test`
    // 的测试二进制拿不到；而依赖链（tauri-runtime-wry 的对话框代码）引用了仅 v6
    // 才导出的 TaskDialogIndirect，缺清单时测试进程会加载 v5.82 并直接以
    // 0xc0000139（STATUS_ENTRYPOINT_NOT_FOUND）退出。
    //
    // 这里关掉 tauri-build 自带的清单，改为由本脚本把同一份清单链到所有目标
    // （含 lib 单测），避免 MANIFEST 资源重复（CVT1100）。
    let attrs = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    if let Err(error) = tauri_build::try_build(attrs) {
        println!("{error:#}");
        std::process::exit(1);
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_common_controls_v6();
    }
}

fn embed_common_controls_v6() {
    use std::path::PathBuf;

    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let manifest = root.join("windows-test.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc = out_dir.join("common-controls-v6.rc");
    // 1 = 资源 ID，24 = RT_MANIFEST；路径用正斜杠，避免 rc.exe 把 `\A` 当转义
    let manifest_path = manifest.display().to_string().replace('\\', "/");
    let content = format!("1 24 \"{manifest_path}\"\n");
    if std::fs::write(&rc, content).is_err() {
        return;
    }

    embed_resource::compile_for_everything(&rc, embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
