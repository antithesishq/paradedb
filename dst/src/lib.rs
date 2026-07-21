// Copyright (c) 2023-2026 ParadeDB, Inc.
//
// This file is part of ParadeDB - Postgres for Search and Analytics
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.

//! Deterministic-simulation-testing (DST) hooks — the one place that talks to the Antithesis
//! Rust SDK.
//!
//! The crate has three compilation modes, chosen automatically:
//!
//! 1. **SDK (`enabled` feature on).** The assertion macros forward to `antithesis_sdk` and
//!    dispatch to the Antithesis platform for real; [`GhostState<T>`] holds its value and
//!    `observe!` runs. This is the instrumented Antithesis build.
//! 2. **Debug (`enabled` off, `debug_assertions` on — i.e. a normal dev/`cargo test` build).**
//!    The crate is *live locally*: `observe!` runs, [`GhostState<T>`] holds its value, and the
//!    conditional/reachability properties become ordinary debug assertions —
//!    `assert_always!` / `assert_always_or_unreachable!` panic when their condition is false and
//!    `assert_unreachable!` panics when reached. `assert_sometimes!` (existential) and
//!    `assert_reachable!` cannot fail at a single call site, so they evaluate their inputs but
//!    never panic. The SDK is **not** linked.
//! 3. **Release (`enabled` off, `debug_assertions` off).** The crate compiles to nothing: the
//!    macros expand to a dead `if false { … }` that still type-checks their arguments,
//!    `GhostState<T>` is zero-sized, `observe!` closures are type-checked but never run, and
//!    [`init`] is a no-op — so an un-instrumented release build never links the SDK and pays
//!    nothing.
//!
//! The assertion wrappers mirror the SDK's signatures (`$message` must be a string literal) and
//! are macros, not functions, so each assertion's location is captured at the real call site.
//!
//! `observe!` and `GhostState` are ported from `precept` (orbitinghail/precept): `observe!` runs
//! a read-only property block whose `Fn` bound makes the compiler reject any code that mutates
//! the observed system, and `GhostState<T>` is auxiliary property-only state erased from release
//! builds. Here they sit on top of the official SDK rather than precept.

// Re-export the SDK so the `#[macro_export]` wrappers below can reach it from a consumer crate
// that does not depend on `antithesis_sdk` directly.
#[cfg(feature = "enabled")]
#[doc(hidden)]
pub use antithesis_sdk;

/// Register the Antithesis assertion catalog for this process. Required once per process that
/// emits assertions; without it a never-hit `assert_unreachable!` would pass vacuously instead
/// of being reported. `antithesis_init` is idempotent, so it is safe to call from every process
/// / forked worker. A no-op unless `enabled` (debug builds assert inline, so nothing to register).
#[cfg(feature = "enabled")]
pub fn init() {
    antithesis_sdk::antithesis_init();
}

/// See the [`enabled` definition](init).
#[cfg(not(feature = "enabled"))]
pub fn init() {}

// Debug-build backends for the assertion macros — compiled only in mode 2 (SDK off,
// `debug_assertions` on). A release build with the SDK off never references these; the SDK build
// forwards to `antithesis_sdk` instead. `details` is taken as `&dyn Debug` so a violation can
// echo the same payload the SDK build would ship (in practice a `&serde_json::Value`).

/// Backing for `assert_always!` / `assert_always_or_unreachable!`: panic if the invariant is
/// false when the site runs. (Reachability — "hit at least once" — can only be checked by the
/// platform, so it is not enforced here.)
#[cfg(all(not(feature = "enabled"), debug_assertions))]
#[doc(hidden)]
#[track_caller]
pub fn __dst_debug_invariant(
    condition: bool,
    message: &str,
    details: Option<&dyn ::core::fmt::Debug>,
) {
    if !condition {
        match details {
            Some(d) => {
                ::core::panic!("dst: `always` property violated: {message} (details: {d:?})")
            }
            None => ::core::panic!("dst: `always` property violated: {message}"),
        }
    }
}

/// Backing for `assert_sometimes!`: existential ("holds at least once across the run"). A single
/// false observation is not a violation and there is no cross-run tracker here, so evaluate the
/// inputs (keeping them live and type-checked) but never panic.
#[cfg(all(not(feature = "enabled"), debug_assertions))]
#[doc(hidden)]
pub fn __dst_debug_existential(
    condition: bool,
    message: &str,
    details: Option<&dyn ::core::fmt::Debug>,
) {
    let _ = (condition, message, details);
}

/// Backing for `assert_reachable!`: reaching the site is the success condition, so there is
/// nothing to assert.
#[cfg(all(not(feature = "enabled"), debug_assertions))]
#[doc(hidden)]
pub fn __dst_debug_reachable(message: &str, details: Option<&dyn ::core::fmt::Debug>) {
    let _ = (message, details);
}

/// Backing for `assert_unreachable!`: reaching the site *is* the violation, so panic.
#[cfg(all(not(feature = "enabled"), debug_assertions))]
#[doc(hidden)]
#[track_caller]
pub fn __dst_debug_unreachable(message: &str, details: Option<&dyn ::core::fmt::Debug>) {
    match details {
        Some(d) => ::core::panic!("dst: `unreachable` site reached: {message} (details: {d:?})"),
        None => ::core::panic!("dst: `unreachable` site reached: {message}"),
    }
}

// Two generator macros stamp out the wrappers. Each emits three mutually-exclusive definitions of
// the same `#[macro_export]` macro — one per compilation mode (SDK / debug / release, see the
// crate docs). `$d` is bound to `$` at each call site so the generated macro can name its own
// metavariables — the standard escape for a macro that defines a macro. `$debug_fn` names the
// mode-2 backing function above.

/// Generate a condition-style wrapper: `name!(condition, "message" [, &details])`.
// rustfmt cannot format a `macro_rules!` that defines a `macro_rules!` idempotently — each pass
// re-indents the nested arms further — so pin this generator's formatting by hand.
#[rustfmt::skip]
macro_rules! define_condition_assert {
    ($d:tt $name:ident, $debug_fn:ident, $doc:literal) => {
        // Mode 1 — SDK: forward to antithesis_sdk.
        #[doc = $doc]
        #[cfg(feature = "enabled")]
        #[macro_export]
        macro_rules! $name {
            ($d condition:expr, $d message:literal $d(, $d details:expr)?) => {
                $crate::antithesis_sdk::$name!($d condition, $d message $d(, $d details)?)
            };
        }

        // Mode 2 — debug: fire as a local debug assertion via the backing fn.
        #[doc = $doc]
        #[cfg(all(not(feature = "enabled"), debug_assertions))]
        #[macro_export]
        macro_rules! $name {
            ($d condition:expr, $d message:literal) => {
                $crate::$debug_fn($d condition, $d message, ::core::option::Option::None)
            };
            ($d condition:expr, $d message:literal, $d details:expr) => {
                $crate::$debug_fn(
                    $d condition,
                    $d message,
                    ::core::option::Option::Some($d details as &dyn ::core::fmt::Debug),
                )
            };
        }

        // Mode 3 — release: compile out, type-checking the arguments only.
        #[doc = $doc]
        #[cfg(all(not(feature = "enabled"), not(debug_assertions)))]
        #[macro_export]
        macro_rules! $name {
            ($d condition:expr, $d message:literal $d(, $d details:expr)?) => {
                if false {
                    let _: bool = $d condition;
                    let _: &str = $d message;
                    $d(let _ = &$d details;)?
                }
            };
        }
    };
}

/// Generate a message-only wrapper: `name!("message" [, &details])`.
#[rustfmt::skip]
macro_rules! define_message_assert {
    ($d:tt $name:ident, $debug_fn:ident, $doc:literal) => {
        // Mode 1 — SDK.
        #[doc = $doc]
        #[cfg(feature = "enabled")]
        #[macro_export]
        macro_rules! $name {
            ($d message:literal $d(, $d details:expr)?) => {
                $crate::antithesis_sdk::$name!($d message $d(, $d details)?)
            };
        }

        // Mode 2 — debug.
        #[doc = $doc]
        #[cfg(all(not(feature = "enabled"), debug_assertions))]
        #[macro_export]
        macro_rules! $name {
            ($d message:literal) => {
                $crate::$debug_fn($d message, ::core::option::Option::None)
            };
            ($d message:literal, $d details:expr) => {
                $crate::$debug_fn($d message, ::core::option::Option::Some($d details as &dyn ::core::fmt::Debug))
            };
        }

        // Mode 3 — release.
        #[doc = $doc]
        #[cfg(all(not(feature = "enabled"), not(debug_assertions)))]
        #[macro_export]
        macro_rules! $name {
            ($d message:literal $d(, $d details:expr)?) => {
                if false {
                    let _: &str = $d message;
                    $d(let _ = &$d details;)?
                }
            };
        }
    };
}

define_condition_assert!($ assert_always, __dst_debug_invariant,
    "Assert `condition` holds every time this site runs and that it is hit at least once. `message` must be a string literal; optional `details` is a `&serde_json::Value`. In a debug build a false condition panics.");
define_condition_assert!($ assert_always_or_unreachable, __dst_debug_invariant,
    "Like `assert_always!`, but the property still passes if the site is never hit. In a debug build a false condition panics.");
define_condition_assert!($ assert_sometimes, __dst_debug_existential,
    "Assert `condition` holds at least once across the run. Existential, so a debug build evaluates it but never panics.");
define_message_assert!($ assert_reachable, __dst_debug_reachable,
    "Assert this site is reached at least once across the run. Reaching it is success, so a debug build never panics.");
define_message_assert!($ assert_unreachable, __dst_debug_unreachable,
    "Assert this site is never reached; reaching it reports a violation. In a debug build reaching it panics.");

// Ghost state + read-only observation, ported from precept (orbitinghail/precept).

/// Auxiliary *ghost state* that exists only to express properties: its inner `T` can be read
/// only through [`observe!`] and mutated only through [`GhostState::mutate`]. In a release build
/// with the SDK off it is a zero-sized type and every access compiles out; the SDK and debug
/// builds hold the real value.
#[cfg(any(feature = "enabled", debug_assertions))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GhostState<T>(T);

/// See the [release definition](GhostState).
#[cfg(all(not(feature = "enabled"), not(debug_assertions)))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GhostState<T>(::core::marker::PhantomData<T>);

#[cfg(any(feature = "enabled", debug_assertions))]
impl<T> GhostState<T> {
    /// Create ghost state, initializing the inner `T` with `init`. `init` is read-only (an `Fn`);
    /// in a release build it is never called and no `T` is constructed.
    pub fn new<F: Fn() -> T>(init: F) -> Self {
        GhostState(init())
    }

    /// The sole mutator. `f` gets `&mut T`; everything it captures is read-only (`Fn`). In a
    /// release build `f` is type-checked but never called.
    pub fn mutate<F: Fn(&mut T)>(&mut self, f: F) {
        f(&mut self.0)
    }

    // Private — the only readers are the crate-internal `__observeN` helpers, so there is no
    // public way to obtain a `&T`: ghost state can only be read from inside an `observe!` block.
    fn inner(&self) -> &T {
        &self.0
    }
}

#[cfg(all(not(feature = "enabled"), not(debug_assertions)))]
impl<T> GhostState<T> {
    /// See the enabled/debug definition.
    #[allow(unused_variables)]
    pub fn new<F: Fn() -> T>(init: F) -> Self {
        GhostState(::core::marker::PhantomData)
    }

    /// See the enabled/debug definition.
    #[allow(unused_variables)]
    pub fn mutate<F: Fn(&mut T)>(&mut self, f: F) {}
}

// The per-arity `__observeN` helpers that back `observe!`. Each carries the `Fn(&T0, ..)` bound
// that enforces read-only access. In a release build the body is empty, so the closure is
// type-checked but never executed; the SDK and debug builds run it.
macro_rules! define_observe_helpers {
    ($( $name:ident ( $($ty:ident : $arg:ident),* ) ),* $(,)?) => {$(
        #[cfg(any(feature = "enabled", debug_assertions))]
        #[doc(hidden)]
        #[allow(clippy::too_many_arguments)]
        pub fn $name<$($ty,)* F: Fn($(&$ty),*)>($($arg: &$crate::GhostState<$ty>,)* f: F) {
            f($($arg.inner()),*)
        }

        #[cfg(all(not(feature = "enabled"), not(debug_assertions)))]
        #[doc(hidden)]
        #[allow(unused_variables, clippy::too_many_arguments)]
        pub fn $name<$($ty,)* F: Fn($(&$ty),*)>($($arg: &$crate::GhostState<$ty>,)* f: F) {}
    )*};
}

define_observe_helpers! {
    __observe0(),
    __observe1(T0: m0),
    __observe2(T0: m0, T1: m1),
    __observe3(T0: m0, T1: m1, T2: m2),
    __observe4(T0: m0, T1: m1, T2: m2, T3: m3),
}

/// Run a read-only observation block, optionally borrowing one or more [`GhostState`]s. The
/// block is an `Fn` closure — the compiler rejects any attempt to mutate what it captures — and
/// may call the assertion macros in this crate. In a release build it is type-checked but never
/// executed; the SDK and debug builds run it. Up to 4 ghost states may be observed at once.
#[macro_export]
macro_rules! observe {
    ($closure:expr $(,)?) => {
        $crate::__observe0($closure)
    };
    ($m0:expr, $closure:expr $(,)?) => {
        $crate::__observe1(&$m0, $closure)
    };
    ($m0:expr, $m1:expr, $closure:expr $(,)?) => {
        $crate::__observe2(&$m0, &$m1, $closure)
    };
    ($m0:expr, $m1:expr, $m2:expr, $closure:expr $(,)?) => {
        $crate::__observe3(&$m0, &$m1, &$m2, $closure)
    };
    ($m0:expr, $m1:expr, $m2:expr, $m3:expr, $closure:expr $(,)?) => {
        $crate::__observe4(&$m0, &$m1, &$m2, &$m3, $closure)
    };
}

#[cfg(test)]
mod tests {
    use crate::GhostState;

    // Compiles and runs in all three modes. Uses only assertions that must NOT panic here:
    // `always`/`always_or_unreachable` with true conditions, existential `sometimes`, and
    // `reachable`. `assert_unreachable!` is type-checked in an unreachable branch (in a debug
    // build it panics when reached — see `debug_unreachable_panics`).
    #[test]
    fn compiles_and_runs_in_all_configs() {
        crate::init();

        let mut seen = GhostState::new(|| 0i64);
        seen.mutate(|n| *n += 1);

        // NB: the assert wrappers are macro-generated `#[macro_export]` macros, which cannot be
        // referred to by an absolute path (`crate::assert_always!`) from inside this crate — so
        // call them unqualified (they are in textual scope). Consumer crates reference them
        // cross-crate as `dst::assert_*!`, which is unaffected.
        crate::observe!(seen, |n: &i64| {
            assert_always!(
                *n >= 0,
                "seen count is never negative",
                &::serde_json::json!({ "n": *n })
            );
        });

        crate::observe!(|| {
            assert_reachable!("observation ran");
            assert_sometimes!(true, "sometimes true");
            assert_always_or_unreachable!(1 + 1 == 2, "arithmetic holds");
            if false {
                assert_unreachable!("never reached in this test");
            }
        });
    }

    // In a debug build with the SDK off, a false `always` condition panics.
    #[cfg(all(not(feature = "enabled"), debug_assertions))]
    #[test]
    #[should_panic(expected = "`always` property violated")]
    fn debug_always_violation_panics() {
        crate::observe!(|| {
            assert_always!(false, "always-false must panic in a debug build");
        });
    }

    // In a debug build with the SDK off, reaching an `unreachable` site panics.
    #[cfg(all(not(feature = "enabled"), debug_assertions))]
    #[test]
    #[should_panic(expected = "`unreachable` site reached")]
    fn debug_unreachable_panics() {
        crate::observe!(|| {
            assert_unreachable!("reaching this must panic in a debug build");
        });
    }
}
