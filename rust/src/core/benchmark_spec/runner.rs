use super::types::{BenchmarkKind, BenchmarkResult, BenchmarkSpecV1};

pub(crate) trait BenchmarkRunner: Send + Sync {
    fn kind(&self) -> BenchmarkKind;
    fn run(&self, spec: &BenchmarkSpecV1) -> anyhow::Result<BenchmarkResult>;
    fn name(&self) -> &'static str;
}
