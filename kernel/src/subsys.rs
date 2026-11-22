//! Subsystem registration and Linux-style initcall levels.
//!
//! Linux boots by walking initcall sections in level order (`early`, `core`,
//! `postcore`, `arch`, `subsys`, `fs`, `device`, `late`). This module reproduces
//! the useful part of that idea without linker sections or function pointers:
//! components register a name and a level, and the boot path asks for the
//! execution order. Subsystems additionally declare a class, which is what
//! diagnostics and the device tree use to group them.
//!
//! The intended boot pathway is documented in [KERNEL.md](../../docs/KERNEL.md):
//!
//! ```text
//! arch init → kernel init → initcalls (by level) → board probes → services → running
//! ```
//!
//! Registration is static and happens before the walk; nothing allocates.

/// Initcall level, in execution order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InitLevel {
    /// Very early: clock, heap, console.
    Early,
    /// Core kernel: scheduler, timers, IRQ dispatch.
    Core,
    /// After core: power, watchdog.
    PostCore,
    /// Arch/SoC: PSRAM, flash, cache.
    Arch,
    /// Subsystems: IPC, storage, net.
    Subsys,
    /// Filesystems and stores.
    Fs,
    /// Device drivers and probes.
    Device,
    /// Last: services, telemetry, UI.
    Late,
}

impl InitLevel {
    /// All levels in execution order.
    pub const ALL: [Self; 8] = [
        Self::Early,
        Self::Core,
        Self::PostCore,
        Self::Arch,
        Self::Subsys,
        Self::Fs,
        Self::Device,
        Self::Late,
    ];

    /// Short name used in boot logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Early => "early",
            Self::Core => "core",
            Self::PostCore => "postcore",
            Self::Arch => "arch",
            Self::Subsys => "subsys",
            Self::Fs => "fs",
            Self::Device => "device",
            Self::Late => "late",
        }
    }
}

/// One registered initcall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitEntry {
    /// Component name, for boot logs.
    pub name: &'static str,
    /// When it runs.
    pub level: InitLevel,
}

/// Why registration or ordering failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitError {
    /// The table has no free slot.
    Capacity,
    /// The output buffer cannot hold the order.
    BufferTooSmall,
}

/// Static registry of initcalls.
#[derive(Clone, Debug)]
pub struct InitTable<const N: usize> {
    entries: [Option<InitEntry>; N],
    count: usize,
}

impl<const N: usize> Default for InitTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> InitTable<N> {
    /// Create an empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [None; N],
            count: 0,
        }
    }

    /// Register an initcall, returning its id.
    ///
    /// # Errors
    /// [`InitError::Capacity`] when the table is full.
    pub fn register(&mut self, name: &'static str, level: InitLevel) -> Result<u8, InitError> {
        if self.count >= N || self.count > usize::from(u8::MAX) {
            return Err(InitError::Capacity);
        }
        self.entries[self.count] = Some(InitEntry { name, level });
        let id = self.count as u8;
        self.count += 1;
        Ok(id)
    }

    /// Number of registered initcalls.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether nothing is registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Number of initcalls at a level.
    #[must_use]
    pub fn count_at(&self, level: InitLevel) -> usize {
        self.entries[..self.count]
            .iter()
            .flatten()
            .filter(|entry| entry.level == level)
            .count()
    }

    /// Write ids in execution order: by level, then registration order.
    ///
    /// # Errors
    /// [`InitError::BufferTooSmall`] when `out` cannot hold every id.
    pub fn order<'a>(&self, out: &'a mut [u8]) -> Result<&'a [u8], InitError> {
        if out.len() < self.count {
            return Err(InitError::BufferTooSmall);
        }
        let mut written = 0;
        for level in InitLevel::ALL {
            for (index, entry) in self.entries[..self.count].iter().enumerate() {
                if entry.is_some_and(|entry| entry.level == level) {
                    out[written] = index as u8;
                    written += 1;
                }
            }
        }
        Ok(&out[..written])
    }

    /// Look up an entry by id.
    #[must_use]
    pub fn get(&self, id: u8) -> Option<&InitEntry> {
        self.entries.get(usize::from(id)).and_then(Option::as_ref)
    }
}

/// Coarse class of a subsystem, used for grouping and diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SubsystemClass {
    /// Kernel core services.
    Core,
    /// Networking (netdev, TCP/IP, MQTT).
    Net,
    /// Storage (flash, EEPROM, KV).
    Storage,
    /// Input devices (buttons, encoders).
    Input,
    /// Cryptography and keys.
    Crypto,
    /// Telemetry and health.
    Diagnostics,
}

impl SubsystemClass {
    /// Short name used in dumps.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Net => "net",
            Self::Storage => "storage",
            Self::Input => "input",
            Self::Crypto => "crypto",
            Self::Diagnostics => "diagnostics",
        }
    }
}

/// A subsystem declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubsystemSpec {
    /// Name, e.g. `net` or `storage`.
    pub name: &'static str,
    /// Coarse class.
    pub class: SubsystemClass,
    /// Initcall level at which it initialises.
    pub level: InitLevel,
}

/// Static registry of subsystems.
#[derive(Clone, Debug)]
pub struct SubsystemTable<const N: usize> {
    specs: [Option<SubsystemSpec>; N],
    count: usize,
}

impl<const N: usize> Default for SubsystemTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> SubsystemTable<N> {
    /// Create an empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            specs: [None; N],
            count: 0,
        }
    }

    /// Register a subsystem.
    ///
    /// # Errors
    /// [`InitError::Capacity`] when the table is full.
    pub fn register(&mut self, spec: SubsystemSpec) -> Result<u8, InitError> {
        if self.count >= N || self.count > usize::from(u8::MAX) {
            return Err(InitError::Capacity);
        }
        self.specs[self.count] = Some(spec);
        let id = self.count as u8;
        self.count += 1;
        Ok(id)
    }

    /// Number of registered subsystems.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether no subsystem is registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Find a subsystem by name.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&SubsystemSpec> {
        self.specs[..self.count]
            .iter()
            .flatten()
            .find(|spec| spec.name == name)
    }

    /// Number of subsystems of a class.
    #[must_use]
    pub fn count_by_class(&self, class: SubsystemClass) -> usize {
        self.specs[..self.count]
            .iter()
            .flatten()
            .filter(|spec| spec.class == class)
            .count()
    }

    /// Iterate declarations in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &SubsystemSpec> {
        self.specs[..self.count].iter().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_by_level_then_registration() {
        let mut table = InitTable::<8>::new();
        let late = table.register("mqtt", InitLevel::Late).unwrap();
        let early = table.register("clock", InitLevel::Early).unwrap();
        let core = table.register("sched", InitLevel::Core).unwrap();
        let device = table.register("i2c", InitLevel::Device).unwrap();

        let mut order = [0u8; 8];
        let order = table.order(&mut order).unwrap();
        assert_eq!(order, &[early, core, device, late]);
    }

    #[test]
    fn preserves_registration_order_within_a_level() {
        let mut table = InitTable::<4>::new();
        let first = table.register("a", InitLevel::Subsys).unwrap();
        let second = table.register("b", InitLevel::Subsys).unwrap();
        let mut order = [0u8; 4];
        let order = table.order(&mut order).unwrap();
        assert_eq!(order, &[first, second]);
    }

    #[test]
    fn reports_capacity_and_buffer_errors() {
        let mut table = InitTable::<1>::new();
        table.register("only", InitLevel::Core).unwrap();
        assert_eq!(
            table.register("second", InitLevel::Core),
            Err(InitError::Capacity)
        );

        let mut order = [0u8; 0];
        assert_eq!(table.order(&mut order), Err(InitError::BufferTooSmall));
    }

    #[test]
    fn counts_per_level_and_looks_up() {
        let mut table = InitTable::<4>::new();
        table.register("clock", InitLevel::Early).unwrap();
        table.register("sched", InitLevel::Core).unwrap();
        table.register("timer", InitLevel::Core).unwrap();
        assert_eq!(table.len(), 3);
        assert_eq!(table.count_at(InitLevel::Core), 2);
        assert_eq!(table.count_at(InitLevel::Late), 0);
        assert_eq!(table.get(0).map(|entry| entry.name), Some("clock"));
        assert_eq!(table.get(9), None);
    }

    #[test]
    fn subsystem_table_groups_by_class() {
        let mut table = SubsystemTable::<4>::new();
        table
            .register(SubsystemSpec {
                name: "net",
                class: SubsystemClass::Net,
                level: InitLevel::Subsys,
            })
            .unwrap();
        table
            .register(SubsystemSpec {
                name: "storage",
                class: SubsystemClass::Storage,
                level: InitLevel::Subsys,
            })
            .unwrap();
        table
            .register(SubsystemSpec {
                name: "mqtt",
                class: SubsystemClass::Net,
                level: InitLevel::Late,
            })
            .unwrap();

        assert_eq!(table.len(), 3);
        assert_eq!(table.count_by_class(SubsystemClass::Net), 2);
        assert_eq!(table.count_by_class(SubsystemClass::Crypto), 0);
        assert_eq!(
            table.find("storage").map(|spec| spec.level),
            Some(InitLevel::Subsys)
        );
        assert_eq!(table.find("missing"), None);
        let names: std::vec::Vec<&str> = table.iter().map(|spec| spec.name).collect();
        assert_eq!(names, ["net", "storage", "mqtt"]);
    }
}
