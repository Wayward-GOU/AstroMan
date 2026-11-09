//! AI 写操作权限策略（AI-01）。
//!
//! 模型能直接调用 `send_request` 对真实环境发 POST / DELETE，这是当前最危险的缺口。
//! 这里把「放行 / 二次确认 / 拒绝」做成纯函数，命令层与前端共用同一套判定：
//!
//! 判定顺序：方法是否只读 → 环境是否生产 → 权限模式 → 是否二次确认。
//! 生产环境无论哪种模式都强制确认，避免 `trusted` 被误开在生产环境。

use serde::Serialize;

/// 只读方法：不改变服务端状态
pub fn is_read_only_method(method: &str) -> bool {
    matches!(
        method.trim().to_ascii_uppercase().as_str(),
        "GET" | "HEAD" | "OPTIONS"
    )
}

/// 权限模式。`ConfirmWrite` 是默认值，空串与未知值都收敛到这里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// 只读：拦截全部写工具与所有非只读请求
    Readonly,
    /// 默认：写工具确认；`send_request` 仅只读方法放行
    ConfirmWrite,
    /// 受信：写工具仍确认；`send_request` 全部放行（仅限自建测试环境）
    Trusted,
}

/// 设置值归一化：只接受 readonly / confirm_write / trusted，其余一律 confirm_write。
pub fn normalize_mode(mode: &str) -> &'static str {
    match mode.trim().to_ascii_lowercase().as_str() {
        "readonly" | "read_only" | "read-only" => "readonly",
        "trusted" => "trusted",
        _ => "confirm_write",
    }
}

pub fn parse(mode: &str) -> PermissionMode {
    match normalize_mode(mode) {
        "readonly" => PermissionMode::Readonly,
        "trusted" => PermissionMode::Trusted,
        _ => PermissionMode::ConfirmWrite,
    }
}

/// 策略判定结果，前端据此弹确认框或直接拒绝。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// 直接执行
    Allow,
    /// 需要用户二次确认
    Confirm,
    /// 拒绝执行，原因回传给模型
    Deny,
}

impl Decision {
    pub fn needs_confirm(self) -> bool {
        matches!(self, Self::Confirm)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Confirm => "confirm",
            Self::Deny => "deny",
        }
    }
}

/// `send_request` 的判定。
///
/// 只读方法先过方法关；生产环境无论模式都要求确认；
/// 非只读方法再按模式决定放行（trusted）或确认（confirm_write）。
pub fn decide_send(mode: &str, method: &str, is_prod: bool) -> Decision {
    let read_only = is_read_only_method(method);
    let parsed = parse(mode);

    // 只读模式先拦非只读请求，生产环境也不例外
    if parsed == PermissionMode::Readonly && !read_only {
        return Decision::Deny;
    }
    if is_prod {
        return Decision::Confirm;
    }
    if read_only {
        return Decision::Allow;
    }
    match parsed {
        PermissionMode::Trusted => Decision::Allow,
        PermissionMode::ConfirmWrite => Decision::Confirm,
        // 上面已拦截，这里只为穷尽枚举
        PermissionMode::Readonly => Decision::Deny,
    }
}

/// 写工具（改数据库 / 改文件 / 变环境）的判定：readonly 一律拒绝，其余模式都要确认。
/// 当前由前端确认层调用；后端保留同一判定给后续命令级拦截复用。
#[allow(dead_code)]
pub fn decide_write(mode: &str) -> Decision {
    match parse(mode) {
        PermissionMode::Readonly => Decision::Deny,
        PermissionMode::ConfirmWrite | PermissionMode::Trusted => Decision::Confirm,
    }
}

/// 拒绝时回传给模型的原因，避免模型反复重试同一个被拦操作。
pub fn deny_reason(mode: &str, method: &str) -> String {
    match parse(mode) {
        PermissionMode::Readonly => format!(
            "当前为只读权限模式，已拦截 {} 请求。请只做查询；需要写操作时由用户切换到 confirm_write 或 trusted。",
            method.trim().to_ascii_uppercase()
        ),
        _ => format!(
            "当前权限模式不允许执行 {} 请求。",
            method.trim().to_ascii_uppercase()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_normalization_defaults_to_confirm_write() {
        assert_eq!(normalize_mode("readonly"), "readonly");
        assert_eq!(normalize_mode("Read-Only"), "readonly");
        assert_eq!(normalize_mode("trusted"), "trusted");
        assert_eq!(normalize_mode("confirm_write"), "confirm_write");
        assert_eq!(normalize_mode(""), "confirm_write");
        assert_eq!(normalize_mode("yolo"), "confirm_write");
    }

    #[test]
    fn readonly_blocks_non_readonly_requests() {
        assert_eq!(decide_send("readonly", "GET", false), Decision::Allow);
        assert_eq!(decide_send("readonly", "HEAD", false), Decision::Allow);
        assert_eq!(decide_send("readonly", "POST", false), Decision::Deny);
        assert_eq!(decide_send("readonly", "DELETE", true), Decision::Deny);
    }

    #[test]
    fn confirm_write_allows_readonly_and_confirms_writes() {
        assert_eq!(decide_send("confirm_write", "get", false), Decision::Allow);
        assert_eq!(
            decide_send("confirm_write", "POST", false),
            Decision::Confirm
        );
        assert_eq!(decide_send("", "DELETE", false), Decision::Confirm);
    }

    #[test]
    fn trusted_only_relaxes_custom_environments() {
        assert_eq!(decide_send("trusted", "POST", false), Decision::Allow);
        assert_eq!(decide_send("trusted", "GET", false), Decision::Allow);
    }

    #[test]
    fn prod_always_confirms_even_readonly_methods() {
        assert_eq!(decide_send("trusted", "GET", true), Decision::Confirm);
        assert_eq!(
            decide_send("confirm_write", "POST", true),
            Decision::Confirm
        );
        // 只读模式对非只读请求仍然是拒绝，不是确认
        assert_eq!(decide_send("readonly", "POST", true), Decision::Deny);
    }

    #[test]
    fn write_tools_always_need_confirmation_except_readonly() {
        assert_eq!(decide_write("readonly"), Decision::Deny);
        assert_eq!(decide_write("confirm_write"), Decision::Confirm);
        assert_eq!(decide_write("trusted"), Decision::Confirm);
    }
}
