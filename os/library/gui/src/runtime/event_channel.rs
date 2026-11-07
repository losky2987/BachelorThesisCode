//! Reliable EventPipe transport and per-process GUI channel state.
//!
//! Callers should use the exports from `gui::runtime` rather than depending
//! on the private transport implementation module.

mod event_pipe_runtime {
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use alloc::vec::Vec;
    use concurrent::thread;
    use concurrent::thread::Thread;
    use spin::Mutex;

    use crate::{
        bus::event_pipe::{EventPipe, EventRole},
        event::{
            events::GUIEvent,
            protocol::{GUIEventProtocol, from_json},
        },
        runtime::event_mailbox::{InMemoryMailbox, MailboxPostError, MailboxPostOutcome, MailboxStats},
    };

    /// Non-zero only inside a standalone graphical application process.
    static PROCESS_APP_ID: AtomicUsize = AtomicUsize::new(0);

    /// Non-zero only inside a standalone graphical application process.
    static PROCESS_WINDOW_ID: AtomicUsize = AtomicUsize::new(0);

    /// Routes handed to newly-created Window Manager FIFO listener threads.
    static PENDING_WM_LISTENERS: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

    /// IDs handed to WM -> APP FIFO writer threads. FIFO open/write can block, so
    /// it must never execute on the Window Manager compositor/input thread.
    static PENDING_WM_WRITERS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

    /// Process main threads waiting to be joined by a monitor worker.
    static PENDING_PROCESS_MONITORS: Mutex<Vec<(usize, usize, usize)>> = Mutex::new(Vec::new());

    struct ProcessChannelThreads {
        app_id: usize,
        listener: Thread,
        writer: Thread,
        monitor: Option<Thread>,
    }

    static PROCESS_CHANNEL_THREADS: Mutex<Vec<ProcessChannelThreads>> = Mutex::new(Vec::new());

    const MAX_PENDING_APPS: usize = 128;
    const PENDING_BIT: usize = 1;
    const IDLE_FAST_YIELDS: u8 = 8;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum PendingAppError {
        InvalidAppId,
        Capacity,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum EventChannelPostError {
        Pending(PendingAppError),
        Mailbox(MailboxPostError),
    }

    impl EventChannelPostError {
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::Pending(PendingAppError::InvalidAppId) => "Invalid GUI event route",
                Self::Pending(PendingAppError::Capacity) => "GUI pending-route registry is full",
                Self::Mailbox(error) => error.as_str(),
            }
        }
    }

    impl From<PendingAppError> for EventChannelPostError {
        fn from(error: PendingAppError) -> Self {
            Self::Pending(error)
        }
    }

    impl From<MailboxPostError> for EventChannelPostError {
        fn from(error: MailboxPostError) -> Self {
            Self::Mailbox(error)
        }
    }

    struct IdleBackoff {
        fast_yields: u8,
    }

    impl IdleBackoff {
        const fn new() -> Self {
            Self { fast_yields: 0 }
        }

        fn reset(&mut self) {
            self.fast_yields = 0;
        }

        fn wait(&mut self) {
            if self.fast_yields < IDLE_FAST_YIELDS {
                self.fast_yields += 1;
                thread::switch();
            } else {
                thread::sleep(1);
            }
        }
    }

    /// Lock-free app-ID registry used only as a wake-up/index structure.
    ///
    /// A slot keeps its app ID for the process lifetime and toggles its low bit to
    /// publish work. This avoids using a spin lock between FIFO listener threads
    /// and the Window Manager's hot main loop.
    struct PendingAppIds {
        slots: [AtomicUsize; MAX_PENDING_APPS],
        has_pending: AtomicBool,
        registration_lock: Mutex<()>,
        rejected_registrations: AtomicUsize,
    }

    impl PendingAppIds {
        const fn new() -> Self {
            Self {
                slots: [const { AtomicUsize::new(0) }; MAX_PENDING_APPS],
                has_pending: AtomicBool::new(false),
                registration_lock: Mutex::new(()),
                rejected_registrations: AtomicUsize::new(0),
            }
        }

        fn register(&self, app_id: usize) -> Result<(), PendingAppError> {
            if app_id == 0 || app_id > (usize::MAX >> 1) {
                return Err(PendingAppError::InvalidAppId);
            }

            let encoded_id = app_id << 1;

            for slot in &self.slots {
                let state = slot.load(Ordering::Acquire);
                if state >> 1 == app_id {
                    return Ok(());
                }
            }

            let _guard = self.registration_lock.lock();
            for slot in &self.slots {
                let state = slot.load(Ordering::Acquire);
                if state >> 1 == app_id {
                    return Ok(());
                }
            }
            for slot in &self.slots {
                if slot.compare_exchange(0, encoded_id, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    return Ok(());
                }
            }

            self.rejected_registrations.fetch_add(1, Ordering::Relaxed);
            Err(PendingAppError::Capacity)
        }

        fn mark(&self, app_id: usize) -> Result<(), PendingAppError> {
            self.register(app_id)?;

            for slot in &self.slots {
                let state = slot.load(Ordering::Acquire);
                if state >> 1 == app_id {
                    slot.fetch_or(PENDING_BIT, Ordering::Release);
                    self.has_pending.store(true, Ordering::Release);
                    return Ok(());
                }
            }

            self.rejected_registrations.fetch_add(1, Ordering::Relaxed);
            Err(PendingAppError::Capacity)
        }

        fn clear(&self, app_id: usize) {
            for slot in &self.slots {
                let state = slot.load(Ordering::Acquire);
                if state >> 1 == app_id {
                    slot.fetch_and(!PENDING_BIT, Ordering::AcqRel);
                }
            }
        }

        fn remove(&self, app_id: usize) {
            for slot in &self.slots {
                loop {
                    let state = slot.load(Ordering::Acquire);
                    if state >> 1 != app_id {
                        break;
                    }
                    if slot.compare_exchange(state, 0, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                        break;
                    }
                }
            }
        }

        fn contains(&self, app_id: usize) -> bool {
            self.slots.iter().any(|slot| {
                let state = slot.load(Ordering::Acquire);
                state & PENDING_BIT != 0 && state >> 1 == app_id
            })
        }

        fn take_all(&self) -> Vec<usize> {
            // This is the Window Manager hot path. Without the gate below an empty
            // loop issued MAX_PENDING_APPS locked read/modify/write operations on
            // every compositor iteration, starving the rest of the system.
            if !self.has_pending.swap(false, Ordering::AcqRel) {
                return Vec::new();
            }

            let mut app_ids = Vec::new();
            for slot in &self.slots {
                // Clearing before the consumer reads the resource is intentional:
                // an update published afterwards remains pending for the next loop,
                // while an update published before this point is covered by the
                // read that follows.
                let state = slot.fetch_and(!PENDING_BIT, Ordering::AcqRel);
                if state & PENDING_BIT != 0 {
                    let app_id = state >> 1;
                    if !app_ids.contains(&app_id) {
                        app_ids.push(app_id);
                    }
                }
            }
            app_ids
        }

        fn rejected_registrations(&self) -> usize {
            self.rejected_registrations.load(Ordering::Relaxed)
        }
    }

    static EVENTPIPE_APP_PENDING: PendingAppIds = PendingAppIds::new();
    static EVENTPIPE_WM_PENDING: PendingAppIds = PendingAppIds::new();

    static MAILBOX_APP: Mutex<InMemoryMailbox> = Mutex::new(InMemoryMailbox::new());

    static MAILBOX_WM: Mutex<InMemoryMailbox> = Mutex::new(InMemoryMailbox::new());

    /// Release fixed-capacity pending slots after a graphical process exits.
    pub fn release_process_state(app_id: usize) {
        EVENTPIPE_APP_PENDING.remove(app_id);
        EVENTPIPE_WM_PENDING.remove(app_id);
        MAILBOX_APP.lock().release(app_id);
        MAILBOX_WM.lock().release(app_id);
        EventPipe::release_reliable_state(app_id);
        PENDING_PROCESS_MONITORS.lock().retain(|(pending_app_id, _, _)| *pending_app_id != app_id);

        let channel = {
            let mut channels = PROCESS_CHANNEL_THREADS.lock();
            channels.iter().position(|channel| channel.app_id == app_id).map(|index| channels.remove(index))
        };
        if let Some(channel) = channel {
            channel.listener.kill();
            channel.writer.kill();
            if let Some(monitor) = channel.monitor {
                monitor.kill();
            }
        }
    }

    fn publish_event(
        mailbox: &Mutex<InMemoryMailbox>, pending: &PendingAppIds, app_id: usize, protocol: GUIEventProtocol,
    ) -> Result<MailboxPostOutcome, EventChannelPostError> {
        // Reserve the pending slot before changing the mailbox. Once the
        // message is visible, mark() cannot silently fail because of capacity.
        pending.register(app_id)?;
        let outcome = mailbox.lock().post(app_id, protocol)?;
        if outcome.needs_notification() {
            pending.mark(app_id)?;
        }
        Ok(outcome)
    }

    pub(crate) fn app_events_pending(app_id: usize) -> bool {
        EVENTPIPE_APP_PENDING.contains(app_id)
    }

    pub fn take_pending_events_for_wm() -> Vec<usize> {
        EVENTPIPE_WM_PENDING.take_all()
    }

    pub(crate) fn post_event_to_wm(app_id: usize, protocol: GUIEventProtocol) -> Result<MailboxPostOutcome, EventChannelPostError> {
        // Keep FIFO rendezvous off the caller/render thread. The dedicated
        // writer retries this ordered mailbox until the control event arrives.
        publish_event(&MAILBOX_WM, &EVENTPIPE_WM_PENDING, app_id, protocol)
    }

    pub fn take_events_for_wm(app_id: usize) -> Vec<GUIEventProtocol> {
        MAILBOX_WM.lock().take(app_id)
    }

    pub(crate) fn send_event_to_app(app_id: usize, protocol: GUIEventProtocol) -> Result<MailboxPostOutcome, EventChannelPostError> {
        // A dedicated writer owns the blocking FIFO endpoint for this app.
        publish_event(&MAILBOX_APP, &EVENTPIPE_APP_PENDING, app_id, protocol)
    }

    pub(crate) fn take_events_for_app(app_id: usize) -> Vec<GUIEventProtocol> {
        EVENTPIPE_APP_PENDING.clear(app_id);
        MAILBOX_APP.lock().take(app_id)
    }

    pub(crate) fn process_app_id() -> Option<usize> {
        match PROCESS_APP_ID.load(Ordering::SeqCst) {
            0 => None,
            app_id => Some(app_id),
        }
    }

    fn process_window_id() -> Option<usize> {
        match PROCESS_WINDOW_ID.load(Ordering::SeqCst) {
            0 => None,
            window_id => Some(window_id),
        }
    }

    pub fn mailbox_stats_for_wm(app_id: usize) -> Option<MailboxStats> {
        MAILBOX_WM.lock().stats(app_id)
    }

    pub fn mailbox_stats_for_app(app_id: usize) -> Option<MailboxStats> {
        MAILBOX_APP.lock().stats(app_id)
    }

    pub fn rejected_pending_route_registrations() -> (usize, usize) {
        (EVENTPIPE_WM_PENDING.rejected_registrations(), EVENTPIPE_APP_PENDING.rejected_registrations())
    }

    fn send_reliable_event(pipe: &EventPipe, role: EventRole, protocol: &GUIEventProtocol) {
        let payload = protocol.to_json();
        loop {
            if pipe.send_reliable(role, &payload).is_ok() {
                return;
            }
            // The EventPipe retains its in-flight packet across a bounded send
            // attempt, so retrying here resumes the same sequence and payload.
            thread::sleep(1);
        }
    }

    /// Create the named pipes and a blocking APP -> WM reader before launching an
    /// application process. The reader is isolated in its own Window Manager
    /// thread so FIFO open/read never stalls composition.
    pub fn init_process_event_channel(app_id: usize, window_id: usize) -> bool {
        let pipe = EventPipe::new(app_id);
        if pipe.init().is_err() {
            return false;
        }
        if EVENTPIPE_WM_PENDING.register(app_id).is_err() || EVENTPIPE_APP_PENDING.register(app_id).is_err() {
            EVENTPIPE_WM_PENDING.remove(app_id);
            EVENTPIPE_APP_PENDING.remove(app_id);
            return false;
        }

        PENDING_WM_LISTENERS.lock().push((app_id, window_id));
        let Some(listener) = thread::create(window_manager_event_listener) else {
            PENDING_WM_LISTENERS.lock().retain(|(pending_app_id, _)| *pending_app_id != app_id);
            EVENTPIPE_WM_PENDING.remove(app_id);
            EVENTPIPE_APP_PENDING.remove(app_id);
            return false;
        };

        PENDING_WM_WRITERS.lock().push(app_id);
        let Some(writer) = thread::create(window_manager_event_writer) else {
            PENDING_WM_WRITERS.lock().retain(|pending| *pending != app_id);
            listener.kill();
            EVENTPIPE_WM_PENDING.remove(app_id);
            EVENTPIPE_APP_PENDING.remove(app_id);
            return false;
        };

        PROCESS_CHANNEL_THREADS.lock().push(ProcessChannelThreads {
            app_id,
            listener,
            writer,
            monitor: None,
        });

        true
    }

    /// Reap a graphical process and publish a synthetic exit if it terminates
    /// without completing the GUI protocol (panic, kill, or early startup error).
    pub fn monitor_process_exit(app_id: usize, window_id: usize, main_thread_id: usize) -> bool {
        PENDING_PROCESS_MONITORS.lock().push((app_id, window_id, main_thread_id));
        let Some(monitor) = thread::create(process_exit_monitor) else {
            PENDING_PROCESS_MONITORS.lock().retain(|(pending_app_id, _, _)| *pending_app_id != app_id);
            return false;
        };

        let mut channels = PROCESS_CHANNEL_THREADS.lock();
        if let Some(channel) = channels.iter_mut().find(|channel| channel.app_id == app_id) {
            channel.monitor = Some(monitor);
            true
        } else {
            monitor.kill();
            false
        }
    }

    /// Configure the application side of an existing channel and start its
    /// blocking WM -> APP reader thread.
    pub(crate) fn init_process_app_channel(app_id: usize, window_id: usize) -> bool {
        let pipe = EventPipe::new(app_id);
        if pipe.init().is_err() {
            return false;
        }
        if EVENTPIPE_WM_PENDING.register(app_id).is_err() || EVENTPIPE_APP_PENDING.register(app_id).is_err() {
            EVENTPIPE_WM_PENDING.remove(app_id);
            EVENTPIPE_APP_PENDING.remove(app_id);
            return false;
        }
        PROCESS_APP_ID.store(app_id, Ordering::SeqCst);
        PROCESS_WINDOW_ID.store(window_id, Ordering::SeqCst);
        let Some(listener) = thread::create(application_event_listener) else {
            return false;
        };
        let Some(writer) = thread::create(application_event_writer) else {
            listener.kill();
            return false;
        };
        // The single outbound writer multiplexes DATA and ACK packets. While it is
        // waiting for a DATA acknowledgement it continues flushing reverse ACKs,
        // avoiding both extra threads and simultaneous bidirectional deadlock.
        let _ = writer;
        true
    }

    fn window_manager_event_listener() {
        let (app_id, window_id) = {
            let mut pending = PENDING_WM_LISTENERS.lock();
            if pending.is_empty() {
                return;
            }
            pending.remove(0)
        };
        let pipe = EventPipe::get_instance(app_id);

        loop {
            match pipe.read_reliable(EventRole::WindowManager) {
                Ok(_) => {
                    while let Some(delivery) = pipe.next_reliable(EventRole::WindowManager) {
                        let sequence = delivery.sequence;
                        let Ok(protocol) = from_json(&delivery.payload) else {
                            // The transport delivered valid bytes. Consume an
                            // invalid GUI payload so it cannot wedge the route.
                            let _ = pipe.complete_reliable(EventRole::WindowManager, sequence);
                            continue;
                        };

                        let route_matches = protocol.matches_route(app_id, window_id);
                        let direction_allowed = matches!(
                            &protocol.event,
                            GUIEvent::GUI_init { .. } | GUIEvent::GUI_exit { .. } | GUIEvent::WindowTitle { .. }
                        );
                        if !route_matches || !direction_allowed {
                            // The FIFO route is trusted; payload-provided
                            // identities and privileged WM commands are not.
                            let _ = pipe.complete_reliable(EventRole::WindowManager, sequence);
                            continue;
                        }

                        let should_exit = matches!(&protocol.event, GUIEvent::GUI_exit { .. });
                        match publish_event(&MAILBOX_WM, &EVENTPIPE_WM_PENDING, app_id, protocol) {
                            Ok(_) => {
                                let _ = pipe.complete_reliable(EventRole::WindowManager, sequence);
                                if should_exit {
                                    return;
                                }
                            }
                            Err(EventChannelPostError::Mailbox(MailboxPostError::Full)) => {
                                // Keep the packet pending and unacknowledged. A
                                // retransmission retries delivery after space is
                                // available.
                                break;
                            }
                            Err(EventChannelPostError::Mailbox(MailboxPostError::Closed)) => {
                                let _ = pipe.complete_reliable(EventRole::WindowManager, sequence);
                            }
                            Err(EventChannelPostError::Pending(_)) => break,
                        }
                    }
                }
                _ => thread::sleep(1),
            }
        }
    }

    /// Drain WM -> APP messages for one app and perform FIFO writes away from the
    /// Window Manager main loop. One worker owns each app's FIFO writer side, so
    /// message order is preserved without blocking composition or input.
    fn window_manager_event_writer() {
        let app_id = {
            let mut pending = PENDING_WM_WRITERS.lock();
            if pending.is_empty() {
                return;
            }
            pending.remove(0)
        };
        let pipe = EventPipe::get_instance(app_id);
        let mut idle = IdleBackoff::new();

        loop {
            let _ = pipe.flush_reliable_acks(EventRole::WindowManager);
            if !app_events_pending(app_id) {
                idle.wait();
                continue;
            }
            idle.reset();

            let mut protocols = take_events_for_app(app_id).into_iter();
            while let Some(protocol) = protocols.next() {
                let should_exit = matches!(protocol.event, GUIEvent::CloseRequested);
                send_reliable_event(&pipe, EventRole::WindowManager, &protocol);

                if should_exit {
                    return;
                }
            }
        }
    }

    fn application_event_listener() {
        let Some(app_id) = process_app_id() else {
            return;
        };
        let Some(window_id) = process_window_id() else {
            return;
        };
        let pipe = EventPipe::get_instance(app_id);

        loop {
            match pipe.read_reliable(EventRole::Application) {
                Ok(_) => {
                    while let Some(delivery) = pipe.next_reliable(EventRole::Application) {
                        let sequence = delivery.sequence;
                        let Ok(protocol) = from_json(&delivery.payload) else {
                            let _ = pipe.complete_reliable(EventRole::Application, sequence);
                            continue;
                        };

                        let route_matches = protocol.matches_route(app_id, window_id);
                        let direction_allowed = matches!(&protocol.event, GUIEvent::CloseRequested);
                        if !route_matches || !direction_allowed {
                            let _ = pipe.complete_reliable(EventRole::Application, sequence);
                            continue;
                        }

                        let should_exit = matches!(&protocol.event, GUIEvent::CloseRequested);
                        match publish_event(&MAILBOX_APP, &EVENTPIPE_APP_PENDING, app_id, protocol) {
                            Ok(_) => {
                                let _ = pipe.complete_reliable(EventRole::Application, sequence);
                                if should_exit {
                                    return;
                                }
                            }
                            Err(EventChannelPostError::Mailbox(MailboxPostError::Full)) => break,
                            Err(EventChannelPostError::Mailbox(MailboxPostError::Closed)) => {
                                let _ = pipe.complete_reliable(EventRole::Application, sequence);
                            }
                            Err(EventChannelPostError::Pending(_)) => break,
                        }
                    }
                }
                _ => thread::sleep(1),
            }
        }
    }

    /// Deliver APP -> WM messages away from the application render thread.
    ///
    /// FIFO endpoints are opened by separate processes and may temporarily miss
    /// one another during startup or reconnect. The stop-and-wait sender retains
    /// the current protocol until it is acknowledged, while newer protocols stay
    /// ordered in this worker's drained batch or the process-local mailbox.
    fn application_event_writer() {
        let Some(app_id) = process_app_id() else {
            return;
        };
        let pipe = EventPipe::get_instance(app_id);
        let mut idle = IdleBackoff::new();

        loop {
            let _ = pipe.flush_reliable_acks(EventRole::Application);
            if !EVENTPIPE_WM_PENDING.contains(app_id) {
                idle.wait();
                continue;
            }
            idle.reset();

            EVENTPIPE_WM_PENDING.clear(app_id);
            let mut protocols = MAILBOX_WM.lock().take(app_id).into_iter();
            while let Some(protocol) = protocols.next() {
                let should_exit = matches!(protocol.event, GUIEvent::GUI_exit { .. });
                send_reliable_event(&pipe, EventRole::Application, &protocol);

                if should_exit {
                    return;
                }
            }
        }
    }

    fn process_exit_monitor() {
        let (app_id, window_id, main_thread_id) = {
            let mut pending = PENDING_PROCESS_MONITORS.lock();
            if pending.is_empty() {
                return;
            }
            pending.remove(0)
        };

        let _ = syscall::syscall(syscall::SystemCall::ThreadJoin, &[main_thread_id]);
        let _ = post_event_to_wm(app_id, GUIEventProtocol::new(app_id, Some(window_id), GUIEvent::GUI_exit { app_id }));
    }
}

pub use self::event_pipe_runtime::{
    EventChannelPostError, PendingAppError, init_process_event_channel, mailbox_stats_for_app, mailbox_stats_for_wm, monitor_process_exit,
    rejected_pending_route_registrations, release_process_state, take_events_for_wm, take_pending_events_for_wm,
};
pub(crate) use self::event_pipe_runtime::{
    app_events_pending, init_process_app_channel, post_event_to_wm, process_app_id, send_event_to_app, take_events_for_app,
};
