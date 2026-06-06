//! Triad-runtime role-marker impls for the schema-emitted runner nouns.
//!
//! `triad-runtime` 0.2.1 gates `RunnerEngines` (the trait the generated
//! `NexusEngine::execute` drives) on empty role-marker traits — `NexusWork`,
//! `SemaWriteInput`, `SemaReadInput`, `NexusEffectCommand`, and their output
//! companions — so the runner's associated types are constrained to the triad
//! role they fill. The current `schema-rust-next` pin (0.1.12) does not yet emit
//! these marker impls; the newer emitter does (lojix, generated against 0.1.13+,
//! carries `impl triad_runtime::NexusWork for NexusWork {}` and friends inline).
//!
//! This module is the report-77 SOFT bridge: the same empty marker impls,
//! hand-attached to the schema-emitted nouns until the cloud schema artifacts
//! are regenerated against the newer emitter (a coordinated all-contract-crate
//! regeneration). Each impl is on the owning generated noun — the marker says
//! "this enum is the Work / SemaWrite / SemaRead / Effect role", which is a
//! property of that type.

use crate::schema::{nexus, sema};

impl triad_runtime::NexusWork for nexus::NexusWork {}

impl triad_runtime::NexusEffectCommand for nexus::EffectCommand {}

impl triad_runtime::NexusEffectResult for nexus::EffectResult {}

impl triad_runtime::SemaWriteInput for sema::SemaWriteInput {}

impl triad_runtime::SemaWriteOutput for sema::SemaWriteOutput {}

impl triad_runtime::SemaReadInput for sema::SemaReadInput {}

impl triad_runtime::SemaReadOutput for sema::SemaReadOutput {}
