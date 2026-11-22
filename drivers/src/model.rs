//! Static device-tree binding.
//!
//! A [`DeviceNode`] mirrors one node of the board description: its path,
//! `compatible` string, optional register address and children. A
//! [`DriverDesc`] declares what a driver handles. [`bind`] walks the tree in
//! pre-order and resolves each node to the first matching driver.
//!
//! The binding result is only *which driver owns which node*. Constructing the
//! driver stays explicit (a board calls `At24::new(...)`), because a type-erased
//! registry would need allocation or `unsafe` downcasting — both excluded here.

/// One node in the board's device tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceNode {
    /// Full path, e.g. `/soc/i2c0/eeprom@50`.
    pub path: &'static str,
    /// Linux-style compatible string, e.g. `atmel,at24c256`.
    pub compatible: &'static str,
    /// Optional `reg` value: I2C address, SPI chip select, MMIO base.
    pub reg: Option<u64>,
    /// Child nodes (buses and their devices).
    pub children: &'static [Self],
}

impl DeviceNode {
    /// Create a node with children.
    #[must_use]
    pub const fn bus(
        path: &'static str,
        compatible: &'static str,
        reg: Option<u64>,
        children: &'static [Self],
    ) -> Self {
        Self {
            path,
            compatible,
            reg,
            children,
        }
    }

    /// Create a leaf node (no children).
    #[must_use]
    pub const fn leaf(path: &'static str, compatible: &'static str, reg: Option<u64>) -> Self {
        Self {
            path,
            compatible,
            reg,
            children: &[],
        }
    }

    /// Depth-first pre-order walk, calling `visit` for this node and its descendants.
    pub fn walk(&self, visit: &mut impl FnMut(&Self)) {
        visit(self);
        for child in self.children {
            child.walk(visit);
        }
    }
}

/// What a driver handles, for binding purposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverDesc {
    /// Driver name used in diagnostics.
    pub name: &'static str,
    /// Compatible strings claimed by this driver, most specific first.
    pub compatible: &'static [&'static str],
}

impl DriverDesc {
    /// Describe a driver.
    #[must_use]
    pub const fn new(name: &'static str, compatible: &'static [&'static str]) -> Self {
        Self { name, compatible }
    }

    /// Whether this driver claims `node`.
    #[must_use]
    pub fn matches(&self, node: &DeviceNode) -> bool {
        self.compatible.contains(&node.compatible)
    }
}

/// A binding failed before it could complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindError {
    /// The output slice is smaller than the number of nodes.
    BufferTooSmall,
}

/// Resolve one node against the registry, first match wins.
#[must_use]
pub fn resolve<'a>(node: &DeviceNode, drivers: &'a [DriverDesc]) -> Option<&'a DriverDesc> {
    drivers.iter().find(|driver| driver.matches(node))
}

/// Bind every node in the tree, writing one entry per node in pre-order.
///
/// Returns the number of nodes written. A node with no matching driver gets
/// `None` — the caller decides whether that is an error (the build-time checker
/// rejects it for defconfigs).
pub fn bind<'a>(
    root: &DeviceNode,
    drivers: &'a [DriverDesc],
    out: &mut [Option<&'a DriverDesc>],
) -> Result<usize, BindError> {
    let mut written = 0;
    let mut overflow = false;
    root.walk(&mut |node| {
        if written < out.len() {
            out[written] = resolve(node, drivers);
        } else {
            overflow = true;
        }
        written += 1;
    });
    if overflow {
        return Err(BindError::BufferTooSmall);
    }
    Ok(written)
}

/// Count nodes in a tree.
#[must_use]
pub fn node_count(root: &DeviceNode) -> usize {
    let mut count = 0;
    root.walk(&mut |_| count += 1);
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT24: DriverDesc = DriverDesc::new("at24", &["atmel,at24c256", "atmel,at24c32"]);
    const BME280: DriverDesc = DriverDesc::new("bme280", &["bosch,bme280"]);
    const I2C: DriverDesc = DriverDesc::new("esp32s3-i2c", &["espressif,esp32s3-i2c"]);
    const DRIVERS: &[DriverDesc] = &[I2C, AT24, BME280];

    static I2C_CHILDREN: &[DeviceNode] = &[
        DeviceNode::leaf("/soc/i2c0/eeprom@50", "atmel,at24c256", Some(0x50)),
        DeviceNode::leaf("/soc/i2c0/sensor@76", "bosch,bme280", Some(0x76)),
    ];

    static I2C_NODE: DeviceNode = DeviceNode::bus(
        "/soc/i2c0",
        "espressif,esp32s3-i2c",
        Some(0x6000_4000),
        I2C_CHILDREN,
    );

    static ROOT: DeviceNode = DeviceNode::bus("/", "sprchuoi,test-board", None, &[I2C_NODE]);

    #[test]
    fn resolves_known_compatibles() {
        let node = &I2C_CHILDREN[0];
        assert_eq!(resolve(node, DRIVERS).map(|d| d.name), Some("at24"));
        assert_eq!(
            resolve(&I2C_CHILDREN[1], DRIVERS).map(|d| d.name),
            Some("bme280")
        );
        assert_eq!(
            resolve(&I2C_NODE, DRIVERS).map(|d| d.name),
            Some("esp32s3-i2c")
        );
    }

    #[test]
    fn unknown_compatible_has_no_driver() {
        let orphan = DeviceNode::leaf("/soc/spi0/nand@0", "jedec,spi-nand", Some(0));
        assert_eq!(resolve(&orphan, DRIVERS), None);
    }

    #[test]
    fn first_matching_driver_wins() {
        const SHADOW: DriverDesc = DriverDesc::new("shadow", &["atmel,at24c256"]);
        let registry = [SHADOW, AT24];
        let node = &I2C_CHILDREN[0];
        assert_eq!(resolve(node, &registry).map(|d| d.name), Some("shadow"));
    }

    #[test]
    fn bind_walks_in_preorder() {
        let mut out = [None; 4];
        let count = bind(&ROOT, DRIVERS, &mut out).unwrap();
        assert_eq!(count, 4);
        let names: [Option<&str>; 4] = [
            out[0].map(|d| d.name),
            out[1].map(|d| d.name),
            out[2].map(|d| d.name),
            out[3].map(|d| d.name),
        ];
        assert_eq!(
            names,
            [None, Some("esp32s3-i2c"), Some("at24"), Some("bme280")]
        );
    }

    #[test]
    fn bind_reports_a_short_buffer() {
        let mut out = [None; 2];
        assert_eq!(
            bind(&ROOT, DRIVERS, &mut out),
            Err(BindError::BufferTooSmall)
        );
    }

    #[test]
    fn counts_nodes() {
        assert_eq!(node_count(&ROOT), 4);
        assert_eq!(node_count(&I2C_CHILDREN[0]), 1);
    }
}
