use core::ptr;

use alloc::vec::Vec;
use syscall::{SystemCall, return_vals::Errno, syscall};

pub struct ChildProcess {
    app_id: usize,
    main_thread_id: usize,
}

impl ChildProcess {
    pub fn app_id(&self) -> usize {
        self.app_id
    }

    pub fn main_thread_id(&self) -> usize {
        self.main_thread_id
    }

    pub fn join(&self) -> Result<usize, Errno> {
        syscall(SystemCall::ThreadJoin, &[self.main_thread_id])
    }

    pub fn terminate(&self) {
        let _ = syscall(SystemCall::ThreadKill, &[self.main_thread_id]);
    }
}

/// Start a standalone graphical application and retain both identifiers.
pub fn run_gapp(name: &str, args: Vec<&str>) -> Option<ChildProcess> {
    let mut app_id = 0usize;
    let result = syscall(
        SystemCall::ProcessExecuteBinary,
        &[
            name.as_bytes().as_ptr() as usize,
            name.len(),
            ptr::from_ref(&args) as usize,
            ptr::from_mut(&mut app_id) as usize,
        ],
    );

    result.ok().map(|main_thread_id| ChildProcess {
        app_id,
        main_thread_id,
    })
}
