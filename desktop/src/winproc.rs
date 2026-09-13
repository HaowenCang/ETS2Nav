//! Windows 进程/作业对象的最小封装（P4R Batch 6A §8）。
//!
//! ## 为什么需要 Job Object
//!
//! 「Desktop 退出后不得残留 nav-core-cli」有两种强度不同的含义：
//!
//! ```text
//! 弱：窗口关闭 / 正常退出路径上杀掉子进程   —— 由 Drop 守卫即可达成
//! 强：Desktop 自身被强杀（任务管理器、崩溃）也不得留下子进程
//! ```
//!
//! 弱形态无法覆盖后者：Windows 不会因为父进程死亡而回收子进程，于是会留下一个
//! 仍在监听端口的 nav-server——这恰好是本轮要消除的 zombie 形态，只不过 zombie
//! 从「服务线程死了、进程还在」换成了「界面没了、服务还在」。
//!
//! Job Object 的 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 把这件事交给内核：作业句柄
//! 关闭（含进程被强杀导致的句柄回收）时，内核终止作业内的全部进程。因此它不是
//! 「更好的清理代码」，而是把清理从用户态职责变成内核保证。
//!
//! 边界：作业只覆盖由本进程显式加入的子进程；若未来 sidecar 再派生子进程，
//! 必须让它们也留在同一作业内（当前 sidecar 不派生子进程）。

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
        PROCESS_TERMINATE,
    };

    /// 内核句柄的所有权包装：Drop 即关闭。
    ///
    /// 句柄以 `isize` 存放而不是 `HANDLE`（`*mut c_void`）：Windows 句柄是进程级的
    /// 内核对象引用，TerminateProcess / AssignProcessToJobObject / CloseHandle 都允许
    /// 从任意线程调用，因此把裸指针存进结构体会让 `ProcessRef` 因裸指针而自动失去
    /// `Send`/`Sync`——那是 Rust 的类型规则，不是 Windows 的约束。存成整数后由本模块
    /// 在调用点转回 `HANDLE`，类型系统即与真实约束一致，无需 `unsafe impl Send`。
    struct Owned(isize);

    impl Owned {
        fn handle(&self) -> HANDLE {
            self.0 as HANDLE
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle());
            }
        }
    }

    /// 已打开的进程句柄（可终止）。句柄为空表示打开失败。
    pub struct ProcessRef {
        handle: Owned,
    }

    impl ProcessRef {
        pub fn open(pid: u32) -> Option<ProcessRef> {
            // PROCESS_SET_QUOTA 是 AssignProcessToJobObject 的前置权限要求。
            let access = PROCESS_TERMINATE | PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION;
            let h = unsafe { OpenProcess(access, 0, pid) };
            if h.is_null() {
                return None;
            }
            Some(ProcessRef {
                handle: Owned(h as isize),
            })
        }

        /// 终止该进程。返回是否成功（失败不 panic：调用方仍会走后续清理）。
        pub fn terminate(&self, exit_code: u32) -> bool {
            unsafe { TerminateProcess(self.handle.handle(), exit_code) != 0 }
        }

        pub fn raw(&self) -> HANDLE {
            self.handle.handle()
        }
    }

    /// 存活期与 Desktop 同寿的作业对象。
    pub struct Job {
        handle: Owned,
    }

    impl Job {
        /// 建立作业并设置「句柄关闭即终止成员进程」。失败返回 None（调用方降级为
        /// 仅 Drop 守卫，并必须如实报告该降级）。
        pub fn create_kill_on_close() -> Option<Job> {
            let h = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if h.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                unsafe { CloseHandle(h) };
                return None;
            }
            Some(Job {
                handle: Owned(h as isize),
            })
        }

        /// 把进程加入作业。返回 `Err(GetLastError)` 便于诊断具体原因
        /// （例如外层作业已禁止嵌套）。
        pub fn assign(&self, proc: &ProcessRef) -> Result<(), u32> {
            let ok = unsafe { AssignProcessToJobObject(self.handle.handle(), proc.raw()) };
            if ok == 0 {
                return Err(unsafe { GetLastError() });
            }
            Ok(())
        }
    }
}

#[cfg(not(windows))]
mod imp {
    /// 非 Windows 平台不存在作业对象语义；本仓库的产品是 Windows-only，
    /// 这里只保证代码可编译，不声称任何生命周期保证。
    pub struct ProcessRef;
    impl ProcessRef {
        pub fn open(_pid: u32) -> Option<ProcessRef> {
            None
        }
        pub fn terminate(&self, _exit_code: u32) -> bool {
            false
        }
    }
    pub struct Job;
    impl Job {
        pub fn create_kill_on_close() -> Option<Job> {
            None
        }
        pub fn assign(&self, _proc: &ProcessRef) -> Result<(), u32> {
            Err(0)
        }
    }
}

pub use imp::{Job, ProcessRef};
