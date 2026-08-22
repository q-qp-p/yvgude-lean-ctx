# Research — adaptive learning and automated tuning

> **Not a current LeanCTX product promise.** LeanCTX does not offer AutoTune,
> autonomous promotion, scheduled optimization, shared learning services, or a
> managed control plane.

## Current product boundary

The local Runtime makes context behavior inspectable and configurable. An
operator may choose local settings and evaluate a declared workload. That is
manual tuning; it is not a system that automatically searches, selects, or
promotes a winning configuration.

## Research record

The repository contains experiments and implementation ideas involving learned
thresholds, placement, retrieval, session handling, and context-selection
heuristics. They are retained as research material. An implementation module,
environment variable, dashboard entry, or heuristic does not create a stable
API, an evidence-backed benefit, or a public service.

## Promotion requirements

Automated tuning requires an objective, bounded search space, quality
constraints, promotion governance, rollback, an evidence gate, and a released
contract. Until those conditions are met, Calibrator and related automated
selection work remain **Research** under the Product Architecture.

Do not use this document to market self-tuning behavior or to claim an
improvement without a comparable baseline, quality gate, and visible methodology.
