//! Process-local staging mailboxes for the reliable GUI control channel.
//!
//! A mailbox is not an IPC transport. Each process has its own pair of
//! mailboxes and the EventPipe workers bridge them across the process boundary:
//! local outbox -> FIFO -> peer inbox.

use alloc::{collections::VecDeque, vec::Vec};

use crate::event::{events::GUIEvent, protocol::GUIEventProtocol};

pub const MAX_MESSAGES_PER_APP: usize = 1024;
const LIFECYCLE_RESERVED_SLOTS: usize = 2;
const NORMAL_MESSAGE_LIMIT: usize = MAX_MESSAGES_PER_APP - LIFECYCLE_RESERVED_SLOTS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailboxPostError {
    Full,
    Closed,
}

impl MailboxPostError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "GUI event mailbox is full",
            Self::Closed => "GUI event route is closed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailboxPostOutcome {
    Queued,
    Coalesced,
    Duplicate,
}

impl MailboxPostOutcome {
    pub const fn needs_notification(self) -> bool {
        matches!(self, Self::Queued | Self::Coalesced)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MailboxStats {
    pub pending_messages: usize,
    pub high_watermark: usize,
    pub rejected_full: usize,
    pub rejected_closed: usize,
    pub coalesced: usize,
    pub duplicates: usize,
}

struct RouteMailbox {
    app_id: usize,
    messages: VecDeque<GUIEventProtocol>,
    init_seen: bool,
    terminal_seen: bool,
    high_watermark: usize,
    rejected_full: usize,
    rejected_closed: usize,
    coalesced: usize,
    duplicates: usize,
}

impl RouteMailbox {
    fn new(app_id: usize) -> Self {
        Self {
            app_id,
            messages: VecDeque::new(),
            init_seen: false,
            terminal_seen: false,
            high_watermark: 0,
            rejected_full: 0,
            rejected_closed: 0,
            coalesced: 0,
            duplicates: 0,
        }
    }

    fn stats(&self) -> MailboxStats {
        MailboxStats {
            pending_messages: self.messages.len(),
            high_watermark: self.high_watermark,
            rejected_full: self.rejected_full,
            rejected_closed: self.rejected_closed,
            coalesced: self.coalesced,
            duplicates: self.duplicates,
        }
    }

    fn record_depth(&mut self) {
        self.high_watermark = self.high_watermark.max(self.messages.len());
    }
}

/// Bounded, per-application mailbox used by the EventPipe transport workers.
///
/// Each route owns a `VecDeque`, so posting and draining no longer scan or
/// compact messages belonging to every other application. Two slots are
/// reserved for the one init and one terminal lifecycle event. Title updates
/// are state, not commands, and are coalesced to their latest value.
pub struct InMemoryMailbox {
    routes: Vec<RouteMailbox>,
}

impl InMemoryMailbox {
    pub const fn new() -> Self {
        Self { routes: Vec::new() }
    }

    pub fn post(&mut self, app_id: usize, protocol: GUIEventProtocol) -> Result<MailboxPostOutcome, MailboxPostError> {
        let route = self.route_mut(app_id);
        let is_init = matches!(&protocol.event, GUIEvent::GUI_init { .. });
        let is_terminal = matches!(&protocol.event, GUIEvent::GUI_exit { .. } | GUIEvent::CloseRequested);
        let is_title = matches!(&protocol.event, GUIEvent::WindowTitle { .. });

        if route.terminal_seen {
            if is_terminal {
                route.duplicates += 1;
                return Ok(MailboxPostOutcome::Duplicate);
            }
            route.rejected_closed += 1;
            return Err(MailboxPostError::Closed);
        }

        if is_init && route.init_seen {
            route.duplicates += 1;
            return Ok(MailboxPostOutcome::Duplicate);
        }

        if is_title {
            if let Some(index) = route.messages.iter().position(|queued| matches!(&queued.event, GUIEvent::WindowTitle { .. })) {
                route.messages.remove(index);
                route.messages.push_back(protocol);
                route.coalesced += 1;
                route.record_depth();
                return Ok(MailboxPostOutcome::Coalesced);
            }
        }

        let limit = if is_init || is_terminal { MAX_MESSAGES_PER_APP } else { NORMAL_MESSAGE_LIMIT };
        if route.messages.len() >= limit {
            route.rejected_full += 1;
            return Err(MailboxPostError::Full);
        }

        route.messages.push_back(protocol);
        route.init_seen |= is_init;
        route.terminal_seen |= is_terminal;
        route.record_depth();
        Ok(MailboxPostOutcome::Queued)
    }

    pub fn take(&mut self, app_id: usize) -> Vec<GUIEventProtocol> {
        let Some(route) = self.routes.iter_mut().find(|route| route.app_id == app_id) else {
            return Vec::new();
        };

        core::mem::take(&mut route.messages).into_iter().collect()
    }

    pub fn release(&mut self, app_id: usize) {
        if let Some(index) = self.routes.iter().position(|route| route.app_id == app_id) {
            self.routes.remove(index);
        }
    }

    pub fn stats(&self, app_id: usize) -> Option<MailboxStats> {
        self.routes.iter().find(|route| route.app_id == app_id).map(RouteMailbox::stats)
    }

    fn route_mut(&mut self, app_id: usize) -> &mut RouteMailbox {
        if let Some(index) = self.routes.iter().position(|route| route.app_id == app_id) {
            return &mut self.routes[index];
        }

        self.routes.push(RouteMailbox::new(app_id));
        self.routes.last_mut().expect("mailbox route was just inserted")
    }
}

impl Default for InMemoryMailbox {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use super::*;
    use crate::api::WindowManagerMessage;

    fn protocol(event: GUIEvent) -> GUIEventProtocol {
        GUIEventProtocol::new(1, Some(7), event)
    }

    #[test]
    fn coalesces_window_titles_at_the_latest_queue_position() {
        let mut mailbox = InMemoryMailbox::new();
        mailbox
            .post(
                1,
                protocol(GUIEvent::WindowTitle {
                    title: Some(String::from("old")),
                }),
            )
            .unwrap();
        mailbox
            .post(
                1,
                GUIEventProtocol::new(
                    1,
                    None,
                    GUIEvent::WindowManagerMessage {
                        message: WindowManagerMessage::CreateNewWorkspace,
                    },
                ),
            )
            .unwrap();
        assert_eq!(
            mailbox.post(
                1,
                protocol(GUIEvent::WindowTitle {
                    title: Some(String::from("new"))
                })
            ),
            Ok(MailboxPostOutcome::Coalesced)
        );

        let events = mailbox.take(1);
        assert!(matches!(&events[0].event, GUIEvent::WindowManagerMessage { .. }));
        assert!(matches!(
            &events[1].event,
            GUIEvent::WindowTitle { title: Some(title) } if title == "new"
        ));
    }

    #[test]
    fn preserves_reserved_lifecycle_capacity() {
        let mut mailbox = InMemoryMailbox::new();
        for _ in 0..NORMAL_MESSAGE_LIMIT {
            mailbox
                .post(
                    1,
                    GUIEventProtocol::new(
                        1,
                        None,
                        GUIEvent::WindowManagerMessage {
                            message: WindowManagerMessage::CreateNewWorkspace,
                        },
                    ),
                )
                .unwrap();
        }

        assert_eq!(
            mailbox.post(
                1,
                GUIEventProtocol::new(
                    1,
                    None,
                    GUIEvent::WindowManagerMessage {
                        message: WindowManagerMessage::CreateNewWorkspace,
                    },
                ),
            ),
            Err(MailboxPostError::Full)
        );
        assert_eq!(mailbox.post(1, protocol(GUIEvent::GUI_init { app_id: 1 })), Ok(MailboxPostOutcome::Queued));
        assert_eq!(mailbox.post(1, protocol(GUIEvent::GUI_exit { app_id: 1 })), Ok(MailboxPostOutcome::Queued));
        assert_eq!(mailbox.stats(1).unwrap().pending_messages, MAX_MESSAGES_PER_APP);
    }

    #[test]
    fn closes_route_after_terminal_event() {
        let mut mailbox = InMemoryMailbox::new();
        mailbox.post(1, protocol(GUIEvent::CloseRequested)).unwrap();

        assert_eq!(mailbox.post(1, protocol(GUIEvent::WindowTitle { title: None })), Err(MailboxPostError::Closed));
        assert_eq!(mailbox.post(1, protocol(GUIEvent::CloseRequested)), Ok(MailboxPostOutcome::Duplicate));
    }
}
