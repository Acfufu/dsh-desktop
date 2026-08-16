use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    Restart,
    Stop,
}

#[derive(Debug, Clone)]
pub struct RestartCounter {
    pub consecutive_failures: u32,
    pub current_delay: Duration,
    pub base: Duration,
}

impl RestartCounter {
    pub fn new() -> Self {
        Self { consecutive_failures: 0, current_delay: Duration::from_secs(1), base: Duration::from_secs(1) }
    }

    /// 规则（spec §6）：存活<30s → 计数+1、延迟 = min(30s, 1s × 2^(n-1))；
    /// 存活≥30s → 重置计数与延迟。计数达 5 → Stop。
    pub fn on_exit(&mut self, alive_secs: u64) -> RestartDecision {
        if alive_secs >= 30 {
            self.consecutive_failures = 0;
            self.current_delay = self.base;
            return RestartDecision::Restart;
        }
        self.consecutive_failures += 1;
        if self.consecutive_failures >= 5 {
            return RestartDecision::Stop;
        }
        let n = self.consecutive_failures as u32; // 1 基
        let delay = self.base.saturating_mul(1u32 << (n.saturating_sub(1))).min(Duration::from_secs(30));
        self.current_delay = delay;
        RestartDecision::Restart
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState2 {
    Stopped,
    FirstStarting,
    Running,
    Restarting,
    RestartStopped,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    Start,            // App 启动 → spawn
    SocketReady,      // socket 可达 → running
    UnexpectedExit,   // 任意非 App 驱动退出（含 exit 0）
    BackoffElapsed,   // 退避到期 → respawn
    FirstStartFailed, // 首启 pre-ready 失败（从未达过 socket-ready）
    RetryFromDialog,  // 对话框重试
    RetryFromTray,    // restart-stopped 托盘重试（重置计数）
    UserQuit,         // Cmd+Q / 托盘退出
}

// R4 修正：ever_ready 由调用方经事件选择（FirstStartFailed vs UnexpectedExit）表达；
// alive_secs 传入 on_exit（≥30s 重置规则可经状态机路径表达，不再硬编码 0）
pub fn transition(state: AppState2, event: AppEvent, counter: &mut RestartCounter, alive_secs: u64) -> AppState2 {
    match (state, event) {
        (AppState2::Stopped, AppEvent::Start) => AppState2::FirstStarting,
        (AppState2::FirstStarting, AppEvent::SocketReady) => AppState2::Running,
        (AppState2::FirstStarting, AppEvent::FirstStartFailed) => AppState2::Stopped, // 首启对话框（UI 层，不计数）
        (AppState2::Running, AppEvent::UnexpectedExit) => match counter.on_exit(alive_secs) {
            RestartDecision::Restart => AppState2::Restarting,
            RestartDecision::Stop => AppState2::RestartStopped,
        },
        (AppState2::Restarting, AppEvent::BackoffElapsed) => AppState2::FirstStarting,
        (AppState2::RestartStopped, AppEvent::RetryFromTray) => {
            counter.consecutive_failures = 0;
            counter.current_delay = counter.base;
            AppState2::FirstStarting
        }
        (_, AppEvent::UserQuit) => AppState2::Stopping,
        _ => state, // 未定义迁移保持原状态（幂等）
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_failure_delay_1s() {
        let mut c = RestartCounter::new();
        assert_eq!(c.on_exit(5), RestartDecision::Restart);
        assert_eq!(c.current_delay, Duration::from_secs(1));
    }

    #[test]
    fn fourth_failure_delay_8s() {
        let mut c = RestartCounter::new();
        c.on_exit(5); c.on_exit(5); c.on_exit(5);
        assert_eq!(c.on_exit(5), RestartDecision::Restart);
        assert_eq!(c.current_delay, Duration::from_secs(8));
    }

    #[test]
    fn fifth_failure_stops() {
        let mut c = RestartCounter::new();
        for _ in 0..4 { c.on_exit(5); }
        assert_eq!(c.on_exit(5), RestartDecision::Stop);
    }

    #[test]
    fn long_lived_resets_counter() {
        let mut c = RestartCounter::new();
        c.on_exit(5); c.on_exit(5); c.on_exit(5);
        assert_eq!(c.on_exit(30), RestartDecision::Restart); // ≥30s 重置
        assert_eq!(c.consecutive_failures, 0);
        assert_eq!(c.current_delay, Duration::from_secs(1));
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn start_to_running() {
        let mut c = RestartCounter::new();
        assert_eq!(transition(AppState2::Stopped, AppEvent::Start, &mut c, 0), AppState2::FirstStarting);
        assert_eq!(transition(AppState2::FirstStarting, AppEvent::SocketReady, &mut c, 0), AppState2::Running);
    }

    #[test]
    fn unexpected_exit_backoff_then_stop() {
        let mut c = RestartCounter::new();
        let s = transition(AppState2::Running, AppEvent::UnexpectedExit, &mut c, 5);
        assert_eq!(s, AppState2::Restarting);
        assert_eq!(transition(AppState2::Restarting, AppEvent::BackoffElapsed, &mut c, 0), AppState2::FirstStarting);
    }

    #[test]
    fn long_lived_crash_resets_via_transition() {
        let mut c = RestartCounter::new();
        c.on_exit(5); c.on_exit(5); // 2 次短命失败
        let s = transition(AppState2::Running, AppEvent::UnexpectedExit, &mut c, 60); // 存活≥30s
        assert_eq!(s, AppState2::Restarting);
        assert_eq!(c.consecutive_failures, 0); // R4 修正：≥30s 重置经状态机路径验证
    }

    #[test]
    fn five_failures_stops() {
        let mut c = RestartCounter::new();
        for _ in 0..5 {
            let s = transition(AppState2::Running, AppEvent::UnexpectedExit, &mut c, 0);
            if s == AppState2::Restarting {
                transition(AppState2::Restarting, AppEvent::BackoffElapsed, &mut c, 0);
            }
        }
        assert_eq!(transition(AppState2::Running, AppEvent::UnexpectedExit, &mut c, 0), AppState2::RestartStopped);
    }

    #[test]
    fn quit_anywhere_stops() {
        let mut c = RestartCounter::new();
        for s in [AppState2::Running, AppState2::Restarting, AppState2::FirstStarting] {
            assert_eq!(transition(s, AppEvent::UserQuit, &mut c, 0), AppState2::Stopping);
        }
    }
}
