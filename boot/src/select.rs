//! Boot decision: combine the partition table with the otadata entries.
//!
//! This is the app-side counterpart of what the IDF bootloader does before it
//! jumps: pick the slot with the highest valid sequence, fall back to `factory`
//! when there is no valid otadata, and never boot when there is no app slot at
//! all.

use crate::otadata;
use crate::otadata::Entry;
use crate::partition::Partition;

/// OTA slot subtypes, indexed by `ota_seq - 1`.
const OTA_SUBTYPES: [&str; 8] = [
    "ota_0", "ota_1", "ota_2", "ota_3", "ota_4", "ota_5", "ota_6", "ota_7",
];

/// Why this target was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Valid otadata pointed at an OTA slot.
    OtaSelected,
    /// No valid otadata (or an out-of-range sequence), so `factory` was chosen.
    FactoryFallback,
    /// No valid otadata and no factory, so the first app partition was chosen.
    FirstAppFallback,
    /// The table has no app partition.
    NoApp,
}

/// What to boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target<'a> {
    /// An OTA slot, with its index (`0` = `ota_0`).
    Slot {
        /// Slot index.
        index: usize,
        /// The partition to boot.
        partition: &'a Partition,
    },
    /// The factory partition.
    Factory(&'a Partition),
    /// Nothing bootable; the caller should enter safe mode.
    Halted,
}

/// Result of [`decide`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision<'a> {
    /// Chosen target.
    pub target: Target<'a>,
    /// Why it was chosen.
    pub reason: Reason,
}

/// Decide what to boot.
///
/// The rules match the IDF bootloader:
/// 1. If an otadata entry is valid, its `seq - 1` names an `ota_<n>` partition.
/// 2. Otherwise boot `factory` if present.
/// 3. Otherwise boot the first app partition.
/// 4. Otherwise halt.
#[must_use]
pub fn decide<'a>(table: &'a [Partition], entries: &[Entry; 2]) -> Decision<'a> {
    if let Some(entry_index) = otadata::select(entries)
        && let Some(slot) = entries[entry_index].slot()
        && let Some(subtype) = OTA_SUBTYPES.get(slot as usize)
        && let Some(partition) = table
            .iter()
            .find(|partition| partition.is_app() && partition.subtype == *subtype)
    {
        return Decision {
            target: Target::Slot {
                index: slot as usize,
                partition,
            },
            reason: Reason::OtaSelected,
        };
    }

    if let Some(factory) = table
        .iter()
        .find(|partition| partition.is_app() && partition.subtype == "factory")
    {
        return Decision {
            target: Target::Factory(factory),
            reason: Reason::FactoryFallback,
        };
    }

    if let Some(first) = table.iter().find(|partition| partition.is_app()) {
        return Decision {
            target: Target::Slot {
                index: 0,
                partition: first,
            },
            reason: Reason::FirstAppFallback,
        };
    }

    Decision {
        target: Target::Halted,
        reason: Reason::NoApp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::otadata::State;
    use crate::partition::PartitionType;

    const fn app(label: &'static str, subtype: &'static str, offset: u64) -> Partition {
        Partition {
            label,
            ty: PartitionType::App,
            subtype,
            offset,
            size: 0x1000,
        }
    }

    const fn data(label: &'static str, offset: u64) -> Partition {
        Partition {
            label,
            ty: PartitionType::Data,
            subtype: "data",
            offset,
            size: 0x1000,
        }
    }

    fn entry(seq: u32) -> Entry {
        Entry {
            seq,
            state: State::Valid,
            crc_ok: true,
        }
    }

    fn broken(seq: u32) -> Entry {
        Entry {
            seq,
            state: State::Valid,
            crc_ok: false,
        }
    }

    static OTA_TABLE: &[Partition] = &[
        data("nvs", 0x9000),
        app("ota_0", "ota_0", 0x20000),
        app("ota_1", "ota_1", 0x420000),
    ];

    static FACTORY_TABLE: &[Partition] = &[
        data("nvs", 0x9000),
        app("factory", "factory", 0x20000),
        app("ota_0", "ota_0", 0x410000),
    ];

    #[test]
    fn selects_the_higher_sequence() {
        let decision = decide(OTA_TABLE, &[entry(1), entry(2)]);
        assert_eq!(decision.reason, Reason::OtaSelected);
        match decision.target {
            Target::Slot { index, partition } => {
                assert_eq!(index, 1);
                assert_eq!(partition.label, "ota_1");
            }
            other => panic!("expected a slot, got {other:?}"),
        }
    }

    #[test]
    fn selects_the_lower_slot_when_it_is_the_only_valid_one() {
        let decision = decide(OTA_TABLE, &[entry(1), broken(2)]);
        assert_eq!(decision.reason, Reason::OtaSelected);
        assert!(matches!(
            decision.target,
            Target::Slot { index: 0, partition } if partition.label == "ota_0"
        ));
    }

    #[test]
    fn falls_back_to_factory_without_valid_otadata() {
        let decision = decide(FACTORY_TABLE, &[broken(1), broken(2)]);
        assert_eq!(decision.reason, Reason::FactoryFallback);
        assert!(matches!(
            decision.target,
            Target::Factory(partition) if partition.label == "factory"
        ));
    }

    #[test]
    fn falls_back_to_the_first_app_when_there_is_no_factory() {
        let decision = decide(OTA_TABLE, &[broken(1), broken(2)]);
        assert_eq!(decision.reason, Reason::FirstAppFallback);
        assert!(matches!(
            decision.target,
            Target::Slot { index: 0, partition } if partition.label == "ota_0"
        ));
    }

    #[test]
    fn out_of_range_sequence_falls_back() {
        let decision = decide(FACTORY_TABLE, &[entry(5), broken(1)]);
        assert_eq!(decision.reason, Reason::FactoryFallback);
    }

    #[test]
    fn no_app_partition_halts() {
        static DATA_ONLY: &[Partition] = &[data("nvs", 0x9000)];
        let decision = decide(DATA_ONLY, &[entry(1), broken(1)]);
        assert_eq!(decision.target, Target::Halted);
        assert_eq!(decision.reason, Reason::NoApp);
    }
}
