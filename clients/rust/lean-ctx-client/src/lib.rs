//! # lean-ctx-client
//!
//! A thin, **Preview** Rust client for the declared lean-ctx `/v1` HTTP
//! boundary. It lets a custom integration talk to a running local lean-ctx
//! server without linking the engine.
//!
//! ```no_run
//! use lean_ctx_client::{LeanCtxClient, CallContext};
//! use serde_json::json;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = LeanCtxClient::builder("http://127.0.0.1:7777")
//!     .bearer_token(std::env::var("LEANCTX_TOKEN").unwrap_or_default())
//!     .workspace_id("acme")
//!     .build()?;
//!
//! // Discover what this instance supports before branching on features.
//! let caps = client.capabilities()?;
//! println!("plane = {}", caps["plane"]);
//!
//! // Call any tool over the boundary and read its text.
//! let text = client.call_tool_text(
//!     "ctx_search",
//!     Some(json!({ "pattern": "fn main", "path": "src/" })),
//!     None::<&CallContext>,
//! )?;
//! println!("{text}");
//! # Ok(()) }
//! ```
//!
//! ## What it covers
//!
//! The development routes covered by [`run_conformance`] and the
//! `sdk-conformance` CI job:
//!
//! - `GET /health`, `GET /v1/manifest`, `GET /v1/capabilities`,
//!   `GET /v1/openapi.json`
//! - `GET /v1/tools` (paginated) and `POST /v1/tools/call`
//! - `GET /v1/events` as a blocking [`EventStream`] iterator (SSE)
//! - `GET /v1/context/summary`, `GET /v1/events/search`,
//!   `GET /v1/events/lineage`, `GET /v1/metrics`
//! - Offline, bounded OCLA v1 wire verification for the public canonical token
//!   and agent wire envelopes, plus an explicit self-relay gateway policy check
//!
//! ## SemVer coupling
//!
//! The public facade and session contract are still converging. This crate
//! follows the declared `http_mcp` contract major
//! ([`SUPPORTED_HTTP_CONTRACT_VERSIONS]) for its preview scope; it does not
//! make every local server route a stable public API.
//!
//! All open-ended documents (`manifest`, `capabilities`, `openapi.json`) are
//! returned as [`serde_json::Value`], so adding server keys never breaks a
//! client build. Branch only on fields explicitly declared by the integration
//! contract, not on human-readable messages or incidental server routes.
//!
//! ## Non-goals (the embedding boundary)
//!
//! This crate is deliberately small and decoupled. It is **not** a binding to
//! the engine's internals:
//!
//! - **No engine linkage.** `lean-ctx-client` does not depend on the `lean-ctx`
//!   engine crate. Integration happens over the **process boundary** (HTTP/MCP),
//!   never by linking the whole engine into your binary. Full-crate linking of
//!   the engine is unsupported and out of scope.
//! - **No re-implementation of engine logic.** Compression, indexing, ranking,
//!   and knowledge all live in the server. The client only speaks the wire
//!   contract.
//! - **Stability over surface.** The exported types mirror the versioned
//!   `/v1` contract (and the TypeScript SDK in `cookbook/sdk`). New endpoints
//!   are added deliberately; the engine's internal modules are never re-exported
//!   here.
//! - **Bring your own async.** The client is blocking by design (one small
//!   dependency, no runtime). Call it from a thread or `spawn_blocking` when
//!   embedding in async code.
//!
//! See `docs/contracts/http-mcp-contract-v1.md` and
//! `docs/contracts/capabilities-contract-v1.md` for the authoritative contract.

#![forbid(unsafe_code)]

mod client;
mod conformance;
mod error;
mod events;
mod ocla;
mod tool_text;
mod types;

pub use client::{EventQuery, LeanCtxClient, LeanCtxClientBuilder};
pub use conformance::{
    run_conformance, ConformanceCheck, ConformanceScorecard, COVERED_ROUTES,
    SUPPORTED_HTTP_CONTRACT_VERSIONS,
};
pub use error::{HttpError, LeanCtxError, Result};
pub use events::EventStream;
pub use ocla::{
    decode_agent_envelope, decode_canonical_token_envelope, verify_agent_gateway_admissibility,
    AgentEnvelopeV1, CanonicalTokenEnvelopeV1, OclaGatewayAdmissibilityError, OclaRequestContext,
    OclaWireError, TokenBalanceV1, TokenEnvelopeSurface, TokenFlowDirection,
    AGENT_ENVELOPE_SCHEMA_ID, AGENT_ENVELOPE_SCHEMA_VERSION, CANONICAL_TOKEN_ENVELOPE_SCHEMA_ID,
    CANONICAL_TOKEN_ENVELOPE_SCHEMA_VERSION, MAX_OCLA_WIRE_BYTES, OCLA_API_VERSION,
};
pub use tool_text::tool_result_to_text;
pub use types::{
    CallContext, ContextEventV1, EnvelopePayload, ListToolsResponse, MessageV1, MessagesPayload,
    StreamChunkPayload, ToolCallPayload, ToolCallResponse, UsagePayload,
};
