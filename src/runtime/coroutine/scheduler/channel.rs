use std::collections::{HashMap, VecDeque};

use crate::value::Value;
use crate::vm::execute::VmError;
use crate::vm::frame::ExecuteData;

pub(super) struct ReceiveWaiter {
    pub(super) task: u64,
    pub(super) frame: *mut ExecuteData,
    pub(super) return_value: *mut Value,
}

struct SendWaiter {
    task: u64,
    value: Value,
}

struct Channel {
    capacity: usize,
    buffer: VecDeque<Value>,
    senders: VecDeque<SendWaiter>,
    receivers: VecDeque<ReceiveWaiter>,
}

impl Channel {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            // Capacity is a logical backpressure bound, not an eager memory
            // reservation controlled by PHP input.
            buffer: VecDeque::new(),
            senders: VecDeque::new(),
            receivers: VecDeque::new(),
        }
    }
}

pub(super) enum SendOutcome {
    Complete,
    Blocked,
    WakeReceiver { waiter: ReceiveWaiter, value: Value },
}

pub(super) enum ReceiveOutcome {
    Ready {
        value: Value,
        wake_sender: Option<u64>,
    },
    Blocked,
}

pub(super) struct ChannelSet {
    next_id: u64,
    channels: HashMap<u64, Channel>,
}

impl Default for ChannelSet {
    fn default() -> Self {
        Self {
            next_id: 1,
            channels: HashMap::new(),
        }
    }
}

impl ChannelSet {
    pub(super) fn create(&mut self, capacity: usize) -> Result<u64, VmError> {
        if capacity == 0 {
            return Err(VmError::Fatal(
                "coroutine_channel capacity must be greater than zero".into(),
            ));
        }
        let id = self.next_id;
        if id > i64::MAX as u64 {
            return Err(VmError::Fatal(
                "coroutine channel identifier space exhausted".into(),
            ));
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| VmError::Fatal("coroutine channel identifier space exhausted".into()))?;
        self.channels.insert(id, Channel::new(capacity));
        Ok(id)
    }

    pub(super) fn send(
        &mut self,
        id: u64,
        task: u64,
        value: Value,
    ) -> Result<SendOutcome, VmError> {
        let channel = self.channel_mut(id)?;
        if let Some(waiter) = channel.receivers.pop_front() {
            return Ok(SendOutcome::WakeReceiver { waiter, value });
        }
        if channel.buffer.len() < channel.capacity {
            channel.buffer.push_back(value);
            return Ok(SendOutcome::Complete);
        }
        channel.senders.push_back(SendWaiter { task, value });
        Ok(SendOutcome::Blocked)
    }

    pub(super) fn receive(
        &mut self,
        id: u64,
        task: u64,
        frame: *mut ExecuteData,
        return_value: *mut Value,
    ) -> Result<ReceiveOutcome, VmError> {
        let channel = self.channel_mut(id)?;
        if let Some(value) = channel.buffer.pop_front() {
            let wake_sender = channel.senders.pop_front().map(|sender| {
                channel.buffer.push_back(sender.value);
                sender.task
            });
            return Ok(ReceiveOutcome::Ready { value, wake_sender });
        }

        channel.receivers.push_back(ReceiveWaiter {
            task,
            frame,
            return_value,
        });
        Ok(ReceiveOutcome::Blocked)
    }

    fn channel_mut(&mut self, id: u64) -> Result<&mut Channel, VmError> {
        self.channels
            .get_mut(&id)
            .ok_or_else(|| VmError::Fatal(format!("unknown coroutine channel {}", id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_channel_preserves_fifo_for_buffer_and_blocked_senders() {
        let mut channels = ChannelSet::default();
        let id = channels.create(1).unwrap();
        assert!(matches!(
            channels.send(id, 1, Value::long(10)).unwrap(),
            SendOutcome::Complete
        ));
        assert!(matches!(
            channels.send(id, 2, Value::long(20)).unwrap(),
            SendOutcome::Blocked
        ));

        let ReceiveOutcome::Ready { value, wake_sender } = channels
            .receive(id, 3, std::ptr::null_mut(), std::ptr::null_mut())
            .unwrap()
        else {
            panic!("buffered value must be ready");
        };
        assert_eq!(value.as_long(), Some(10));
        assert_eq!(wake_sender, Some(2));

        let ReceiveOutcome::Ready { value, wake_sender } = channels
            .receive(id, 3, std::ptr::null_mut(), std::ptr::null_mut())
            .unwrap()
        else {
            panic!("promoted sender value must be ready");
        };
        assert_eq!(value.as_long(), Some(20));
        assert_eq!(wake_sender, None);
    }

    #[test]
    fn direct_handoff_wakes_waiting_receivers_in_fifo_order() {
        let mut channels = ChannelSet::default();
        let id = channels.create(1).unwrap();
        assert!(matches!(
            channels
                .receive(id, 1, std::ptr::null_mut(), std::ptr::null_mut())
                .unwrap(),
            ReceiveOutcome::Blocked
        ));
        assert!(matches!(
            channels
                .receive(id, 2, std::ptr::null_mut(), std::ptr::null_mut())
                .unwrap(),
            ReceiveOutcome::Blocked
        ));

        let SendOutcome::WakeReceiver { waiter, value } =
            channels.send(id, 3, Value::long(10)).unwrap()
        else {
            panic!("oldest receiver must accept the direct handoff");
        };
        assert_eq!(waiter.task, 1);
        assert_eq!(value.as_long(), Some(10));

        let SendOutcome::WakeReceiver { waiter, value } =
            channels.send(id, 3, Value::long(20)).unwrap()
        else {
            panic!("second receiver must accept the next direct handoff");
        };
        assert_eq!(waiter.task, 2);
        assert_eq!(value.as_long(), Some(20));
    }
}
