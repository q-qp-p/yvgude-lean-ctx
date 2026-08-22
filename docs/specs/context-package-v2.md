# Research — .ctxpkg v2

> **Draft research, not a released package contract.** This file does not define
> an installable marketplace format, a hosted registry, a graph-native public
> API, or a LeanCTX ecosystem promise.

## Current status

LeanCTX has a signed local .ctxpkg substrate. First-class Context Kit semantics,
portable package identity, pinning, signer behavior, and the public
ContextKitV1 contract are still **Research**. The local substrate must not be
represented as a public publisher service or as a general package manager.

## Research direction

Earlier work explored a richer package shape for reusable context assets,
including versioned manifests, integrity metadata, provenance, optional graph
relationships, and local verification. These are useful design questions, not
available behavior.

Any future contract must:

- preserve local-first use without an account or hosted dependency;
- define version, digest, provenance, and access boundaries explicitly;
- make recovery and verification behavior inspectable;
- earn compatibility, security, and support ownership before release;
- avoid turning Context Kits into an unsupported marketplace or agent platform.

## Out of scope until promoted

The following are not current LeanCTX product claims: a hosted registry,
publisher accounts, package billing, marketplace rankings, social discovery,
cross-vendor capability distribution, autonomous graph learning, or automatic
installation of third-party agent capabilities.

The canonical status map is [Product Architecture](../internal/vision/PRODUCT-ARCHITECTURE.md).
