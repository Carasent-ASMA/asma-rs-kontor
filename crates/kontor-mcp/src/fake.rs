//! A recording transport, so a test can assert what was *not* sent.
//!
//! The cardinality claim this crate makes — one tool invocation makes exactly one
//! `/v1` request — is only checkable against something that counts. A refusal that
//! happens before dispatch and a refusal that happens at the daemon look identical
//! from the outside; they differ in whether a request exists, and this is what
//! knows.
//!
//! It is compiled into the library rather than a test module because the contract
//! crate drives the same dispatch path from outside, and a fake that lived in
//! `#[cfg(test)]` would have to be written twice.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::client::{CallerTier, FrameBudget, Reply, Request, Transport, TransportFailure};

/// A transport that answers from a script and remembers every request.
#[derive(Debug)]
pub struct RecordingTransport {
    tier: CallerTier,
    seen: Mutex<Vec<Request>>,
    scripted: Mutex<VecDeque<Reply>>,
    /// What to answer once the script is exhausted.
    fallback: Reply,
}

impl RecordingTransport {
    /// A transport at one tier that answers `200 {}` until it is scripted.
    #[must_use]
    pub fn new(tier: CallerTier) -> Self {
        Self {
            tier,
            seen: Mutex::new(Vec::new()),
            scripted: Mutex::new(VecDeque::new()),
            fallback: Reply {
                status: 200,
                body: serde_json::json!({}),
            },
        }
    }

    /// Queue one answer. They are returned in the order they were queued.
    #[must_use]
    pub fn answering(self, status: u16, body: serde_json::Value) -> Self {
        self.scripted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(Reply { status, body });
        self
    }

    /// Every request this transport was asked to make, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<Request> {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// How many requests reached the wire.
    #[must_use]
    pub fn count(&self) -> usize {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// The one request this transport was asked to make.
    ///
    /// # Panics
    /// Panics unless exactly one request was made, which is the assertion nearly
    /// every cardinality test wants to state anyway.
    #[must_use]
    pub fn only_request(&self) -> Request {
        let seen = self.requests();
        assert_eq!(
            seen.len(),
            1,
            "expected exactly one request, and {} were made",
            seen.len()
        );
        seen.into_iter().next().expect("one request")
    }

    /// Record one request and answer it from the script.
    fn respond(&self, request: &Request) -> Reply {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request.clone());
        self.scripted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| self.fallback.clone())
    }
}

#[async_trait]
impl Transport for RecordingTransport {
    fn tier(&self) -> CallerTier {
        self.tier
    }

    fn base_url(&self) -> String {
        "http://127.0.0.1:0".to_owned()
    }

    async fn call(&self, request: &Request) -> Result<Reply, TransportFailure> {
        Ok(self.respond(request))
    }

    async fn frames(
        &self,
        request: &Request,
        _budget: FrameBudget,
    ) -> Result<Reply, TransportFailure> {
        Ok(self.respond(request))
    }
}

/// A transport that never answers, for the "there was nobody there" path.
#[derive(Debug)]
pub struct UnreachableTransport(pub CallerTier);

#[async_trait]
impl Transport for UnreachableTransport {
    fn tier(&self) -> CallerTier {
        self.0
    }

    fn base_url(&self) -> String {
        "http://127.0.0.1:0".to_owned()
    }

    async fn call(&self, _request: &Request) -> Result<Reply, TransportFailure> {
        Err(TransportFailure::Unreachable {
            base_url: self.base_url(),
        })
    }

    async fn frames(
        &self,
        _request: &Request,
        _budget: FrameBudget,
    ) -> Result<Reply, TransportFailure> {
        Err(TransportFailure::Unreachable {
            base_url: self.base_url(),
        })
    }
}
