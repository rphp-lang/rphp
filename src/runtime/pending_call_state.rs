use crate::value::{PhpArray, Value};

const LATE_STATIC_TAG: usize = 1usize << (usize::BITS - 1);
const MAGIC_CALL_TAG: usize = 1usize << (usize::BITS - 2);

enum Payload {
    Receiver(Value),
    LateStatic(u32),
}

struct Record {
    key: usize,
    payload: Payload,
}

enum Storage {
    Stack(Vec<Record>),
    Opaque(Value),
}

/// Engine call metadata owns its values directly, without PHP array/COW state.
/// The reserved words preserve the existing optional Value field geometry.
pub struct PendingCallState {
    storage: Box<Storage>,
    _layout_reserve: [usize; 2],
}

const _: [(); std::mem::size_of::<Option<Value>>()] =
    [(); std::mem::size_of::<Option<PendingCallState>>()];
const _: [(); std::mem::align_of::<Option<Value>>()] =
    [(); std::mem::align_of::<Option<PendingCallState>>()];

impl Default for PendingCallState {
    fn default() -> Self {
        Self {
            storage: Box::new(Storage::Stack(Vec::with_capacity(2))),
            _layout_reserve: [0; 2],
        }
    }
}

impl From<Value> for PendingCallState {
    fn from(value: Value) -> Self {
        Self {
            storage: Box::new(Storage::Opaque(value)),
            _layout_reserve: [0; 2],
        }
    }
}

impl PendingCallState {
    #[cfg(test)]
    pub(crate) fn opaque(value: Value) -> Self {
        Self {
            storage: Box::new(Storage::Opaque(value)),
            _layout_reserve: [0; 2],
        }
    }

    pub(crate) fn push_receiver(&mut self, key: usize, value: Value) {
        match self.storage.as_mut() {
            Storage::Stack(records) => records.push(Record {
                key,
                payload: Payload::Receiver(value),
            }),
            Storage::Opaque(value_state) => {
                let array = value_state
                    .as_array_mut()
                    .expect("pending call state must remain a packed array");
                array.push(Value::long(key as i64));
                array.push(value);
            }
        }
    }

    pub(crate) fn push_late_static(&mut self, key: usize, class_id: u32) {
        debug_assert_eq!(key & LATE_STATIC_TAG, 0);
        match self.storage.as_mut() {
            Storage::Stack(records) => records.push(Record {
                key: key | LATE_STATIC_TAG,
                payload: Payload::LateStatic(class_id),
            }),
            Storage::Opaque(value) => {
                let array = value
                    .as_array_mut()
                    .expect("pending call state must remain a packed array");
                array.push(Value::long((key | LATE_STATIC_TAG) as i64));
                array.push(Value::long(i64::from(class_id)));
            }
        }
    }

    #[inline]
    pub(crate) fn has_call(&self, key: usize) -> bool {
        match self.storage.as_ref() {
            Storage::Stack(records) => records
                .last()
                .is_none_or(|record| record.key & !(LATE_STATIC_TAG | MAGIC_CALL_TAG) == key),
            Storage::Opaque(value) => {
                let Some(array) = value.as_array() else {
                    return true;
                };
                if array.len() % 2 != 0 {
                    return true;
                }
                let Some(position) = array.len().checked_sub(2) else {
                    return true;
                };
                let Some(stored) = array.get_value_at(position).and_then(Value::as_long) else {
                    return true;
                };
                stored as usize & !(LATE_STATIC_TAG | MAGIC_CALL_TAG) == key
            }
        }
    }

    pub(crate) fn take_receiver(&mut self, key: usize) -> Option<Value> {
        match self.storage.as_mut() {
            Storage::Stack(records) => {
                if records.last()?.key != key {
                    return None;
                }
                match records.pop()?.payload {
                    Payload::Receiver(value) => Some(value),
                    Payload::LateStatic(class_id) => Some(Value::long(i64::from(class_id))),
                }
            }
            Storage::Opaque(value) => {
                let array = value.as_array()?;
                let position = array.len().checked_sub(2)?;
                if array.get_value_at(position)?.as_long()? as usize != key {
                    return None;
                }
                let array = value.as_array_mut()?;
                let receiver = array.pop()?;
                let _key = array.pop()?;
                Some(receiver)
            }
        }
    }

    #[inline]
    pub(crate) fn late_static(&self, key: usize) -> u32 {
        match self.storage.as_ref() {
            Storage::Stack(records) => match records.last() {
                Some(Record {
                    key: stored,
                    payload: Payload::LateStatic(class_id),
                }) if *stored == key | LATE_STATIC_TAG => *class_id,
                _ => 0,
            },
            Storage::Opaque(value) => {
                let Some(array) = value.as_array() else {
                    return 0;
                };
                let Some(position) = array.len().checked_sub(2) else {
                    return 0;
                };
                if array
                    .get_value_at(position)
                    .and_then(Value::as_long)
                    .map(|v| v as usize)
                    != Some(key | LATE_STATIC_TAG)
                {
                    return 0;
                }
                array
                    .get_value_at(position + 1)
                    .and_then(Value::as_long)
                    .map_or(0, |id| id as u32)
            }
        }
    }

    #[inline]
    pub(crate) fn discard_late_static(&mut self, key: usize) -> bool {
        match self.storage.as_mut() {
            Storage::Stack(records) => {
                if records
                    .last()
                    .is_some_and(|record| record.key == key | LATE_STATIC_TAG)
                {
                    records.pop();
                    true
                } else {
                    false
                }
            }
            Storage::Opaque(value) => {
                let array = value
                    .as_array_mut()
                    .expect("pending call state must remain a packed array");
                let Some(position) = array.len().checked_sub(2) else {
                    return false;
                };
                if array
                    .get_value_at(position)
                    .and_then(Value::as_long)
                    .map(|v| v as usize)
                    != Some(key | LATE_STATIC_TAG)
                {
                    return false;
                }
                array.pop();
                array.pop();
                true
            }
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        match self.storage.as_ref() {
            Storage::Stack(records) => records.is_empty(),
            Storage::Opaque(value) => value.as_array().is_some_and(PhpArray::is_empty),
        }
    }

    pub(crate) fn magic_name(&self, key: usize) -> Option<String> {
        let tagged = key | MAGIC_CALL_TAG;
        match self.storage.as_ref() {
            Storage::Stack(records) => records
                .iter()
                .rev()
                .find(|record| record.key == tagged)
                .and_then(|record| match &record.payload {
                    Payload::Receiver(value) => value.as_str().map(str::to_owned),
                    Payload::LateStatic(_) => None,
                }),
            Storage::Opaque(value) => {
                let array = value.as_array()?;
                let mut position = array.len().checked_sub(2)?;
                loop {
                    if array.get_value_at(position)?.as_long()? as usize == tagged {
                        return array
                            .get_value_at(position + 1)
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                    }
                    position = position.checked_sub(2)?;
                }
            }
        }
    }

    pub(crate) fn for_each_value(&self, visit: &mut dyn FnMut(&Value)) {
        match self.storage.as_ref() {
            Storage::Stack(records) => {
                for record in records {
                    if let Payload::Receiver(value) = &record.payload {
                        visit(value)
                    }
                }
            }
            Storage::Opaque(value) => visit(value),
        }
    }

    /// Inspect a legacy embedding value; engine-created typed stacks return None.
    pub fn opaque_value(&self) -> Option<&Value> {
        match self.storage.as_ref() {
            Storage::Opaque(value) => Some(value),
            Storage::Stack(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LATE_STATIC_TAG, PendingCallState};
    use crate::runtime::ExecutorGlobals;
    use crate::value::{PhpArray, Value};

    #[test]
    fn late_static_discard_preserves_empty_and_unrelated_embedding_state() {
        let mut eg = ExecutorGlobals::new();
        eg.pending_invoke_this = Some(Value::array(PhpArray::new()).into());
        eg.discard_late_static_scope(16);
        assert!(
            eg.pending_invoke_this
                .as_ref()
                .and_then(PendingCallState::opaque_value)
                .and_then(Value::as_array)
                .is_some_and(PhpArray::is_empty)
        );

        let mut array = PhpArray::new();
        array.push(Value::long((32 | LATE_STATIC_TAG) as i64));
        array.push(Value::long(7));
        eg.pending_invoke_this = Some(Value::array(array).into());
        eg.discard_late_static_scope(16);
        assert_eq!(eg.late_static_scope_class_id(32), 7);
        eg.discard_late_static_scope(32);
        assert!(eg.pending_invoke_this.is_none());
    }
}
