use core::str::from_utf8;

use alloc::{
    collections::VecDeque,
    format,
    string::{String, ToString},
    vec::Vec,
};
use concurrent::thread;
use naming::{close, mkdir, mkfifo, open, read, shared_types::OpenOptions, write};
use serde::{Deserialize, Serialize};
use spin::Mutex;
use syscall::return_vals::Errno;
use terminal::println;

const RELIABLE_EVENT_PIPE_VERSION: u8 = 1;
const RELIABLE_ACK_WAIT_MS: usize = 50;
const RELIABLE_ACK_FAST_YIELDS: usize = 4;
const RELIABLE_MAX_TRANSMISSIONS: usize = 8;
const RELIABLE_REORDER_WINDOW: u64 = 64;

/// A small TCP-like transport envelope carried by the two FIFO directions.
///
/// The sender uses stop-and-wait, while the receiver retains a bounded reorder
/// window so delayed packets can still be delivered when a preceding gap is
/// filled. Delivery acknowledgement is explicit: callers ACK only after the
/// payload has been accepted by their local destination.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "packet_type")]
pub enum ReliableEventPacket {
    Data {
        version: u8,
        sequence: u64,
        checksum: u32,
        payload: String,
    },
    Ack {
        version: u8,
        sequence: u64,
    },
}

impl ReliableEventPacket {
    pub fn data(sequence: u64, payload: String) -> Self {
        Self::Data {
            version: RELIABLE_EVENT_PIPE_VERSION,
            sequence,
            checksum: reliable_checksum(payload.as_bytes()),
            payload,
        }
    }

    pub fn ack(sequence: u64) -> Self {
        Self::Ack {
            version: RELIABLE_EVENT_PIPE_VERSION,
            sequence,
        }
    }

    pub fn is_supported(&self) -> bool {
        match self {
            Self::Data { version, .. } | Self::Ack { version, .. } => *version == RELIABLE_EVENT_PIPE_VERSION,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Reliable EventPipe packet serialization failed")
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// FNV-1a is intentionally small and deterministic in `no_std`. It is not a
/// cryptographic primitive; it detects truncated/corrupt FIFO payloads so they
/// are left unacknowledged and retransmitted.
pub fn reliable_checksum(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReliableDelivery {
    pub sequence: u64,
    pub payload: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReliableIngestReport {
    pub data_packets: usize,
    pub acknowledgement_packets: usize,
    pub duplicates: usize,
    pub reordered: usize,
    pub rejected: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReliableSendError {
    Busy,
    Pipe(Errno),
    AckTimeout { sequence: u64, transmissions: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventPipe {
    app_id: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventRole {
    Application,
    WindowManager,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventAction {
    Write,
    Read,
}

struct PendingSend {
    sequence: u64,
    payload: String,
    encoded_packet: String,
    acknowledged: bool,
}

struct BufferedPacket {
    sequence: u64,
    payload: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceiveDisposition {
    InOrder,
    Reordered,
    DuplicateBuffered,
    DuplicateCommitted,
    OutsideWindow,
}

struct ReliableRouteState {
    app_id: usize,
    role: EventRole,
    next_send_sequence: u64,
    pending_send: Option<PendingSend>,
    expected_receive_sequence: u64,
    receive_buffer: Vec<BufferedPacket>,
    pending_acknowledgements: VecDeque<u64>,
}

impl ReliableRouteState {
    fn new(app_id: usize, role: EventRole) -> Self {
        Self {
            app_id,
            role,
            next_send_sequence: 1,
            pending_send: None,
            expected_receive_sequence: 1,
            receive_buffer: Vec::new(),
            pending_acknowledgements: VecDeque::new(),
        }
    }

    fn queue_acknowledgement(&mut self, sequence: u64) {
        if !self.pending_acknowledgements.contains(&sequence) {
            self.pending_acknowledgements.push_back(sequence);
        }
    }

    fn observe_acknowledgement(&mut self, sequence: u64) {
        if let Some(pending) = self.pending_send.as_mut()
            && pending.sequence == sequence
        {
            pending.acknowledged = true;
        }
    }

    fn ingest_data(&mut self, sequence: u64, payload: String) -> ReceiveDisposition {
        if sequence == self.expected_receive_sequence {
            if self.receive_buffer.iter().any(|packet| packet.sequence == sequence) {
                return ReceiveDisposition::DuplicateBuffered;
            }
            self.receive_buffer.push(BufferedPacket { sequence, payload });
            return ReceiveDisposition::InOrder;
        }

        if let Some(distance) = sequence_distance_ahead(sequence, self.expected_receive_sequence) {
            if distance > RELIABLE_REORDER_WINDOW {
                return ReceiveDisposition::OutsideWindow;
            }
            if self.receive_buffer.iter().any(|packet| packet.sequence == sequence) {
                return ReceiveDisposition::DuplicateBuffered;
            }
            self.receive_buffer.push(BufferedPacket { sequence, payload });
            return ReceiveDisposition::Reordered;
        }

        // Already committed packets are acknowledged again so loss of the
        // original ACK cannot cause the sender to retransmit forever.
        self.queue_acknowledgement(sequence);
        ReceiveDisposition::DuplicateCommitted
    }

    fn next_delivery(&self) -> Option<ReliableDelivery> {
        let packet = self.receive_buffer.iter().find(|packet| packet.sequence == self.expected_receive_sequence)?;
        Some(ReliableDelivery {
            sequence: packet.sequence,
            payload: packet.payload.clone(),
        })
    }

    fn complete_delivery(&mut self, sequence: u64) -> bool {
        if sequence != self.expected_receive_sequence {
            return false;
        }
        let Some(index) = self.receive_buffer.iter().position(|packet| packet.sequence == sequence) else {
            return false;
        };
        self.receive_buffer.remove(index);
        self.expected_receive_sequence = next_reliable_sequence(sequence);
        self.queue_acknowledgement(sequence);
        true
    }
}

static RELIABLE_STATES: Mutex<Vec<ReliableRouteState>> = Mutex::new(Vec::new());

/// FIFO writes from different EventPipe workers must not interleave when a JSON
/// packet is larger than the platform's atomic FIFO write size.
static RELIABLE_WRITE_LOCK: Mutex<()> = Mutex::new(());

fn next_reliable_sequence(sequence: u64) -> u64 {
    let next = sequence.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

fn sequence_distance_ahead(sequence: u64, expected: u64) -> Option<u64> {
    let distance = sequence.wrapping_sub(expected);
    if distance != 0 && distance < (1u64 << 63) { Some(distance) } else { None }
}

fn reliable_state_mut(states: &mut Vec<ReliableRouteState>, app_id: usize, role: EventRole) -> &mut ReliableRouteState {
    if let Some(index) = states.iter().position(|state| state.app_id == app_id && state.role == role) {
        return &mut states[index];
    }
    states.push(ReliableRouteState::new(app_id, role));
    states.last_mut().expect("EventPipe reliable state was just inserted")
}

impl EventPipe {
    pub fn new(app_id: usize) -> Self {
        ensure_pipe_path(&format!("/tmp/gui/{}", app_id));
        return EventPipe { app_id };
    }

    pub fn init(&self) -> Result<(), Errno> {
        match mkfifo(&format!("/tmp/gui/{}/app2wm", self.app_id)) {
            Ok(_) => {
                println!("EventPipe create success for {} with direction to Window Manager", self.app_id);
            }
            // FIFO names may survive a Window Manager restart. Reusing the
            // existing pipe is safe: reader/writer handles are process-local.
            Err(Errno::EEXIST) => {
                println!("EventPipe already exists for {} with direction to Window Manager", self.app_id);
            }
            Err(e) => {
                println!("EventPipe create failed for {} with direction to Window Manager, because {:?}", self.app_id, e);
                return Err(e);
            }
        }

        match mkfifo(&format!("/tmp/gui/{}/wm2app", self.app_id)) {
            Ok(_) => {
                println!("EventPipe create success for {} with direction to Application", self.app_id);
            }
            Err(Errno::EEXIST) => {
                println!("EventPipe already exists for {} with direction to Application", self.app_id);
            }
            Err(e) => {
                println!("EventPipe create failed for {} with direction to Application, because {:?}", self.app_id, e);
                return Err(e);
            }
        }

        return Ok(());
    }

    pub fn get_instance(app_id: usize) -> Self {
        return EventPipe { app_id };
    }

    pub fn get_pipe(&self, role: EventRole, action: EventAction) -> String {
        match role {
            EventRole::Application => match action {
                EventAction::Write => format!("/tmp/gui/{}/app2wm", self.app_id),
                EventAction::Read => format!("/tmp/gui/{}/wm2app", self.app_id),
            },
            EventRole::WindowManager => match action {
                EventAction::Write => format!("/tmp/gui/{}/wm2app", self.app_id),
                EventAction::Read => format!("/tmp/gui/{}/app2wm", self.app_id),
            },
        }
    }

    pub fn write_data(&self, role: EventRole, data: &[u8]) -> Result<(), Errno> {
        let pipe = self.get_pipe(role, EventAction::Write);
        let mut framed = Vec::with_capacity(data.len() + 1);
        framed.extend_from_slice(data);
        framed.push(0);

        // `read_data` intentionally reconnects for every batch. A writer can
        // rendezvous with the previous reader just as that reader closes and
        // receive EPIPE before writing any byte. Retry that safe case so an
        // lifecycle/control notification cannot disappear silently.
        for attempt in 0..8 {
            let fh = match open(&pipe, OpenOptions::WRITEONLY) {
                Ok(fh) => fh,
                Err(_error) if attempt < 7 => {
                    thread::sleep(1);
                    continue;
                }
                Err(error) => {
                    println!("EventPipe cannot be opened on {}, because {:?}", pipe, error);
                    return Err(error);
                }
            };

            let mut written = 0usize;
            let result = loop {
                if written == framed.len() {
                    break Ok(());
                }

                match write(fh, &framed[written..]) {
                    Ok(0) => break Err(Errno::EPIPE),
                    Ok(count) => written += count,
                    Err(error) => break Err(error),
                }
            };
            let _ = close(fh);

            match result {
                Ok(()) => return Ok(()),
                Err(Errno::EPIPE) if written == 0 && attempt < 7 => {
                    thread::sleep(1);
                }
                Err(error) => {
                    println!("EventPipe cannot write data on {} after {}, because {:?}", pipe, written, error);
                    return Err(error);
                }
            }
        }

        Err(Errno::EPIPE)
    }

    pub fn read_data(&self, role: EventRole) -> Result<Vec<u8>, Errno> {
        let pipe = self.get_pipe(role, EventAction::Read);
        let fh = open(&pipe, OpenOptions::READONLY).map_err(|e| {
            println!("EventPipe cannot be opened on {}, because{:?}", pipe, e);
            return e;
        })?;

        let mut buffer: [u8; 4096] = [0; 4096];
        let mut data: Vec<u8> = Vec::new();

        let read_result = loop {
            match read(fh, &mut buffer) {
                Ok(0) => break Ok(()),
                Ok(n) => data.extend_from_slice(&buffer[..n]),
                Err(e) => {
                    println!("Event Pipe cannot read data on {}, because {:?}", pipe, e);
                    break Err(e);
                }
            }
        };

        let close_result = close(fh);
        if let Err(error) = close_result {
            println!("Event Pipe {} cannot be closed, because {:?}", pipe, error);
        }

        read_result?;
        close_result?;
        Ok(data)
    }

    /// Read and classify every framed packet in one FIFO connection.
    ///
    /// ACK packets advance the sender state. Valid DATA packets are retained in
    /// the receive reorder window; the caller obtains only the next contiguous
    /// packet through [`Self::next_reliable`] and explicitly completes it after
    /// local acceptance.
    pub fn read_reliable(&self, role: EventRole) -> Result<ReliableIngestReport, Errno> {
        let data = self.read_data(role)?;
        Ok(self.ingest_reliable_bytes(role, &data))
    }

    /// Feed already-read EventPipe bytes into the reliability state machine.
    ///
    /// This method is also useful for deterministic tests because it performs no
    /// FIFO I/O.
    pub fn ingest_reliable_bytes(&self, role: EventRole, data: &[u8]) -> ReliableIngestReport {
        let mut report = ReliableIngestReport::default();

        for frame in data.split(|byte| *byte == 0).filter(|frame| !frame.is_empty()) {
            let Ok(json) = from_utf8(frame) else {
                report.rejected += 1;
                continue;
            };
            let Ok(packet) = ReliableEventPacket::from_json(json) else {
                report.rejected += 1;
                continue;
            };
            if !packet.is_supported() {
                report.rejected += 1;
                continue;
            }

            let mut states = RELIABLE_STATES.lock();
            let state = reliable_state_mut(&mut states, self.app_id, role);
            match packet {
                ReliableEventPacket::Ack { sequence, .. } => {
                    if sequence == 0 {
                        report.rejected += 1;
                        continue;
                    }
                    report.acknowledgement_packets += 1;
                    state.observe_acknowledgement(sequence);
                }
                ReliableEventPacket::Data {
                    sequence, checksum, payload, ..
                } => {
                    if sequence == 0 || checksum != reliable_checksum(payload.as_bytes()) {
                        report.rejected += 1;
                        continue;
                    }
                    report.data_packets += 1;
                    match state.ingest_data(sequence, payload) {
                        ReceiveDisposition::InOrder => {}
                        ReceiveDisposition::Reordered => report.reordered += 1,
                        ReceiveDisposition::DuplicateBuffered | ReceiveDisposition::DuplicateCommitted => {
                            report.duplicates += 1;
                        }
                        ReceiveDisposition::OutsideWindow => report.rejected += 1,
                    }
                }
            }
        }

        report
    }

    /// Return the next contiguous payload without removing it.
    ///
    /// Repeated calls return the same delivery until
    /// [`Self::complete_reliable`] is called. This two-phase interface prevents a
    /// full local queue from causing an early ACK and data loss.
    pub fn next_reliable(&self, role: EventRole) -> Option<ReliableDelivery> {
        let states = RELIABLE_STATES.lock();
        states
            .iter()
            .find(|state| state.app_id == self.app_id && state.role == role)
            .and_then(ReliableRouteState::next_delivery)
    }

    /// Commit one locally accepted payload and schedule its ACK.
    ///
    /// Completing the expected sequence also exposes any consecutively buffered
    /// out-of-order packet through [`Self::next_reliable`].
    pub fn complete_reliable(&self, role: EventRole, sequence: u64) -> bool {
        let mut states = RELIABLE_STATES.lock();
        reliable_state_mut(&mut states, self.app_id, role).complete_delivery(sequence)
    }

    /// Write all ACKs queued by completed or duplicate DATA packets.
    ///
    /// A failed ACK is restored to the front of the queue and will be retried by
    /// the next call.
    pub fn flush_reliable_acks(&self, role: EventRole) -> Result<(), Errno> {
        loop {
            let sequence = {
                let mut states = RELIABLE_STATES.lock();
                reliable_state_mut(&mut states, self.app_id, role).pending_acknowledgements.pop_front()
            };
            let Some(sequence) = sequence else {
                return Ok(());
            };

            let packet = ReliableEventPacket::ack(sequence).to_json();
            if let Err(error) = self.write_reliable_packet(role, packet.as_bytes()) {
                let mut states = RELIABLE_STATES.lock();
                let state = reliable_state_mut(&mut states, self.app_id, role);
                if !state.pending_acknowledgements.contains(&sequence) {
                    state.pending_acknowledgements.push_front(sequence);
                }
                return Err(error);
            }
        }
    }

    /// Send one payload with automatic sequence allocation and stop-and-wait
    /// acknowledgement.
    ///
    /// On each ACK timeout the exact encoded packet and sequence are
    /// retransmitted. A timeout or pipe error leaves the packet in-flight, so a
    /// subsequent call with the same payload resumes it rather than allocating a
    /// new sequence. Callers must not submit a different payload until the
    /// outstanding one has completed.
    pub fn send_reliable(&self, role: EventRole, payload: &str) -> Result<u64, ReliableSendError> {
        let (sequence, encoded_packet) = {
            let mut states = RELIABLE_STATES.lock();
            let state = reliable_state_mut(&mut states, self.app_id, role);

            match state.pending_send.as_ref() {
                Some(pending) if pending.payload != payload => return Err(ReliableSendError::Busy),
                Some(pending) => (pending.sequence, pending.encoded_packet.clone()),
                None => {
                    let sequence = state.next_send_sequence;
                    state.next_send_sequence = next_reliable_sequence(sequence);
                    let payload = payload.to_string();
                    let encoded_packet = ReliableEventPacket::data(sequence, payload.clone()).to_json();
                    state.pending_send = Some(PendingSend {
                        sequence,
                        payload,
                        encoded_packet: encoded_packet.clone(),
                        acknowledged: false,
                    });
                    (sequence, encoded_packet)
                }
            }
        };

        if self.finish_acknowledged_send(role, sequence) {
            return Ok(sequence);
        }

        let mut last_pipe_error = None;
        let mut successful_transmissions = 0usize;

        for transmission in 1..=RELIABLE_MAX_TRANSMISSIONS {
            let _ = self.flush_reliable_acks(role);
            if self.finish_acknowledged_send(role, sequence) {
                return Ok(sequence);
            }

            match self.write_reliable_packet(role, encoded_packet.as_bytes()) {
                Ok(()) => successful_transmissions += 1,
                Err(error) => {
                    last_pipe_error = Some(error);
                    if transmission < RELIABLE_MAX_TRANSMISSIONS {
                        thread::sleep(1);
                    }
                    continue;
                }
            }

            for _ in 0..RELIABLE_ACK_WAIT_MS {
                for _ in 0..RELIABLE_ACK_FAST_YIELDS {
                    thread::switch();
                    let _ = self.flush_reliable_acks(role);
                    if self.finish_acknowledged_send(role, sequence) {
                        return Ok(sequence);
                    }
                }
                thread::sleep(1);
                if self.finish_acknowledged_send(role, sequence) {
                    return Ok(sequence);
                }
            }
        }

        if successful_transmissions == 0 {
            Err(ReliableSendError::Pipe(last_pipe_error.unwrap_or(Errno::EPIPE)))
        } else {
            Err(ReliableSendError::AckTimeout {
                sequence,
                transmissions: RELIABLE_MAX_TRANSMISSIONS,
            })
        }
    }

    /// Remove all reliable transport state for an application route.
    pub fn release_reliable_state(app_id: usize) {
        RELIABLE_STATES.lock().retain(|state| state.app_id != app_id);
    }

    fn write_reliable_packet(&self, role: EventRole, packet: &[u8]) -> Result<(), Errno> {
        let _guard = RELIABLE_WRITE_LOCK.lock();
        self.write_data(role, packet)
    }

    fn finish_acknowledged_send(&self, role: EventRole, sequence: u64) -> bool {
        let mut states = RELIABLE_STATES.lock();
        let state = reliable_state_mut(&mut states, self.app_id, role);
        let acknowledged = matches!(state.pending_send.as_ref(), Some(pending) if pending.sequence == sequence && pending.acknowledged);
        if acknowledged {
            state.pending_send = None;
        }
        acknowledged
    }
}

pub fn ensure_pipe_path(path: &str) {
    let _ = mkdir("/tmp");
    let _ = mkdir("/tmp/gui");
    let mut prefix = String::new();
    for subfolder in path.split('/').filter(|c| !c.is_empty()) {
        prefix.push('/');
        prefix.push_str(subfolder);
        let _ = mkdir(&prefix);
    }
}

#[cfg(test)]
mod tests {
    use alloc::{string::String, vec};

    use super::*;

    #[test]
    fn assigns_sequences_without_using_zero() {
        assert_eq!(next_reliable_sequence(1), 2);
        assert_eq!(next_reliable_sequence(u64::MAX), 1);
    }

    #[test]
    fn filters_committed_duplicates_and_repeats_the_ack() {
        let mut state = ReliableRouteState::new(7, EventRole::Application);
        assert_eq!(state.ingest_data(1, String::from("first")), ReceiveDisposition::InOrder);
        assert_eq!(state.next_delivery().unwrap().sequence, 1);
        assert!(state.complete_delivery(1));
        assert_eq!(state.ingest_data(1, String::from("first")), ReceiveDisposition::DuplicateCommitted);
        assert_eq!(state.next_delivery(), None);
        assert_eq!(state.pending_acknowledgements.into_iter().collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn restores_order_after_a_gap_is_filled() {
        let mut state = ReliableRouteState::new(7, EventRole::WindowManager);
        assert_eq!(state.ingest_data(2, String::from("second")), ReceiveDisposition::Reordered);
        assert_eq!(state.next_delivery(), None);

        assert_eq!(state.ingest_data(1, String::from("first")), ReceiveDisposition::InOrder);
        assert_eq!(state.next_delivery().unwrap().payload, "first");
        assert!(state.complete_delivery(1));
        assert_eq!(state.next_delivery().unwrap().payload, "second");
        assert!(state.complete_delivery(2));
        assert_eq!(state.next_delivery(), None);
        assert_eq!(state.pending_acknowledgements.into_iter().collect::<Vec<_>>(), vec![1, 2]);
    }
}
