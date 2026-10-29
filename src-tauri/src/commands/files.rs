use tauri::command;

/// 读取文本文件（用于 Runner 数据文件等场景），最大 20 MB
#[command]
pub fn file_read_text(path: String) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("文件路径为空".to_string());
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("读取文件失败（{path}）：{e}"))?;
    if meta.len() > 20 * 1024 * 1024 {
        return Err("文件超过 20 MB，请拆分后再导入".to_string());
    }
    std::fs::read_to_string(path).map_err(|e| format!("读取文件失败（{path}）：{e}"))
}

/// 判断文件是否已存在：导出前用于提示覆盖（T-17）
#[command]
pub fn file_exists(path: String) -> bool {
    let path = path.trim();
    !path.is_empty() && std::path::Path::new(path).exists()
}

/// 写入文本文件（用于导出结果、保存响应体等场景）
#[command]
pub fn file_write_text(path: String, content: String) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("文件路径为空".to_string());
    }
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败（{}）：{e}", parent.display()))?;
        }
    }
    std::fs::write(path, content).map_err(|e| format!("写入文件失败（{path}）：{e}"))
}
