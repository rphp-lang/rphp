use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::time::Instant;

#[derive(Default)]
pub(super) struct Readiness {
    ready: VecDeque<u64>,
    timers: BinaryHeap<Reverse<(Instant, u64, u64)>>,
    next_timer_sequence: u64,
}

impl Readiness {
    pub(super) fn enqueue(&mut self, task: u64) {
        self.ready.push_back(task);
    }

    pub(super) fn pop_ready(&mut self) -> Option<u64> {
        self.ready.pop_front()
    }

    pub(super) fn remove_ready(&mut self, task: u64) {
        if let Some(position) = self.ready.iter().position(|queued| *queued == task) {
            self.ready.remove(position);
        }
    }

    pub(super) fn schedule_timer(&mut self, task: u64, deadline: Instant) {
        let sequence = self.next_timer_sequence;
        self.next_timer_sequence = self
            .next_timer_sequence
            .checked_add(1)
            .expect("coroutine timer sequence exhausted");
        self.timers.push(Reverse((deadline, sequence, task)));
    }

    pub(super) fn cancel_timer(&mut self, task: u64) {
        self.timers
            .retain(|Reverse((_, _, queued_task))| *queued_task != task);
    }

    pub(super) fn drain_due(&mut self, now: Instant, due: &mut Vec<u64>) {
        while let Some(Reverse((deadline, _, _))) = self.timers.peek() {
            if *deadline > now {
                break;
            }
            let Reverse((_, _, task)) = self.timers.pop().unwrap();
            due.push(task);
        }
    }

    pub(super) fn next_deadline(&self) -> Option<Instant> {
        self.timers
            .peek()
            .map(|Reverse((deadline, _, _))| *deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn ready_tasks_and_equal_deadlines_are_fifo_and_stable() {
        let mut readiness = Readiness::default();
        readiness.enqueue(3);
        readiness.enqueue(4);
        assert_eq!(readiness.pop_ready(), Some(3));
        assert_eq!(readiness.pop_ready(), Some(4));

        let deadline = Instant::now() + Duration::from_millis(1);
        readiness.schedule_timer(2, deadline);
        readiness.schedule_timer(1, deadline);
        let mut due = Vec::new();
        readiness.drain_due(deadline, &mut due);
        assert_eq!(due, vec![2, 1]);
    }

    #[test]
    fn direct_resume_can_remove_a_queued_task() {
        let mut readiness = Readiness::default();
        readiness.enqueue(1);
        readiness.enqueue(2);
        readiness.remove_ready(1);
        assert_eq!(readiness.pop_ready(), Some(2));
        assert_eq!(readiness.pop_ready(), None);
    }

    #[test]
    fn cancelled_timer_no_longer_contributes_a_deadline() {
        let mut readiness = Readiness::default();
        let deadline = Instant::now() + Duration::from_secs(1);
        readiness.schedule_timer(7, deadline);
        assert_eq!(readiness.next_deadline(), Some(deadline));

        readiness.cancel_timer(7);

        assert_eq!(readiness.next_deadline(), None);
        let mut due = Vec::new();
        readiness.drain_due(deadline, &mut due);
        assert!(due.is_empty());
    }
}
