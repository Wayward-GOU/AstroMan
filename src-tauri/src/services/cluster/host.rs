//! 集群会话与宿主环境的解耦层
//!
//! 生产环境用 `AppHandle` 发事件、读 `AppState`；验收 harness 用 Tauri mock
//! 运行时构造独立的 AppHandle，模拟两台机器各自的状态与任务注册表。
//! 会话只依赖这里的 trait，不直接绑定具体运行时。

use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::client::EVENT_SLAVE;
use super::protocol::{ClusterMasterSnapshot, ClusterSlaveSnapshot};
use super::server::{ClusterSampleEvent, EVENT_MASTER, EVENT_SAMPLE};
use crate::state::AppState;

/// 会话宿主：发事件 + 读取本机状态
///
/// 方法保持非泛型，才能作为 `dyn` 对象跨线程共享。
pub trait ClusterApp: Send + Sync + 'static {
    fn emit_master(&self, snapshot: ClusterMasterSnapshot);
    fn emit_sample(&self, event: ClusterSampleEvent);
    fn emit_slave(&self, snapshot: ClusterSlaveSnapshot);
    fn try_state(&self) -> Option<tauri::State<'_, AppState>>;
}

impl<R: Runtime> ClusterApp for AppHandle<R> {
    fn emit_master(&self, snapshot: ClusterMasterSnapshot) {
        let _ = self.emit(EVENT_MASTER, snapshot);
    }

    fn emit_sample(&self, event: ClusterSampleEvent) {
        let _ = self.emit(EVENT_SAMPLE, event);
    }

    fn emit_slave(&self, snapshot: ClusterSlaveSnapshot) {
        let _ = self.emit(EVENT_SLAVE, snapshot);
    }

    fn try_state(&self) -> Option<tauri::State<'_, AppState>> {
        <Self as Manager<R>>::try_state::<AppState>(self)
    }
}
