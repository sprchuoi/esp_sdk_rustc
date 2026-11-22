//! Service lifecycle management.
//!
//! A TinyRTOS product is a set of services (WiFi, MQTT, storage, telemetry…)
//! with start order, dependencies and a restart policy. This module is the pure
//! state machine behind that: registration, dependency-ordered startup, failure
//! propagation and restart backoff. It does not poll the executor; the board's
//! supervisor drives transitions and performs the actual `start` call.
//!
//! The model mirrors what `systemd`/`runit` give a Linux system, reduced to what
//! a 512 KiB MCU can afford: no unit files, no sockets, no allocation.

/// Maximum number of services a manager can hold.
pub const MAX_SERVICES: usize = 16;

/// Monotonic identifier of a registered service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceId(u8);

impl ServiceId {
    /// Construct from a raw index (used by generated tables).
    #[must_use]
    pub const fn new(raw: u8) -> Self {
        Self(raw)
    }

    /// Raw index.
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// What to do when a service fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Never restart.
    Never,
    /// Restart up to `max_restarts` times.
    OnFailure {
        /// Maximum restart attempts before giving up.
        max_restarts: u8,
    },
    /// Always restart (rate-limited by the backoff table).
    Always,
}

/// Lifecycle state of a service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceState {
    /// Registered, not started.
    Idle,
    /// `start()` is running.
    Starting,
    /// Running normally.
    Running,
    /// `stop()` is running.
    Stopping,
    /// Stopped cleanly.
    Stopped,
    /// Failed; may restart depending on policy.
    Failed,
}

/// Static description of a service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceSpec {
    /// Name used in logs and dumps.
    pub name: &'static str,
    /// Services that must be running before this one starts.
    pub dependencies: &'static [ServiceId],
    /// Failure policy.
    pub restart: RestartPolicy,
}

/// Why a manager operation was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceError {
    /// No room for another service.
    Capacity,
    /// The name is already registered.
    DuplicateName,
    /// A dependency id is not registered (or is out of range).
    UnknownDependency,
    /// The dependency graph contains a cycle.
    DependencyCycle,
    /// The provided output buffer is smaller than the service count.
    BufferTooSmall,
    /// The transition is not legal from the current state.
    InvalidTransition,
}

/// Result of a failure notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartDecision {
    /// Restart after `delay_ms`.
    Restart {
        /// Backoff delay in milliseconds.
        delay_ms: u64,
    },
    /// Do not restart.
    Stop,
}

/// Backoff for successive restarts; the last value repeats.
const BACKOFF_MS: [u64; 5] = [0, 100, 500, 2_000, 5_000];

/// Service registry and lifecycle state machine.
#[derive(Clone, Debug)]
pub struct ServiceManager<const N: usize> {
    specs: [Option<ServiceSpec>; N],
    states: [ServiceState; N],
    restarts: [u8; N],
    count: usize,
}

impl<const N: usize> Default for ServiceManager<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ServiceManager<N> {
    /// Create an empty manager.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            specs: [None; N],
            states: [ServiceState::Idle; N],
            restarts: [0; N],
            count: 0,
        }
    }

    /// Number of registered services.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether no service is registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Register a service. Dependencies must already be registered.
    ///
    /// # Errors
    /// [`ServiceError::Capacity`] when the table is full,
    /// [`ServiceError::DuplicateName`] for a repeated name, or
    /// [`ServiceError::UnknownDependency`] for an unregistered dependency.
    pub fn register(&mut self, spec: ServiceSpec) -> Result<ServiceId, ServiceError> {
        if spec.name.is_empty() {
            return Err(ServiceError::UnknownDependency);
        }
        if self.count >= N {
            return Err(ServiceError::Capacity);
        }
        if self.specs[..self.count]
            .iter()
            .flatten()
            .any(|existing| existing.name == spec.name)
        {
            return Err(ServiceError::DuplicateName);
        }
        for dependency in spec.dependencies {
            if usize::from(dependency.0) >= self.count {
                return Err(ServiceError::UnknownDependency);
            }
        }

        let id: ServiceId = ServiceId::new(self.count as u8);
        self.specs[self.count] = Some(spec);
        self.states[self.count] = ServiceState::Idle;
        self.count += 1;
        Ok(id)
    }

    /// Description of a service.
    #[must_use]
    pub fn spec(&self, id: ServiceId) -> Option<&ServiceSpec> {
        self.specs.get(usize::from(id.0)).and_then(Option::as_ref)
    }

    /// Current state of a service.
    #[must_use]
    pub fn state(&self, id: ServiceId) -> Option<ServiceState> {
        if usize::from(id.0) >= self.count {
            return None;
        }
        Some(self.states[usize::from(id.0)])
    }

    /// Start order: every service after its dependencies, stable by id.
    ///
    /// Writes ids into `out` and returns the used prefix.
    ///
    /// # Errors
    /// [`ServiceError::BufferTooSmall`] or [`ServiceError::DependencyCycle`].
    pub fn boot_order<'a>(&self, out: &'a mut [u8]) -> Result<&'a [u8], ServiceError> {
        if out.len() < self.count {
            return Err(ServiceError::BufferTooSmall);
        }
        let mut degree = [0u8; N];
        let mut emitted = [false; N];
        for (index, spec) in self.specs.iter().enumerate() {
            if let Some(spec) = spec {
                degree[index] = spec.dependencies.len() as u8;
            }
        }

        let mut written = 0;
        for _ in 0..self.count {
            let mut picked = None;
            for index in 0..self.count {
                if !emitted[index] && degree[index] == 0 {
                    picked = Some(index);
                    break;
                }
            }
            let Some(index) = picked else {
                return Err(ServiceError::DependencyCycle);
            };
            emitted[index] = true;
            out[written] = index as u8;
            written += 1;

            for (other, spec) in self.specs.iter().enumerate() {
                if emitted[other] {
                    continue;
                }
                if let Some(spec) = spec
                    && spec.dependencies.contains(&ServiceId::new(index as u8))
                {
                    degree[other] = degree[other].saturating_sub(1);
                }
            }
        }
        Ok(&out[..written])
    }

    /// Begin starting a service.
    ///
    /// # Errors
    /// [`ServiceError::InvalidTransition`] if it is already starting or running.
    pub fn begin_start(&mut self, id: ServiceId) -> Result<(), ServiceError> {
        match self.state(id) {
            Some(ServiceState::Idle | ServiceState::Stopped | ServiceState::Failed) => {
                self.states[usize::from(id.0)] = ServiceState::Starting;
                Ok(())
            }
            _ => Err(ServiceError::InvalidTransition),
        }
    }

    /// Mark a starting service as running.
    pub fn mark_started(&mut self, id: ServiceId) -> Result<(), ServiceError> {
        if self.state(id) != Some(ServiceState::Starting) {
            return Err(ServiceError::InvalidTransition);
        }
        self.states[usize::from(id.0)] = ServiceState::Running;
        Ok(())
    }

    /// Begin stopping a service.
    pub fn begin_stop(&mut self, id: ServiceId) -> Result<(), ServiceError> {
        match self.state(id) {
            Some(ServiceState::Running) => {
                self.states[usize::from(id.0)] = ServiceState::Stopping;
                Ok(())
            }
            _ => Err(ServiceError::InvalidTransition),
        }
    }

    /// Mark a stopping service as stopped.
    pub fn mark_stopped(&mut self, id: ServiceId) -> Result<(), ServiceError> {
        if self.state(id) != Some(ServiceState::Stopping) {
            return Err(ServiceError::InvalidTransition);
        }
        self.states[usize::from(id.0)] = ServiceState::Stopped;
        Ok(())
    }

    /// Report that a service failed, and decide whether to restart it.
    ///
    /// Failed dependants are marked failed as well: a service whose dependency
    /// is gone cannot run.
    pub fn mark_failed(&mut self, id: ServiceId) -> RestartDecision {
        if self.state(id).is_none() {
            return RestartDecision::Stop;
        }
        let index = usize::from(id.0);
        self.states[index] = ServiceState::Failed;
        self.restarts[index] = self.restarts[index].saturating_add(1);
        let restarts = self.restarts[index];

        self.fail_dependants(id);

        let Some(spec) = self.spec(id) else {
            return RestartDecision::Stop;
        };
        let allowed = match spec.restart {
            RestartPolicy::Never => false,
            RestartPolicy::OnFailure { max_restarts } => restarts <= max_restarts,
            RestartPolicy::Always => true,
        };
        if !allowed {
            return RestartDecision::Stop;
        }
        let backoff = (usize::from(restarts) - 1).min(BACKOFF_MS.len() - 1);
        RestartDecision::Restart {
            delay_ms: BACKOFF_MS[backoff],
        }
    }

    /// Number of restart attempts recorded for a service.
    #[must_use]
    pub fn restart_count(&self, id: ServiceId) -> u8 {
        self.restarts.get(usize::from(id.0)).copied().unwrap_or(0)
    }

    /// Number of services currently in the failed state.
    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.states[..self.count]
            .iter()
            .filter(|state| **state == ServiceState::Failed)
            .count()
    }

    fn fail_dependants(&mut self, failed: ServiceId) {
        for index in 0..self.count {
            if self.states[index] == ServiceState::Failed {
                continue;
            }
            let depends = self.specs[index].is_some_and(|spec| spec.dependencies.contains(&failed));
            if depends {
                self.states[index] = ServiceState::Failed;
                self.fail_dependants(ServiceId::new(index as u8));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEP_STORAGE: &[ServiceId] = &[ServiceId::new(0)];
    const DEP_NET: &[ServiceId] = &[ServiceId::new(1)];
    const DEP_STORAGE_MQTT: &[ServiceId] = &[ServiceId::new(0), ServiceId::new(2)];
    const DEP_MISSING: &[ServiceId] = &[ServiceId::new(9)];

    fn spec(name: &'static str, deps: &'static [ServiceId], restart: RestartPolicy) -> ServiceSpec {
        ServiceSpec {
            name,
            dependencies: deps,
            restart,
        }
    }

    #[test]
    fn registers_services_in_order() {
        let mut manager = ServiceManager::<8>::new();
        let storage = manager
            .register(spec("storage", &[], RestartPolicy::Never))
            .unwrap();
        let net = manager
            .register(spec(
                "net",
                DEP_STORAGE,
                RestartPolicy::OnFailure { max_restarts: 3 },
            ))
            .unwrap();
        assert_eq!(manager.len(), 2);
        assert_eq!(manager.state(storage), Some(ServiceState::Idle));
        assert_eq!(manager.spec(net).unwrap().name, "net");
    }

    #[test]
    fn rejects_duplicate_and_unknown_dependencies() {
        let mut manager = ServiceManager::<8>::new();
        manager
            .register(spec("a", &[], RestartPolicy::Never))
            .unwrap();
        assert_eq!(
            manager.register(spec("a", &[], RestartPolicy::Never)),
            Err(ServiceError::DuplicateName)
        );
        assert_eq!(
            manager.register(spec("b", DEP_MISSING, RestartPolicy::Never)),
            Err(ServiceError::UnknownDependency)
        );
    }

    #[test]
    fn rejects_capacity_overflow() {
        let mut manager = ServiceManager::<1>::new();
        manager
            .register(spec("a", &[], RestartPolicy::Never))
            .unwrap();
        assert_eq!(
            manager.register(spec("b", &[], RestartPolicy::Never)),
            Err(ServiceError::Capacity)
        );
    }

    #[test]
    fn boot_order_puts_dependencies_first() {
        let mut manager = ServiceManager::<8>::new();
        let storage = manager
            .register(spec("storage", &[], RestartPolicy::Never))
            .unwrap();
        let net = manager
            .register(spec("net", DEP_STORAGE, RestartPolicy::Never))
            .unwrap();
        let mqtt = manager
            .register(spec("mqtt", DEP_NET, RestartPolicy::Never))
            .unwrap();
        let telemetry = manager
            .register(spec("telemetry", DEP_STORAGE_MQTT, RestartPolicy::Never))
            .unwrap();

        let mut order = [0u8; 8];
        let order = manager.boot_order(&mut order).unwrap();
        assert_eq!(
            order,
            &[storage.raw(), net.raw(), mqtt.raw(), telemetry.raw()]
        );
    }

    #[test]
    fn boot_order_requires_enough_space() {
        let mut manager = ServiceManager::<4>::new();
        manager
            .register(spec("a", &[], RestartPolicy::Never))
            .unwrap();
        manager
            .register(spec("b", &[], RestartPolicy::Never))
            .unwrap();
        let mut order = [0u8; 1];
        assert_eq!(
            manager.boot_order(&mut order),
            Err(ServiceError::BufferTooSmall)
        );
    }

    #[test]
    fn state_machine_rejects_invalid_transitions() {
        let mut manager = ServiceManager::<4>::new();
        let service = manager
            .register(spec("svc", &[], RestartPolicy::Never))
            .unwrap();
        assert_eq!(
            manager.mark_started(service),
            Err(ServiceError::InvalidTransition)
        );
        manager.begin_start(service).unwrap();
        assert_eq!(manager.state(service), Some(ServiceState::Starting));
        manager.mark_started(service).unwrap();
        assert_eq!(manager.state(service), Some(ServiceState::Running));
        assert_eq!(
            manager.begin_start(service),
            Err(ServiceError::InvalidTransition)
        );
        manager.begin_stop(service).unwrap();
        manager.mark_stopped(service).unwrap();
        assert_eq!(manager.state(service), Some(ServiceState::Stopped));
    }

    #[test]
    fn restart_policy_backs_off_and_stops_at_the_limit() {
        let mut manager = ServiceManager::<4>::new();
        let service = manager
            .register(spec(
                "svc",
                &[],
                RestartPolicy::OnFailure { max_restarts: 2 },
            ))
            .unwrap();

        assert_eq!(
            manager.mark_failed(service),
            RestartDecision::Restart { delay_ms: 0 }
        );
        assert_eq!(
            manager.mark_failed(service),
            RestartDecision::Restart { delay_ms: 100 }
        );
        assert_eq!(manager.mark_failed(service), RestartDecision::Stop);
        assert_eq!(manager.restart_count(service), 3);
    }

    #[test]
    fn never_policy_never_restarts() {
        let mut manager = ServiceManager::<4>::new();
        let service = manager
            .register(spec("svc", &[], RestartPolicy::Never))
            .unwrap();
        assert_eq!(manager.mark_failed(service), RestartDecision::Stop);
    }

    #[test]
    fn failure_propagates_to_dependants() {
        let mut manager = ServiceManager::<8>::new();
        let storage = manager
            .register(spec("storage", &[], RestartPolicy::Never))
            .unwrap();
        let net = manager
            .register(spec("net", DEP_STORAGE, RestartPolicy::Never))
            .unwrap();
        let mqtt = manager
            .register(spec("mqtt", DEP_NET, RestartPolicy::Never))
            .unwrap();

        manager.mark_failed(storage);
        assert_eq!(manager.state(net), Some(ServiceState::Failed));
        assert_eq!(manager.state(mqtt), Some(ServiceState::Failed));
        assert_eq!(manager.failed_count(), 3);
    }
}
