//! The gate every tool call passes before a request exists.
//!
//! # Why the order matters more than the checks
//!
//! Each tool declares one fixed [`CallerTier`], and the server is configured with
//! exactly one maximum authority. A call is admitted only if the configured
//! authority reaches the tool's requirement — and that comparison happens
//! **before a [`crate::client::Request`] is built**, let alone sent.
//!
//! That ordering is the entire security property of this crate, and it is why
//! [`Gate::admit`] returns a *token* rather than a boolean. A boolean can be
//! ignored; a token has to be produced, and the only thing that produces one is
//! the check. Every dispatch path in `crate::tools` takes an [`Admitted`] by
//! value, so a tool that skipped the gate does not compile.
//!
//! Relying on the daemon's own `forbidden` instead would be indistinguishable
//! most of the time and wrong in the case that matters: an observer-configured
//! server would still *send* the mutation, and "the write was refused" and "the
//! write was never attempted" are different facts about a control plane. The
//! matrix test asserts the second one by counting what a recording transport
//! received, which is only meaningful because the refusal happens here.
//!
//! # What the gate does not do
//!
//! It does not decide whether a runtime can perform an operation. That is the
//! binding's frozen capability set, it lives in the daemon, and its refusal
//! (`unsupported_capability`) is relayed untouched. A client-side guess at a
//! runtime's capabilities would be exactly the re-grading the freeze rule exists
//! to prevent.

use crate::client::CallerTier;

/// Proof that one tool call cleared the authority gate.
///
/// It carries the tier the call was admitted at, and it is not constructible
/// outside this module: the field is private and [`Gate::admit`] is the only
/// thing that returns one.
#[derive(Debug, Clone, Copy)]
pub struct Admitted {
    tier: CallerTier,
}

impl Admitted {
    /// The authority the call was admitted at.
    #[must_use]
    pub const fn tier(self) -> CallerTier {
        self.tier
    }
}

/// Why a tool call was refused before anything was dispatched.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Denied {
    /// This server does not carry the authority the tool requires.
    #[error(
        "tool `{tool}` requires {required} authority and this server was configured with {configured}"
    )]
    Authority {
        /// The tool that was called.
        tool: String,
        /// What it needs.
        required: CallerTier,
        /// What this server has.
        configured: CallerTier,
    },
    /// The tool does not exist on this server.
    ///
    /// A staged surface — one whose owning seam has not merged — is *absent*
    /// rather than present and failing, so it arrives here. Advertising a tool
    /// that cannot work teaches a caller to trust an answer nobody computed.
    #[error("no tool named `{tool}` is served at {configured} authority")]
    NoSuchTool {
        /// The name that was called.
        tool: String,
        /// This server's authority.
        configured: CallerTier,
    },
    /// The arguments carry a property this schema does not have.
    ///
    /// Refused rather than ignored. An unknown property is either a caller
    /// believing in a parameter that does not exist, or an attempt to smuggle one
    /// past a schema; silently dropping it would make the first invisible and the
    /// second worth trying.
    #[error("tool `{tool}` has no property `{property}`")]
    ForbiddenProperty {
        /// The tool that was called.
        tool: String,
        /// The property that was refused.
        property: String,
    },
    /// A required argument is absent.
    #[error("tool `{tool}` requires the property `{property}`")]
    MissingProperty {
        /// The tool that was called.
        tool: String,
        /// The property that is missing.
        property: String,
    },
    /// An argument is present but not the shape the schema declares.
    #[error("tool `{tool}` property `{property}` must be {expected}")]
    WrongType {
        /// The tool that was called.
        tool: String,
        /// The property that was refused.
        property: String,
        /// What the schema declares.
        expected: &'static str,
    },
    /// An argument is the right shape but not a value the domain accepts.
    ///
    /// Distinct from [`Denied::WrongType`] because the two are different mistakes:
    /// a wrong type is a caller sending a number where a string belongs, and this
    /// is a caller sending a string that is not a canonical identifier, an open key
    /// or a positive revision. Both are refused here, before a request exists, so
    /// the daemon never has to be asked about a value the domain already rejects.
    #[error("tool `{tool}` property `{property}` is not valid: {rule}")]
    WrongTypeDetail {
        /// The tool that was called.
        tool: String,
        /// The property that was refused.
        property: String,
        /// The domain rule that refused it.
        rule: String,
    },
}

/// The one maximum authority a server was configured with.
#[derive(Debug, Clone, Copy)]
pub struct Gate {
    configured: CallerTier,
}

impl Gate {
    /// A gate that admits nothing above `configured`.
    #[must_use]
    pub const fn new(configured: CallerTier) -> Self {
        Self { configured }
    }

    /// The configured maximum authority.
    #[must_use]
    pub const fn configured(self) -> CallerTier {
        self.configured
    }

    /// Admit one tool call, or refuse it.
    ///
    /// # Errors
    /// Returns [`Denied::Authority`] when the configured authority does not reach
    /// `required`.
    pub fn admit(self, tool: &str, required: CallerTier) -> Result<Admitted, Denied> {
        if self.configured.at_least(required) {
            return Ok(Admitted {
                tier: self.configured,
            });
        }
        Err(Denied::Authority {
            tool: tool.to_owned(),
            required,
            configured: self.configured,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gate_admits_its_own_tier_and_everything_below_it() {
        let admin = Gate::new(CallerTier::Admin);
        for required in CallerTier::ALL {
            admin
                .admit("any", *required)
                .expect("admin authority reaches every tier");
        }

        let operator = Gate::new(CallerTier::Operator);
        operator
            .admit("run_launch", CallerTier::Operator)
            .expect("an operator tool");
        operator
            .admit("run_show", CallerTier::Observer)
            .expect("an observer tool");
        assert!(
            matches!(
                operator.admit("account_list", CallerTier::Admin),
                Err(Denied::Authority { .. })
            ),
            "an operator may not reach an admin tool"
        );
    }

    #[test]
    fn an_observer_gate_refuses_every_mutation_and_says_which_authority_was_wanted() {
        let observer = Gate::new(CallerTier::Observer);
        let denied = observer
            .admit("run_launch", CallerTier::Operator)
            .expect_err("an observer may not launch");
        assert_eq!(
            denied,
            Denied::Authority {
                tool: "run_launch".to_owned(),
                required: CallerTier::Operator,
                configured: CallerTier::Observer,
            },
            "the refusal names both authorities, so an operator can see what to reconfigure"
        );
    }

    #[test]
    fn the_admitted_token_reports_the_configured_tier_and_not_the_required_one() {
        // The distinction matters for logging: a call admitted at admin authority
        // was made with the admin secret even when the tool only needed observer.
        let admitted = Gate::new(CallerTier::Admin)
            .admit("run_show", CallerTier::Observer)
            .expect("admin reaches an observer tool");
        assert_eq!(admitted.tier(), CallerTier::Admin);
    }
}
