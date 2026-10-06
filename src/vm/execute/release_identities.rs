use super::{IdentityMap, IdentitySet};

const INLINE_IDENTITIES: usize = 8;

/// Scratch membership for a release inspection. Identities have no observable
/// iteration order; small walks keep them on the stack and larger walks retain
/// the canonical hash table's bounded lookup cost.
pub(super) enum InspectionIdentitySet {
    Inline {
        identities: [usize; INLINE_IDENTITIES],
        len: u8,
    },
    Spilled(IdentitySet),
}

impl Default for InspectionIdentitySet {
    fn default() -> Self {
        Self::Inline {
            identities: [0; INLINE_IDENTITIES],
            len: 0,
        }
    }
}

impl InspectionIdentitySet {
    #[inline]
    pub(super) fn insert(&mut self, identity: usize) -> bool {
        match self {
            Self::Inline { identities, len } => {
                if identities[..usize::from(*len)].contains(&identity) {
                    return false;
                }
                if usize::from(*len) < INLINE_IDENTITIES {
                    identities[usize::from(*len)] = identity;
                    *len += 1;
                    true
                } else {
                    self.spill_and_insert(identity)
                }
            }
            Self::Spilled(identities) => identities.insert(identity),
        }
    }

    #[cold]
    #[inline(never)]
    fn spill_and_insert(&mut self, identity: usize) -> bool {
        let Self::Inline { identities, .. } = self else {
            unreachable!("only a full inline identity set spills");
        };
        let mut spilled =
            IdentitySet::with_capacity_and_hasher(INLINE_IDENTITIES * 2, Default::default());
        spilled.extend(*identities);
        let inserted = spilled.insert(identity);
        *self = Self::Spilled(spilled);
        inserted
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Inline { len, .. } => usize::from(*len),
            Self::Spilled(identities) => identities.len(),
        }
    }
}

/// Encounter and opaque-snapshot counts use the same identities as membership.
/// A zero count is still an existing entry, just as in the canonical map.
pub(super) enum InspectionIdentityCounts {
    Inline {
        identities: [usize; INLINE_IDENTITIES],
        counts: [usize; INLINE_IDENTITIES],
        len: u8,
    },
    Spilled(IdentityMap),
}

impl Default for InspectionIdentityCounts {
    fn default() -> Self {
        Self::Inline {
            identities: [0; INLINE_IDENTITIES],
            counts: [0; INLINE_IDENTITIES],
            len: 0,
        }
    }
}

impl InspectionIdentityCounts {
    #[inline]
    pub(super) fn get_or_insert_zero(&mut self, identity: usize) -> &mut usize {
        // Determine the inline position before taking the returned mutable
        // borrow, so a spill can replace the representation using safe Rust.
        let position = match self {
            Self::Inline {
                identities, len, ..
            } => Some(
                identities[..usize::from(*len)]
                    .iter()
                    .position(|stored| *stored == identity)
                    .unwrap_or(usize::from(*len)),
            ),
            Self::Spilled(_) => None,
        };
        if position == Some(INLINE_IDENTITIES) {
            self.spill();
        }
        match self {
            Self::Inline {
                identities,
                counts,
                len,
            } => {
                let position = position.expect("inline count position");
                if position == usize::from(*len) {
                    identities[position] = identity;
                    *len += 1;
                }
                &mut counts[position]
            }
            Self::Spilled(counts) => counts.entry(identity).or_insert(0),
        }
    }

    #[inline]
    pub(super) fn get(&self, identity: &usize) -> Option<&usize> {
        match self {
            Self::Inline {
                identities,
                counts,
                len,
            } => identities[..usize::from(*len)]
                .iter()
                .position(|stored| stored == identity)
                .map(|position| &counts[position]),
            Self::Spilled(counts) => counts.get(identity),
        }
    }

    #[inline]
    pub(super) fn get_mut(&mut self, identity: &usize) -> Option<&mut usize> {
        match self {
            Self::Inline {
                identities,
                counts,
                len,
            } => identities[..usize::from(*len)]
                .iter()
                .position(|stored| stored == identity)
                .map(|position| &mut counts[position]),
            Self::Spilled(counts) => counts.get_mut(identity),
        }
    }

    #[cold]
    #[inline(never)]
    fn spill(&mut self) {
        let Self::Inline {
            identities, counts, ..
        } = self
        else {
            unreachable!("only a full inline identity count map spills");
        };
        let mut spilled =
            IdentityMap::with_capacity_and_hasher(INLINE_IDENTITIES * 2, Default::default());
        spilled.extend(identities.iter().copied().zip(counts.iter().copied()));
        *self = Self::Spilled(spilled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_preserves_duplicates_across_the_spill_boundary() {
        let mut scratch = InspectionIdentitySet::default();
        let mut canonical = IdentitySet::default();
        for identity in 0..INLINE_IDENTITIES {
            assert_eq!(scratch.insert(identity), canonical.insert(identity));
            assert_eq!(scratch.insert(identity), canonical.insert(identity));
        }
        assert!(matches!(scratch, InspectionIdentitySet::Inline { .. }));
        for identity in [usize::MAX, 0, 7, 8, usize::MAX] {
            assert_eq!(scratch.insert(identity), canonical.insert(identity));
        }
        assert!(matches!(scratch, InspectionIdentitySet::Spilled(_)));
        for identity in (0..2048).map(|n| (n % 311) * 64) {
            assert_eq!(scratch.insert(identity), canonical.insert(identity));
        }
        assert_eq!(scratch.len(), canonical.len());
    }

    #[test]
    fn encounter_and_snapshot_counts_survive_spill_and_zero_counts() {
        let mut scratch = InspectionIdentityCounts::default();
        let mut canonical = IdentityMap::default();
        for identity in 0..INLINE_IDENTITIES {
            *scratch.get_or_insert_zero(identity) += identity + 1;
            *canonical.entry(identity).or_insert(0) += identity + 1;
        }
        assert!(matches!(scratch, InspectionIdentityCounts::Inline { .. }));
        *scratch.get_mut(&3).unwrap() = 0;
        *canonical.get_mut(&3).unwrap() = 0;
        assert_eq!(scratch.get(&3), Some(&0));
        assert_eq!(scratch.get(&usize::MAX), None);
        *scratch.get_or_insert_zero(usize::MAX) += 2;
        *canonical.entry(usize::MAX).or_insert(0) += 2;
        assert!(matches!(scratch, InspectionIdentityCounts::Spilled(_)));
        for identity in (0..2048).map(|n| (n % 311) * 64) {
            *scratch.get_or_insert_zero(identity) += 1;
            *canonical.entry(identity).or_insert(0) += 1;
            if identity.is_multiple_of(128) {
                let observed = scratch.get_mut(&identity).unwrap();
                *observed = observed.saturating_sub(1);
                let expected = canonical.get_mut(&identity).unwrap();
                *expected = expected.saturating_sub(1);
            }
        }
        for (identity, count) in canonical {
            assert_eq!(scratch.get(&identity), Some(&count));
        }
    }
}
