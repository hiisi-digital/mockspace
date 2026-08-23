//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `bench_matrix!` declarative macro: the ergonomic front-end that emits the
//! `setup` / `cell_<tag>` / `matrix_decls()` boilerplate the engine consumes.
//!
//! The macro is the front-end to the same data model a consumer can hand-write
//! (see the crate README "Using the engine today"); the hand-form is exactly what
//! this expands to, so the two are interchangeable. The load-bearing invariants it
//! preserves, all from the extraction design:
//!
//! - A cell is a real, type-checked `pub fn` in the consumer crate, referenced by
//!   PATH from the generated sibling crates, never captured as a body. So the macro
//!   is a plain `macro_rules!`: it emits the function AND the `format!`-built path
//!   that names it, from one source, and does zero filesystem IO.
//! - The macro NEVER emits the measurement logic (anti-hoist, seed table, S timing,
//!   digest). That lives once in [`crate::scaffold`]; the generated variant is a
//!   one-line call to it. The macro only emits the cell/setup functions and the
//!   `MatrixDecl` data.
//!
//! `macro_rules!` cannot concatenate identifiers, so `cell_<tag>` and `setup_<tag>`
//! function names are built with `paste` (re-exported as [`crate::paste`], so a
//! consumer needs no direct `paste` dependency). Everything else is plain
//! `macro_rules!`. The two arms differ only in the regime value and the cell arity:
//! `warm` cells are `|s, seed|` (2-arg), `cold_cycle(M)` cells are `|s, k, seed|`
//! (3-arg), and the scaffold entry point follows from the regime.

/// Declare one matrix family: a `sweep` axis producing one bench per value, a set
/// of `cell`s compared within each bench, and a `size` sweep. See the crate README
/// for the full worked example and the extraction design for the grammar rationale.
///
/// ```ignore
/// pub struct DispatchState { pub d: Decoded, pub r: Vec<u64>, pub sinks: Vec<u32> }
///
/// bench_matrix! {
///     name: "carrier_dispatch",
///     crate_path: vehje_bench_carrier,
///     crate_dep: "vehje-bench-carrier = {{ path = \"../../carrier\"{carrier_features} }}",
///     extra_deps: [ "mockspace-bench-core = { path = \"...\" }" ],
///     seed: 0x5eed_d15b_a7c4_0002,
///     sweep profile in ["real", "madd", "tight"],
///     sizes: [64, 256, 1024, 4096, 16384],
///     baseline: "switch",
///     floor: "nullfloor",
///     regime: warm,
///
///     setup |profile: &str, n: usize| -> DispatchState { /* build state */ }
///
///     cell switch    |s, seed| { interpret(&s.d, seed, &mut s.r); checksum_at(&s.r, &s.sinks) }
///     cell fntable   |s, seed| { interpret_fntable(&s.d, seed, &mut s.r); checksum_at(&s.r, &s.sinks) }
///     cell nullfloor |s, seed| { interpret_null(&s.d, seed, &mut s.r); checksum_at(&s.r, &s.sinks) }
/// }
/// ```
#[macro_export]
macro_rules! bench_matrix {
    // ── warm regime: cells are |s, seed| (2-arg) ──
    (
        name: $name:literal,
        crate_path: $cp:path,
        crate_dep: $dep:literal,
        extra_deps: [ $( $ed:literal ),* $(,)? ],
        seed: $seed:literal,
        sweep $axis:ident in [ $( $sv:literal ),* $(,)? ],
        sizes: [ $( $sz:literal ),* $(,)? ],
        baseline: $base:literal,
        $( floor: $floor:literal, )?
        regime: warm,
        setup | $( $sa:ident : $sty:ty ),* $(,)? | -> $st:ty $sbody:block
        $(
            cell $tag:ident
                $( #[feature = $feat:literal] )*
                $( setup | $( $csa:ident : $csty:ty ),* $(,)? | -> $cst:ty $csbody:block )?
                | $cs:ident , $cseed:ident | $cbody:block
        )+
    ) => {
        /// The family's shared setup: the S term, timed on every call by the scaffold.
        pub fn setup( $( $sa : $sty ),* ) -> $st $sbody

        $(
            // optional per-cell setup override (times a different construction for
            // this cell, e.g. the `direct` cell that needs resolved handlers). It is
            // cfg-gated by the cell's own feature: the cell body reaches a
            // feature-gated consumer API, so the fn must not exist (and not be
            // compiled) unless the depending variant built the carrier with that
            // feature. The variant that requests the feature is the only one that
            // references the path, and it built the carrier with the feature on.
            // Routed through `__bm_setup!` so the feature list and the optional
            // setup are sibling arguments there, never nested (a `$(...)*` feat
            // list cannot expand inside a `$(...)?` setup group).
            $crate::__bm_setup!(
                [ $( $feat ),* ] $tag ;
                $( setup | $( $csa : $csty ),* | -> $cst $csbody )?
            );
            // the measured-op cell fn: one keep-alive u64 the scaffold folds per iter.
            // a per-cell setup returns its own state type, so the cell reads that
            // (`$cst`) rather than the shared `$st` when the override is present.
            // Same cfg gate: a feature-carrying cell exists iff its feature is on.
            $( #[cfg(feature = $feat)] )*
            $crate::paste::paste! {
                pub fn [< cell_ $tag >](
                    $cs : &mut $crate::__bm_state!( $st $( , $cst )? ),
                    $cseed : u64
                ) -> u64 $cbody
            }
        )+

        /// The matrix as data: paths of the setup and cell functions, never bodies.
        pub fn matrix_decls() -> ::std::vec::Vec<$crate::MatrixDecl> {
            $crate::__bench_matrix_decl!(
                @build $crate::Regime::Warm,
                $name, $cp, $dep, [ $( $ed ),* ], $seed, $axis, [ $( $sv ),* ],
                [ $( $sz ),* ], $base, [ $( $floor )? ],
                [ $(
                    { $tag, [ $( $feat ),* ], [ $( $cst )? ] }
                )+ ]
            )
        }
    };

    // ── cold_cycle(M) regime: cells are |s, k, seed| (3-arg) ──
    (
        name: $name:literal,
        crate_path: $cp:path,
        crate_dep: $dep:literal,
        extra_deps: [ $( $ed:literal ),* $(,)? ],
        seed: $seed:literal,
        sweep $axis:ident in [ $( $sv:literal ),* $(,)? ],
        sizes: [ $( $sz:literal ),* $(,)? ],
        baseline: $base:literal,
        $( floor: $floor:literal, )?
        regime: cold_cycle( $m:literal ),
        setup | $( $sa:ident : $sty:ty ),* $(,)? | -> $st:ty $sbody:block
        $(
            cell $tag:ident
                $( #[feature = $feat:literal] )*
                $( setup | $( $csa:ident : $csty:ty ),* $(,)? | -> $cst:ty $csbody:block )?
                | $cs:ident , $ck:ident , $cseed:ident | $cbody:block
        )+
    ) => {
        pub fn setup( $( $sa : $sty ),* ) -> $st $sbody

        $(
            $crate::__bm_setup!(
                [ $( $feat ),* ] $tag ;
                $( setup | $( $csa : $csty ),* | -> $cst $csbody )?
            );
            // cold cell: third param k is the iteration index the scaffold provides.
            $( #[cfg(feature = $feat)] )*
            $crate::paste::paste! {
                pub fn [< cell_ $tag >](
                    $cs : &mut $crate::__bm_state!( $st $( , $cst )? ),
                    $ck : usize,
                    $cseed : u64
                ) -> u64 $cbody
            }
        )+

        pub fn matrix_decls() -> ::std::vec::Vec<$crate::MatrixDecl> {
            $crate::__bench_matrix_decl!(
                @build $crate::Regime::ColdCycle($m),
                $name, $cp, $dep, [ $( $ed ),* ], $seed, $axis, [ $( $sv ),* ],
                [ $( $sz ),* ], $base, [ $( $floor )? ],
                [ $(
                    { $tag, [ $( $feat ),* ], [ $( $cst )? ] }
                )+ ]
            )
        }
    };

    // ── stream regime: cells are |s, input| (2-arg, second is the input slice) ──
    (
        name: $name:literal,
        crate_path: $cp:path,
        crate_dep: $dep:literal,
        extra_deps: [ $( $ed:literal ),* $(,)? ],
        seed: $seed:literal,
        sweep $axis:ident in [ $( $sv:literal ),* $(,)? ],
        sizes: [ $( $sz:literal ),* $(,)? ],
        baseline: $base:literal,
        $( floor: $floor:literal, )?
        regime: stream,
        setup | $( $sa:ident : $sty:ty ),* $(,)? | -> $st:ty $sbody:block
        $(
            cell $tag:ident
                $( #[feature = $feat:literal] )*
                $( setup | $( $csa:ident : $csty:ty ),* $(,)? | -> $cst:ty $csbody:block )?
                | $cs:ident , $cinput:ident | $cbody:block
        )+
    ) => {
        pub fn setup( $( $sa : $sty ),* ) -> $st $sbody

        $(
            $crate::__bm_setup!(
                [ $( $feat ),* ] $tag ;
                $( setup | $( $csa : $csty ),* | -> $cst $csbody )?
            );
            // stream cell: the second param is the harness input slice the cell sweeps.
            $( #[cfg(feature = $feat)] )*
            $crate::paste::paste! {
                pub fn [< cell_ $tag >](
                    $cs : &mut $crate::__bm_state!( $st $( , $cst )? ),
                    $cinput : &[u8]
                ) -> u64 $cbody
            }
        )+

        pub fn matrix_decls() -> ::std::vec::Vec<$crate::MatrixDecl> {
            $crate::__bench_matrix_decl!(
                @build $crate::Regime::Stream,
                $name, $cp, $dep, [ $( $ed ),* ], $seed, $axis, [ $( $sv ),* ],
                [ $( $sz ),* ], $base, [ $( $floor )? ],
                [ $(
                    { $tag, [ $( $feat ),* ], [ $( $cst )? ] }
                )+ ]
            )
        }
    };
}

/// Internal: build the `Vec<MatrixDecl>` from the parsed pieces. Shared by both
/// regime arms so the `MatrixDecl` construction lives once. The per-cell setup
/// presence is carried as a `[ ... ]` group that is empty (shared setup) or holds
/// the override marker (per-cell setup), so the `setup_path` Option is set by a
/// `$( ... )?`-style match without the caller needing conditional logic.
#[doc(hidden)]
#[macro_export]
macro_rules! __bench_matrix_decl {
    (
        @build $regime:expr,
        $name:literal, $cp:path, $dep:literal, [ $( $ed:literal ),* ], $seed:literal,
        $axis:ident, [ $( $sv:literal ),* ], [ $( $sz:literal ),* ], $base:literal,
        [ $( $floor:literal )? ],
        [ $( { $tag:ident, [ $( $feat:literal ),* ], [ $( $has_setup:tt )* ] } )+ ]
    ) => {
        ::std::vec![ $crate::MatrixDecl {
            name:        $name.to_string(),
            crate_path:  stringify!($cp).to_string(),
            crate_dep:   $dep.to_string(),
            extra_deps:  ::std::vec![ $( $ed.to_string() ),* ],
            master_seed: stringify!($seed).to_string(),
            sweep:       $crate::SweepAxis {
                name:   stringify!($axis).to_string(),
                values: ::std::vec![ $( $sv.to_string() ),* ],
            },
            sizes:       ::std::vec![ $( $sz ),* ],
            baseline:    $base.to_string(),
            floor:       {
                let mut __f: ::std::option::Option<::std::string::String> =
                    ::std::option::Option::None;
                $( __f = ::std::option::Option::Some($floor.to_string()); )?
                __f
            },
            regime:      $regime,
            setup_path:  format!("{}::setup", module_path!()),
            cells:       ::std::vec![ $(
                $crate::CellDecl {
                    tag:        stringify!($tag).to_string(),
                    op_path:    format!("{}::cell_{}", module_path!(), stringify!($tag)),
                    setup_path: $crate::__bench_matrix_decl!(
                        @cell_setup $tag, [ $( $has_setup )* ]
                    ),
                    features:   ::std::vec![ $( $feat.to_string() ),* ],
                }
            ),+ ],
        } ]
    };

    // per-cell setup path: empty marker group => shared setup (None); non-empty =>
    // this cell has its own `setup_<tag>` fn.
    ( @cell_setup $tag:ident, [ ] ) => {
        ::std::option::Option::None
    };
    ( @cell_setup $tag:ident, [ $( $marker:tt )+ ] ) => {
        ::std::option::Option::Some(format!("{}::setup_{}", module_path!(), stringify!($tag)))
    };
}

/// Internal: pick a cell's state type. With no per-cell setup the cell reads the
/// family's shared state (`$shared`); with a per-cell setup override it reads that
/// setup's own return type (`$cell`). Expands in type position.
#[doc(hidden)]
#[macro_export]
macro_rules! __bm_state {
    ( $shared:ty ) => { $shared };
    ( $shared:ty , $cell:ty ) => { $cell };
}

/// Internal: emit a cell's optional per-cell setup fn, cfg-gated by the cell's
/// feature list. The feature list `[ $feat, ... ]` and the optional setup are
/// passed as SIBLING arguments so the feature `$(...)*` never nests inside the
/// setup `$(...)?` (which macro_rules rejects when a cell has a feature but no
/// per-cell setup, "feat repeats 1 time but csa repeats 0 times"). The empty
/// arm (no per-cell setup) emits nothing; the present arm emits the gated fn.
#[doc(hidden)]
#[macro_export]
macro_rules! __bm_setup {
    // no per-cell setup: nothing to emit (the cell uses the shared `setup`).
    ( [ $( $feat:literal ),* ] $tag:ident ; ) => {};
    // per-cell setup present: emit `setup_<tag>`, gated on the cell's features.
    (
        [ $( $feat:literal ),* ] $tag:ident ;
        setup | $( $csa:ident : $csty:ty ),* $(,)? | -> $cst:ty $csbody:block
    ) => {
        $( #[cfg(feature = $feat)] )*
        $crate::paste::paste! {
            pub fn [< setup_ $tag >]( $( $csa : $csty ),* ) -> $cst $csbody
        }
    };
}
