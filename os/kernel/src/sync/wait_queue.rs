/* ╔═════════════════════════════════════════════════════════════════════════╗
   ║ Module: wait_queue                                                      ║
   ╟─────────────────────────────────────────────────────────────────────────╢
   ║ Wait queues for blocking i/o.                                           ║
   ║                                                                         ║
   ║ Public functions:                                                       ║
   ║   - wait:       Blocks calling thread if the given predicate is true.   ║
   ║   - notify_one: Deblocks one waiting thread (if any).                   ║
   ║   - notify_all: Deblocks all waiting threads (if any).                  ║
   ╟─────────────────────────────────────────────────────────────────────────╢
   ║ Author: Michael Schoettner, Univ. Duesseldorf, 16.02.2026               ║
   ╚═════════════════════════════════════════════════════════════════════════╝
*/

use alloc::collections::VecDeque;
use crate::scheduler;
use crate::sync::irqsave_spinlock::IrqSaveSpinlock;

pub struct WaitQueue {
    queue: IrqSaveSpinlock<VecDeque<(usize, usize)>>,
}

impl WaitQueue {
    pub fn new() -> WaitQueue {
        WaitQueue {
            queue: IrqSaveSpinlock::new(VecDeque::<(usize, usize)>::new()),
        }
    }

    /// Block until `pred()` becomes true.
    pub fn wait<F>(&self, mut pred: F, _message: &str)
    where
        F: FnMut() -> bool,
    {
        //        info!("WaitQueue::wait");
        let (pid, tid) = scheduler().current_ids();

        loop {
            if pred() {
                return;
            }

            {
                let mut guard = self.queue.lock();

                // re-check under lock
                if pred() {
                    return;
                }

                guard.push_back((pid, tid));

                // park after we are visible to notifiers
                scheduler().park_current();
            }

            // `yield_now()` forces the current state back to `Ready`, which
            // cancels the `Parking` state set above and turns every blocking
            // FIFO wait into a busy scheduler loop. Use the normal scheduler
            // switch path: it moves a Parking thread to the blocked list and
            // only `notify_one/all` makes it runnable again.
            scheduler().switch_thread_no_interrupt();
        }
    }

    /// Wake up exactly one waiter (if any). Returns true if someone was woken up.
    pub fn notify_one(&self) -> bool {
        //  info!("WaitQueue::notify_one");

        loop {
            let waiter = self.queue.lock().pop_front();
            let Some((pid, tid)) = waiter else { return false; };

            // Never hold the wait-queue spinlock while entering the scheduler;
            // scheduler paths may in turn wake or switch another waiter.
            if scheduler().unblock(pid, tid) {
                return true;
            }
        }
    }

    /// Wake up all waiters currently queued.
    /// Returns the number of threads actually unblocked (stale entries are ignored).
    pub fn notify_all(&self) -> usize {
        let mut woke = 0;

        loop {
            let waiter = self.queue.lock().pop_front();
            let Some((pid, tid)) = waiter else { break; };
            if scheduler().unblock(pid, tid) {
                woke += 1;
            }
        }

        woke
    }
}
