//! Workload: stage-based program definitions with Shuffle/Chain/OneOf
//! strategies. Persistent allocations for warm mode. Fresh clones for
//! cold mode. Defined once, run both ways automatically.
//!
//! A [`Workload`] is a collection of [`Program`]s; each program is a
//! sequence of [`Stage`]s; each stage is a list of [`WorkloadItemKind`]
//! plus a [`StageStrategy`] (`Shuffle`, `Chain`, or `OneOf`) that
//! picks the run order. The harness selects one program per iteration
//! by seed and threads `algo_call` items through to the variant under
//! test, mixing in surrounding work items to model realistic call
//! contexts.

use std::collections::HashMap;

use crate::core::FfiBenchCall;

/// Splitmix64 hash for deterministic data generation.
#[inline(always)]
pub fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58476D1CE4E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D049BB133111EB);
    x ^= x >> 31;
    x
}

// ── Stage execution strategies ──

/// Items run in seed-determined random order.
pub struct Shuffle;
/// Items run in declaration order.
pub struct Chain;
/// One item picked at random per seed, others skipped.
pub struct OneOf;

/// Marker trait for stage strategies.
pub trait StageStrategy {
    /// Given N items and a seed, produce the execution order.
    /// Returns indices into the items array.
    fn order(n: usize, seed: u64) -> Vec<usize>;
}

impl StageStrategy for Shuffle {
    fn order(n: usize, seed: u64) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..n).collect();
        let mut h = seed;
        for i in (1..n).rev() {
            h = mix(h);
            let j = (h as usize) % (i + 1);
            indices.swap(i, j);
        }
        indices
    }
}

impl StageStrategy for Chain {
    fn order(n: usize, _seed: u64) -> Vec<usize> {
        (0..n).collect()
    }
}

impl StageStrategy for OneOf {
    fn order(n: usize, seed: u64) -> Vec<usize> {
        let pick = (mix(seed) as usize) % n;
        vec![pick]
    }
}

// ── Persistent allocation context ──

/// Opaque handle to a persistent allocation.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllocHandle(pub u32);

/// Workload context: provides persistent allocations that survive
/// across stages within a program run. Workload items can read/write
/// shared state (e.g. algorithm output from a previous stage).
pub struct WorkloadCtx {
    slots: HashMap<AllocHandle, Vec<u8>>,
    next_id: u32,
}

impl Default for WorkloadCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkloadCtx {
    pub fn new() -> Self {
        WorkloadCtx { slots: HashMap::new(), next_id: 0 }
    }

    /// Allocate a persistent slot of `size` bytes. Returns a handle.
    pub fn alloc(&mut self, size: usize) -> AllocHandle {
        let id = self.next_id;
        self.next_id += 1;
        let handle = AllocHandle(id);
        self.slots.insert(handle, vec![0u8; size]);
        handle
    }

    /// Get a reference to the slot's data.
    pub fn get(&self, handle: AllocHandle) -> &[u8] {
        self.slots.get(&handle).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get a mutable reference to the slot's data.
    pub fn get_mut(&mut self, handle: AllocHandle) -> &mut [u8] {
        self.slots.get_mut(&handle).map(|v| v.as_mut_slice()).unwrap_or(&mut [])
    }

    /// Reset all slots to zero (for fresh program runs).
    pub fn reset(&mut self) {
        for slot in self.slots.values_mut() {
            slot.fill(0);
        }
    }
}

// ── Workload items ──

/// A single item in a workload stage. Enum dispatch replaces dyn trait
/// objects: no vtable overhead, no heap allocation per item, inlineable.
#[derive(Clone)]
pub enum WorkloadItemKind {
    /// Marker for an algorithm call point. Execution handled by the harness.
    AlgoCall,
    /// Dependent multiply-add chain. Pure ALU, 1 register working set.
    ScalarWork { n: u32 },
    /// Pointer chase through shuffled cache-line nodes. Memory latency.
    GraphWork { n: u32 },
    /// Data-dependent branches (~50% taken). Branch predictor pressure.
    BranchWork { n: u32 },
    /// Sequential scan + second pass. L1 eviction.
    HeavyMemory { u64_count: usize },
    /// Single comparison. Negligible cost. Minimal gap.
    LightScalar,
    /// Domain-specific work item with a captured function pointer.
    /// Used for benchmark-specific surrounding work (e.g. RCM pipeline items).
    DomainWork { run_fn: fn(u64, &mut u64) },
}

impl WorkloadItemKind {
    pub fn is_algo_call(&self) -> bool {
        matches!(self, WorkloadItemKind::AlgoCall)
    }

    pub fn run(&self, seed: u64, accum: &mut u64) {
        match self {
            WorkloadItemKind::AlgoCall => {}
            WorkloadItemKind::ScalarWork { n } => {
                let mut x = seed;
                for _ in 0..*n {
                    x = x.wrapping_mul(0x517cc1b727220a95).wrapping_add(0x6c62272e07bb0142);
                }
                *accum = accum.wrapping_add(x);
            }
            WorkloadItemKind::GraphWork { n } => {
                let n = *n as usize;
                let mut chain: Vec<usize> = (0..n).collect();
                let mut h = seed;
                for i in (1..n).rev() {
                    h = mix(h);
                    let j = (h as usize) % (i + 1);
                    chain.swap(i, j);
                }
                let mut idx = 0;
                let mut acc = 0u64;
                for _ in 0..n {
                    idx = chain[idx % n];
                    acc = acc.wrapping_add(idx as u64);
                }
                *accum = accum.wrapping_add(acc);
            }
            WorkloadItemKind::BranchWork { n } => {
                let mut h = seed;
                let mut acc = 0u64;
                for _ in 0..*n {
                    h = mix(h);
                    if h & 1 == 0 {
                        acc = acc.wrapping_add(h);
                    } else {
                        acc = acc.wrapping_mul(h);
                    }
                }
                *accum = accum.wrapping_add(acc);
            }
            WorkloadItemKind::HeavyMemory { u64_count } => {
                let n = (*u64_count).min(1024);
                let mut buf = [0u64; 1024];
                let mut h = seed;
                for i in 0..n { h = mix(h); buf[i] = h; }
                let mut acc = 0u64;
                for i in 0..n { acc = acc.wrapping_add(buf[i]); }
                for i in 0..n { acc = acc.wrapping_add(buf[i].wrapping_mul(3)); }
                *accum = accum.wrapping_add(acc);
            }
            WorkloadItemKind::LightScalar => {
                *accum = mix(*accum ^ seed);
            }
            WorkloadItemKind::DomainWork { run_fn } => {
                run_fn(seed, accum);
            }
        }
    }
}

// ── Convenience constructors ──

pub fn algo_call() -> WorkloadItemKind { WorkloadItemKind::AlgoCall }
pub fn scalar_work(n: u32) -> WorkloadItemKind { WorkloadItemKind::ScalarWork { n } }
pub fn graph_work(n: u32) -> WorkloadItemKind { WorkloadItemKind::GraphWork { n } }
pub fn branch_work(n: u32) -> WorkloadItemKind { WorkloadItemKind::BranchWork { n } }
pub fn heavy_memory(u64_count: usize) -> WorkloadItemKind { WorkloadItemKind::HeavyMemory { u64_count } }
pub fn light_scalar() -> WorkloadItemKind { WorkloadItemKind::LightScalar }
pub fn domain_work(run_fn: fn(u64, &mut u64)) -> WorkloadItemKind { WorkloadItemKind::DomainWork { run_fn } }

// ── Stage ──

/// A resolved stage: items + strategy order function.
pub struct Stage {
    pub items: Vec<WorkloadItemKind>,
    pub order_fn: fn(usize, u64) -> Vec<usize>,
}

// ── Program ──

/// A sequence of stages forming one mini-program.
pub struct Program {
    pub name: String,
    pub stages: Vec<Stage>,
}

/// Builder for a single program.
pub struct ProgramBuilder {
    name: String,
    stages: Vec<Stage>,
}

impl ProgramBuilder {
    pub fn new(name: &str) -> Self {
        ProgramBuilder {
            name: name.to_string(),
            stages: Vec::new(),
        }
    }

    /// Add a stage with the default strategy (Shuffle).
    pub fn stage(&mut self, items: Vec<WorkloadItemKind>) -> &mut Self {
        self.stages.push(Stage {
            items,
            order_fn: Shuffle::order,
        });
        self
    }

    /// Add a stage with a specific strategy.
    pub fn stage_with<S: StageStrategy>(&mut self, items: Vec<WorkloadItemKind>) -> &mut Self {
        self.stages.push(Stage {
            items,
            order_fn: S::order,
        });
        self
    }

    pub fn build(self) -> Program {
        Program {
            name: self.name,
            stages: self.stages,
        }
    }
}

// ── Workload ──

/// Collection of programs. Run one per iteration (selected by seed).
pub struct Workload {
    pub programs: Vec<Program>,
}

impl Default for Workload {
    fn default() -> Self {
        Self::new()
    }
}

impl Workload {
    pub fn new() -> Self {
        Workload { programs: Vec::new() }
    }

    pub fn program(&mut self, name: &str, f: impl FnOnce(&mut ProgramBuilder)) -> &mut Self {
        let mut builder = ProgramBuilder::new(name);
        f(&mut builder);
        self.programs.push(builder.build());
        self
    }

    /// Hash the workload structure for cache invalidation.
    /// Hashes program count, stage counts per program, and item kind
    /// tags per stage. Does not hash item parameters (e.g. n values)
    /// to avoid spurious invalidation on parameter tuning. Only
    /// structural changes (add/remove programs/stages/items) move the
    /// hash.
    pub fn structure_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mix_in = |h: u64, v: u64| -> u64 {
            (h ^ v).wrapping_mul(0x100000001b3)
        };
        h = mix_in(h, self.programs.len() as u64);
        for prog in &self.programs {
            h = mix_in(h, prog.stages.len() as u64);
            for stage in &prog.stages {
                h = mix_in(h, stage.items.len() as u64);
                for item in &stage.items {
                    // structural tag only, not parameter values
                    let tag: u64 = match item {
                        WorkloadItemKind::AlgoCall => 0,
                        WorkloadItemKind::ScalarWork { .. } => 1,
                        WorkloadItemKind::GraphWork { .. } => 2,
                        WorkloadItemKind::BranchWork { .. } => 3,
                        WorkloadItemKind::HeavyMemory { .. } => 4,
                        WorkloadItemKind::LightScalar => 5,
                        WorkloadItemKind::DomainWork { .. } => 6,
                    };
                    h = mix_in(h, tag);
                }
            }
        }
        h
    }

    /// Select and run a program. Returns checksum.
    /// The `on_algo_call` closure is invoked for each algo_call item.
    /// It receives the seed and should call the dylib entry, returning
    /// the FfiBenchCall result.
    pub fn run_program(
        &self,
        seed: u64,
        on_algo_call: &mut impl FnMut(u64) -> FfiBenchCall,
    ) -> u64 {
        let prog_idx = (seed % self.programs.len() as u64) as usize;
        let program = &self.programs[prog_idx];
        let mut accum = 0u64;
        let mut stage_seed = seed;

        for stage in &program.stages {
            stage_seed = mix(stage_seed);
            let order = (stage.order_fn)(stage.items.len(), stage_seed);

            for &idx in &order {
                let item_seed = mix(stage_seed ^ idx as u64);
                let item = &stage.items[idx];
                if item.is_algo_call() {
                    let result = on_algo_call(item_seed);
                    accum = mix(accum ^ result.run_ticks);
                } else {
                    item.run(item_seed, &mut accum);
                }
            }
        }

        accum
    }
}

// ── Convenience: workload_items! macro for building item vectors ──

/// Helper to build a Vec<WorkloadItemKind> from a list of items.
#[macro_export]
macro_rules! workload_items {
    ( $( $item:expr ),+ $(,)? ) => {
        vec![ $( $item ),+ ]
    };
}
