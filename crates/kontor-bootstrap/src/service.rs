//! The per-user service port and its fake manager.
//!
//! A real installation eventually asks a per-user service manager to keep the
//! daemon alive: a launch agent on macOS, a user unit on Linux. This unit
//! defines only the port and a fake that records what it was asked to do. No
//! production implementation exists here, so no host service manager, installer
//! or subprocess is ever started by bootstrap in this unit.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The per-user service families the port speaks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServicePlatform {
    /// A macOS per-user launch agent.
    Launchd,
    /// A Linux per-user systemd unit.
    SystemdUser,
}

impl ServicePlatform {
    /// The stable lowercase name used in receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ServicePlatform::Launchd => "launchd",
            ServicePlatform::SystemdUser => "systemd-user",
        }
    }
}

/// One durable service a manager would be asked to install.
///
/// The plan carries the program and arguments because a manager needs them;
/// nothing here is serialized into a receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServicePlan {
    /// Which per-user service family is being addressed.
    pub platform: ServicePlatform,
    /// The stable service identity, for example `kontor-daemon`.
    pub identity: String,
    /// The installed daemon executable.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
}

/// What a service manager observes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    /// No such service is installed.
    Absent,
    /// The service is installed but not necessarily running.
    Installed,
    /// The service is installed and running.
    Running,
}

impl ServiceState {
    /// The stable lowercase name used in receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ServiceState::Absent => "absent",
            ServiceState::Installed => "installed",
            ServiceState::Running => "running",
        }
    }
}

/// The redacted service outcome of one bootstrap run: identity, platform and
/// state only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceReceipt {
    /// Which platform was addressed.
    pub platform: ServicePlatform,
    /// The logical service identity.
    pub identity: String,
    /// The observed state after the run.
    pub state: ServiceState,
}

/// Why a service manager refused an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ServiceError {
    /// This platform has no supported per-user service family here.
    #[error("this platform has no supported per-user service family")]
    UnsupportedPlatform,
    /// The plan has no stable identity.
    #[error("the service plan has no identity")]
    InvalidPlan,
    /// No service with that identity is installed.
    #[error("no service with that identity is installed")]
    UnknownIdentity,
}

/// One recorded call a fake manager received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceCall {
    /// An install was requested for one identity.
    Install {
        /// The logical identity.
        identity: String,
    },
    /// A restart was requested for one identity.
    Restart {
        /// The logical identity.
        identity: String,
    },
    /// An uninstall was requested for one identity.
    Uninstall {
        /// The logical identity.
        identity: String,
    },
    /// A readback was requested for one identity.
    Readback {
        /// The logical identity.
        identity: String,
    },
}

/// The per-user service port.
pub trait UserServiceManager {
    /// Install or confirm one durable service.
    ///
    /// # Errors
    /// An unsupported platform or an unusable plan.
    fn install(&mut self, plan: &ServicePlan) -> Result<ServiceState, ServiceError>;

    /// Restart one existing service.
    ///
    /// # Errors
    /// An unsupported platform or an unknown identity.
    fn restart(&mut self, identity: &str) -> Result<ServiceState, ServiceError>;

    /// Remove one existing service.
    ///
    /// # Errors
    /// An unsupported platform or an unknown identity.
    fn uninstall(&mut self, identity: &str) -> Result<ServiceState, ServiceError>;

    /// Read one service's current state.
    ///
    /// # Errors
    /// An unsupported platform.
    fn readback(&mut self, identity: &str) -> Result<ServiceState, ServiceError>;
}

/// A manager for a platform with no supported per-user service family.
///
/// It answers every operation with a typed refusal rather than pretending
/// durability; it performs no operation.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedServiceManager;

impl UserServiceManager for UnsupportedServiceManager {
    fn install(&mut self, _plan: &ServicePlan) -> Result<ServiceState, ServiceError> {
        Err(ServiceError::UnsupportedPlatform)
    }

    fn restart(&mut self, _identity: &str) -> Result<ServiceState, ServiceError> {
        Err(ServiceError::UnsupportedPlatform)
    }

    fn uninstall(&mut self, _identity: &str) -> Result<ServiceState, ServiceError> {
        Err(ServiceError::UnsupportedPlatform)
    }

    fn readback(&mut self, _identity: &str) -> Result<ServiceState, ServiceError> {
        Err(ServiceError::UnsupportedPlatform)
    }
}

/// A fake manager that records every call and holds state in memory.
#[derive(Debug, Default, Clone)]
pub struct FakeServiceManager {
    services: BTreeMap<String, ServiceState>,
    calls: Vec<ServiceCall>,
}

impl FakeServiceManager {
    /// An empty fake with no installed services.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every call this fake received, in order.
    #[must_use]
    pub fn calls(&self) -> &[ServiceCall] {
        &self.calls
    }

    /// The state this fake currently records for an identity.
    #[must_use]
    pub fn state(&self, identity: &str) -> ServiceState {
        self.services
            .get(identity)
            .copied()
            .unwrap_or(ServiceState::Absent)
    }
}

impl UserServiceManager for FakeServiceManager {
    fn install(&mut self, plan: &ServicePlan) -> Result<ServiceState, ServiceError> {
        if plan.identity.trim().is_empty() {
            return Err(ServiceError::InvalidPlan);
        }
        self.calls.push(ServiceCall::Install {
            identity: plan.identity.clone(),
        });
        let state = self
            .services
            .entry(plan.identity.clone())
            .or_insert(ServiceState::Installed);
        Ok(*state)
    }

    fn restart(&mut self, identity: &str) -> Result<ServiceState, ServiceError> {
        self.calls.push(ServiceCall::Restart {
            identity: identity.to_owned(),
        });
        let state = self
            .services
            .get_mut(identity)
            .ok_or(ServiceError::UnknownIdentity)?;
        *state = ServiceState::Running;
        Ok(*state)
    }

    fn uninstall(&mut self, identity: &str) -> Result<ServiceState, ServiceError> {
        self.calls.push(ServiceCall::Uninstall {
            identity: identity.to_owned(),
        });
        self.services
            .remove(identity)
            .ok_or(ServiceError::UnknownIdentity)?;
        Ok(ServiceState::Absent)
    }

    fn readback(&mut self, identity: &str) -> Result<ServiceState, ServiceError> {
        self.calls.push(ServiceCall::Readback {
            identity: identity.to_owned(),
        });
        Ok(self.state(identity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(identity: &str) -> ServicePlan {
        ServicePlan {
            platform: ServicePlatform::Launchd,
            identity: identity.to_owned(),
            program: PathBuf::from("/opt/kontor/kontord"),
            args: vec!["--state-root".to_owned(), "/synthetic/root".to_owned()],
        }
    }

    #[test]
    fn the_fake_records_every_call_and_never_executes_anything() {
        let mut manager = FakeServiceManager::new();
        assert_eq!(manager.state("kontor-daemon"), ServiceState::Absent);
        assert_eq!(
            manager.install(&plan("kontor-daemon")),
            Ok(ServiceState::Installed)
        );
        assert_eq!(manager.restart("kontor-daemon"), Ok(ServiceState::Running));
        assert_eq!(manager.readback("kontor-daemon"), Ok(ServiceState::Running));
        assert_eq!(manager.uninstall("kontor-daemon"), Ok(ServiceState::Absent));
        assert_eq!(manager.calls().len(), 4);
        assert_eq!(
            manager.restart("missing"),
            Err(ServiceError::UnknownIdentity)
        );
        assert_eq!(manager.install(&plan("")), Err(ServiceError::InvalidPlan));
    }

    #[test]
    fn an_unsupported_platform_is_a_typed_refusal() {
        let mut manager = UnsupportedServiceManager;
        assert_eq!(
            manager.install(&plan("kontor-daemon")),
            Err(ServiceError::UnsupportedPlatform)
        );
        assert_eq!(
            manager.readback("kontor-daemon"),
            Err(ServiceError::UnsupportedPlatform)
        );
    }

    #[test]
    fn the_port_module_performs_no_host_service_operation() {
        let source = include_str!("service.rs");
        for forbidden in [
            concat!("std::", "process"),
            concat!("Command", "::new"),
            concat!("launch", "ctl"),
            concat!("system", "ctl"),
            concat!("systemd", "-run"),
        ] {
            assert!(
                !source.contains(forbidden),
                "service port must not reference {forbidden}"
            );
        }
    }

    #[test]
    fn the_receipt_carries_no_program_or_arguments() {
        let receipt = ServiceReceipt {
            platform: ServicePlatform::SystemdUser,
            identity: "kontor-daemon".to_owned(),
            state: ServiceState::Running,
        };
        let document = format!("{receipt:?}");
        assert!(!document.contains("/opt/kontor"));
        assert!(!document.contains("--state-root"));
    }
}
