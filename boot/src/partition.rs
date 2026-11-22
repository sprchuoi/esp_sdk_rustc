//! Partition table model and validation.
//!
//! The generated board tables (Phase 2) emit [`Partition`] values; this module
//! is what makes a bad table a build or boot error instead of a field failure:
//! alignment, overlap, bounds and the presence of at least one app partition are
//! all checked before anything is written.
//!
//! The table mirrors ESP-IDF's: an `app` partition has subtype `factory`,
//! `ota_0`, `ota_1`, …; `data` partitions are `nvs`, `ota`, `phy`, `fat`, …

/// Flash erase-sector size; partitions are aligned to it.
pub const ALIGNMENT: u64 = 0x1000;

/// Partition type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionType {
    /// Executable image slot.
    App,
    /// Data partition.
    Data,
}

/// One partition table entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Partition {
    /// Unique label, e.g. `ota_0`.
    pub label: &'static str,
    /// App or data.
    pub ty: PartitionType,
    /// Subtype, e.g. `ota_0`, `nvs`.
    pub subtype: &'static str,
    /// Byte offset in flash.
    pub offset: u64,
    /// Size in bytes.
    pub size: u64,
}

impl Partition {
    /// One past the last byte of the partition.
    #[must_use]
    pub const fn end(&self) -> u64 {
        self.offset + self.size
    }

    /// Whether this partition is an executable slot.
    #[must_use]
    pub const fn is_app(&self) -> bool {
        matches!(self.ty, PartitionType::App)
    }
}

/// Why a table was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    /// No partitions at all.
    Empty,
    /// A partition has an empty label.
    EmptyLabel,
    /// Two partitions share a label.
    DuplicateLabel,
    /// A partition has zero size.
    ZeroSize,
    /// Offset or size is not a multiple of [`ALIGNMENT`].
    Misaligned,
    /// A partition extends past the end of flash.
    OutOfBounds,
    /// Two partitions overlap.
    Overlap,
    /// No app partition to boot.
    NoApp,
}

/// Validate a table against a flash size.
///
/// # Errors
/// Returns the first [`TableError`] found, in this order: emptiness, empty
/// label, zero size, alignment, bounds, duplicate label, overlap, missing app
/// partition.
pub fn validate(table: &[Partition], flash_size: u64) -> Result<(), TableError> {
    if table.is_empty() {
        return Err(TableError::Empty);
    }

    let mut has_app = false;
    for (index, partition) in table.iter().enumerate() {
        if partition.label.is_empty() {
            return Err(TableError::EmptyLabel);
        }
        if partition.size == 0 {
            return Err(TableError::ZeroSize);
        }
        if !partition.offset.is_multiple_of(ALIGNMENT) || !partition.size.is_multiple_of(ALIGNMENT)
        {
            return Err(TableError::Misaligned);
        }
        let Some(end) = partition.offset.checked_add(partition.size) else {
            return Err(TableError::OutOfBounds);
        };
        if end > flash_size {
            return Err(TableError::OutOfBounds);
        }
        has_app |= partition.is_app();

        for other in &table[index + 1..] {
            if partition.label == other.label {
                return Err(TableError::DuplicateLabel);
            }
            if partition.offset < other.end() && other.offset < end {
                return Err(TableError::Overlap);
            }
        }
    }

    if has_app {
        Ok(())
    } else {
        Err(TableError::NoApp)
    }
}

/// Find a partition by label.
#[must_use]
pub fn find<'a>(table: &'a [Partition], label: &str) -> Option<&'a Partition> {
    table.iter().find(|partition| partition.label == label)
}

/// Iterate app partitions, yielding their slot index (`0` for the first app
/// slot, which is what otadata's `seq - 1` maps to).
pub fn app_slots(table: &[Partition]) -> impl Iterator<Item = (usize, &Partition)> {
    table
        .iter()
        .filter(|partition| partition.is_app())
        .enumerate()
}

#[cfg(test)]
mod tests {
    use super::*;

    static TABLE: &[Partition] = &[
        Partition {
            label: "nvs",
            ty: PartitionType::Data,
            subtype: "nvs",
            offset: 0x9000,
            size: 0x6000,
        },
        Partition {
            label: "otadata",
            ty: PartitionType::Data,
            subtype: "ota",
            offset: 0xf000,
            size: 0x2000,
        },
        Partition {
            label: "ota_0",
            ty: PartitionType::App,
            subtype: "ota_0",
            offset: 0x20000,
            size: 0x400000,
        },
        Partition {
            label: "ota_1",
            ty: PartitionType::App,
            subtype: "ota_1",
            offset: 0x420000,
            size: 0x400000,
        },
    ];

    const FLASH_16M: u64 = 16 * 1024 * 1024;

    fn partition(label: &'static str, offset: u64, size: u64) -> Partition {
        Partition {
            label,
            ty: PartitionType::Data,
            subtype: "data",
            offset,
            size,
        }
    }

    #[test]
    fn accepts_a_real_table() {
        assert_eq!(validate(TABLE, FLASH_16M), Ok(()));
    }

    #[test]
    fn finds_partitions_and_app_slots() {
        assert_eq!(find(TABLE, "otadata").map(|p| p.offset), Some(0xf000));
        assert_eq!(find(TABLE, "missing"), None);
        let slots: std::vec::Vec<&str> = app_slots(TABLE).map(|(_, p)| p.label).collect();
        assert_eq!(slots, ["ota_0", "ota_1"]);
        assert_eq!(app_slots(TABLE).next().map(|(index, _)| index), Some(0));
    }

    #[test]
    fn rejects_empty_table() {
        assert_eq!(validate(&[], FLASH_16M), Err(TableError::Empty));
    }

    #[test]
    fn rejects_duplicate_labels() {
        let table = [
            partition("kv", 0x9000, 0x1000),
            partition("kv", 0xa000, 0x1000),
            Partition {
                label: "app",
                ty: PartitionType::App,
                subtype: "oa_0",
                offset: 0x20000,
                size: 0x1000,
            },
        ];
        assert_eq!(validate(&table, FLASH_16M), Err(TableError::DuplicateLabel));
    }

    #[test]
    fn rejects_misalignment_and_bounds() {
        let table = [Partition {
            label: "app",
            ty: PartitionType::App,
            subtype: "factory",
            offset: 0x20001,
            size: 0x1000,
        }];
        assert_eq!(validate(&table, FLASH_16M), Err(TableError::Misaligned));

        let table = [Partition {
            label: "app",
            ty: PartitionType::App,
            subtype: "factory",
            offset: 0x20000,
            size: 0x1000,
        }];
        assert_eq!(validate(&table, 0x20000), Err(TableError::OutOfBounds));
    }

    #[test]
    fn rejects_overlap() {
        let table = [
            partition("kv", 0x9000, 0x2000),
            partition("log", 0xa000, 0x2000),
            Partition {
                label: "app",
                ty: PartitionType::App,
                subtype: "factory",
                offset: 0x20000,
                size: 0x1000,
            },
        ];
        assert_eq!(validate(&table, FLASH_16M), Err(TableError::Overlap));
    }

    #[test]
    fn rejects_table_without_app() {
        let table = [partition("kv", 0x9000, 0x1000)];
        assert_eq!(validate(&table, FLASH_16M), Err(TableError::NoApp));
    }

    #[test]
    fn rejects_zero_size_and_empty_label() {
        let zero = [partition("kv", 0x9000, 0)];
        assert_eq!(validate(&zero, FLASH_16M), Err(TableError::ZeroSize));

        let empty = [partition("", 0x9000, 0x1000)];
        assert_eq!(validate(&empty, FLASH_16M), Err(TableError::EmptyLabel));
    }
}
