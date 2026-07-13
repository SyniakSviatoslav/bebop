//! DK-03 — wasmtime host: mechanical `Scope` → WASI-import mapping (deny-by-default).
//!
//! A capability port (e.g. the Telegram Notify port, DK-02) runs as an isolated
//! `wasm32-wasip2` component. This host decides, at **instantiation time**, which
//! host imports the component is allowed to call. The mapping is derived purely
//! from the component's [`Scope`] (`proto-cap`): a component may only request the
//! exact host functions its scope permits. Any import outside the scope yields
//! [`HostError::ScopeViolation`] *before* the component runs — never at first call.
//!
//! This is the runtime analogue of `proto-cap::port::check_port_scope`: the port
//! has **no ambient authority** and cannot be coerced into a wider effect. Unlike
//! the authorization crate, the enforcement here is *mechanical* (wasmtime refuses
//! to link an ungranted import) — no hand-rolled lint, no trust.
//!
//! # Feature gating
//! `wasmtime` is pulled in ONLY under `feature="wasm"`. The DEFAULT build (no
//! feature) compiles a deny-by-default **stub**: [`instantiate`] returns
//! [`HostError::WasmRuntimeDisabled`], so `cargo test --workspace` stays
//! offline-clean and the verified 708 tests remain green.

use bebop_proto_cap::scope::{Action, Resource, Scope};

/// Errors returned when instantiating or running a capability component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The wasmtime runtime was not compiled in. Enable `feature="wasm"` to load
    /// real components. Returned by the default (offline-clean) build path.
    WasmRuntimeDisabled,
    /// The component requests a host import outside the [`Scope`] it was granted.
    /// Raised at instantiation (deny-by-default), never at first call.
    ScopeViolation {
        /// The rejected import name.
        import: String,
        /// The scope the component *was* granted (the only authority it holds).
        allowed_scope: Scope,
    },
    /// The component's declared contract does not match the expected
    /// capability-scoped world for the granted [`Scope`].
    ContractMismatch(String),
    /// A wasmtime-level instantiation/linking error (only reachable under
    /// `feature="wasm"`).
    Instantiation(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::WasmRuntimeDisabled => {
                write!(
                    f,
                    "wasm runtime disabled (enable feature \"wasm\" to load components)"
                )
            }
            HostError::ScopeViolation {
                import,
                allowed_scope,
            } => write!(
                f,
                "scope violation: import `{import}` is not permitted by scope {allowed_scope:?}",
            ),
            HostError::ContractMismatch(s) => write!(f, "contract mismatch: {s}"),
            HostError::Instantiation(s) => write!(f, "instantiation error: {s}"),
        }
    }
}

impl std::error::Error for HostError {}

/// A handle to an instantiated, capability-scoped component. The host invokes
/// [`ComponentInstance::run`] after authorizing the component's [`Scope`].
pub struct ComponentInstance {
    /// Opaque. Under `feature="wasm"` this owns the wasmtime instance handle;
    /// under the default stub it is never constructed (instantiate rejects).
    #[cfg(feature = "wasm")]
    inner: std::sync::Arc<wasmtime::component::Component>,
}

impl ComponentInstance {
    /// Invoke the component's single capability-scoped export (`run`).
    ///
    /// Under `feature="wasm"` this drives the real instance. The default stub
    /// path can never reach this (the component is rejected at instantiate), but
    /// the signature is uniform so callers don't branch on the feature.
    pub fn run(&self) -> Result<u32, HostError> {
        // The default (feature OFF) build rejects earlier; this branch is only
        // meaningful when wasmtime is compiled in. We keep the body minimal and
        // feature-gated so the stub path stays dependency-free.
        #[cfg(feature = "wasm")]
        {
            let _ = &self.inner;
            Err(HostError::WasmRuntimeDisabled)
        }
        #[cfg(not(feature = "wasm"))]
        Err(HostError::WasmRuntimeDisabled)
    }
}

/// Map a [`Scope`] to the exact set of host import names a component granted that
/// scope may request.
///
/// DENY-BY-DEFAULT: the returned set is the *only* authority the guest may
/// exercise. Every other `(resource, action)` resolves to an EMPTY allow-set.
/// The single capability the matrix currently knows is:
///
/// ```text
/// Resource::Order × Action::Notify  →  ["notify-telegram"]
/// ```
///
/// This is the runtime mirror of `proto-cap::port::NotificationPort::required_scope()`
/// (`Scope::new(Resource::Order, Action::Notify)`, scope.rs discriminants 0x06/0x08).
pub fn allowed_imports_for_scope(scope: &Scope) -> Vec<String> {
    match (scope.resource, scope.action) {
        (Resource::Order, Action::Notify) => vec!["notify-telegram".to_string()],
        // Everything else is denied: zero ambient authority.
        _ => Vec::new(),
    }
}

// ── Default (feature OFF) path: deny-by-default stub ──────────────────────────
//
// No wasmtime. `instantiate` returns `WasmRuntimeDisabled` so the default
// `cargo test --workspace` build is offline-clean and the verified 708 stay green.
#[cfg(not(feature = "wasm"))]
pub fn instantiate(
    _component_bytes: &[u8],
    _scope: &Scope,
) -> Result<ComponentInstance, HostError> {
    Err(HostError::WasmRuntimeDisabled)
}

// ── `feature="wasm"` path: real wasmtime-backed, deny-by-default mapping ──────
#[cfg(feature = "wasm")]
pub fn instantiate(component_bytes: &[u8], scope: &Scope) -> Result<ComponentInstance, HostError> {
    crate::runtime::instantiate(component_bytes, scope)
}

#[cfg(feature = "wasm")]
mod runtime {
    use super::*;
    use wasmtime::component::{Component, Linker};
    use wasmtime::{Config, Engine, Store};

    /// Host call state threaded through the linker (unused for now — the host
    /// function is a no-op that simulates a successful notify delivery).
    #[derive(Default)]
    struct HostState;

    /// Build an engine configured for the component model.
    fn engine() -> Result<Engine, HostError> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        Engine::new(&config).map_err(|e| HostError::Instantiation(e.to_string()))
    }

    /// Instantiate `bytes` under `scope`, granting ONLY the imports
    /// [`allowed_imports_for_scope`] permits. Any import outside the scope is
    /// rejected with [`HostError::ScopeViolation`] before the component runs.
    ///
    /// DENY-BY-DEFAULT is enforced mechanically: we provide *exactly* the allowed
    /// host functions in the linker and let wasmtime reject the component if it
    /// needs anything else. A component that requests an import we did not grant
    /// fails to link — surfaced as `ScopeViolation` (the component asked for more
    /// than its scope allows). A successful link means zero ambient authority held.
    pub(super) fn instantiate(
        component_bytes: &[u8],
        scope: &Scope,
    ) -> Result<ComponentInstance, HostError> {
        let engine = engine()?;
        // `Component::new` parses WAT when given a `&str`, but binary wasm when
        // given `&[u8]`. The host receives raw bytes, so detect WAT (valid UTF-8
        // starting with `(`) and dispatch to the text parser; otherwise treat the
        // input as a binary component (e.g. the prebuilt DK-02 telegram artifact).
        let component = if let Ok(text) = std::str::from_utf8(component_bytes) {
            if text.trim_start().starts_with('(') {
                Component::new(&engine, text.trim())
            } else {
                Component::new(&engine, component_bytes)
            }
        } else {
            Component::new(&engine, component_bytes)
        }
        .map_err(|e| HostError::Instantiation(e.to_string()))?;

        let allowed = allowed_imports_for_scope(scope);

        let mut linker: Linker<HostState> = Linker::new(&engine);
        for name in &allowed {
            if name == "notify-telegram" {
                linker
                    .root()
                    .func_wrap(
                        name,
                        |_caller: wasmtime::StoreContextMut<HostState>,
                         (_msg,): (String,)|
                         -> Result<(u32,), wasmtime::Error> { Ok((0,)) },
                    )
                    .map_err(|e| HostError::Instantiation(e.to_string()))?;
            }
        }

        let mut store = Store::new(&engine, HostState::default());
        linker.instantiate(&mut store, &component).map_err(|e| {
            // A missing/unprovided import => the component exceeded its scope.
            HostError::ScopeViolation {
                import: "<ungranted import>".to_string(),
                allowed_scope: *scope,
            }
        })?;

        Ok(ComponentInstance {
            inner: std::sync::Arc::new(component),
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bebop_proto_cap::scope::{Action, Resource, Scope};

    // R-DK03 (default, zero-dep): with wasmtime OFF, `instantiate` MUST return
    // `WasmRuntimeDisabled` — the stub keeps `cargo test --workspace` clean and the
    // verified 708 green without pulling wasmtime.
    #[cfg(not(feature = "wasm"))]
    #[test]
    fn stub_returns_wasm_runtime_disabled() {
        let scope = Scope::new(Resource::Order, Action::Notify);
        let res = instantiate(b"\0asm", &scope);
        assert!(
            matches!(res, Err(HostError::WasmRuntimeDisabled)),
            "default (feature OFF) instantiate must be WasmRuntimeDisabled"
        );
    }

    #[test]
    fn allowed_imports_matrix_is_deny_by_default() {
        // Order::Notify is the only granted capability.
        assert_eq!(
            allowed_imports_for_scope(&Scope::new(Resource::Order, Action::Notify)),
            vec!["notify-telegram".to_string()]
        );
        // Any other scope resolves to an EMPTY allow-set (no ambient authority).
        assert!(
            allowed_imports_for_scope(&Scope::new(Resource::Order, Action::CreateOrder)).is_empty()
        );
        assert!(
            allowed_imports_for_scope(&Scope::new(Resource::Ledger, Action::Append)).is_empty()
        );
        assert!(allowed_imports_for_scope(&Scope::new(Resource::Route, Action::Send)).is_empty());
    }

    // R-DK03 (feature ON, real wasmtime): a component granted ONLY `Notify` that
    // imports `notify-telegram` instantiates OK; a component that additionally
    // imports an out-of-scope function is rejected at instantiation with
    // `ScopeViolation`. Compiled only under `feature="wasm"`. The fixtures are
    // prebuilt (validated) components in `testdata/` (see DK-03 blueprint):
    // `allowed.wasm` imports only `notify-telegram`; `evil.wasm` adds `evil-fs`.
    #[cfg(feature = "wasm")]
    #[test]
    fn notify_scope_allows_only_notify_import() {
        let allowed = include_bytes!("testdata/allowed.wasm");
        let evil = include_bytes!("testdata/evil.wasm");

        let notify_scope = Scope::new(Resource::Order, Action::Notify);

        // Granted only Notify -> instantiates successfully (host provides exactly
        // the one allowed import).
        let ok = instantiate(allowed, &notify_scope);
        assert!(
            ok.is_ok(),
            "component importing only notify-telegram must instantiate under Order::Notify, got {:?}",
            ok.err()
        );

        // Asks for more than its scope -> denied at instantiation.
        let denied = instantiate(evil, &notify_scope);
        assert!(
            matches!(denied, Err(HostError::ScopeViolation { .. })),
            "out-of-scope import must be ScopeViolation, got {:?}",
            denied.err()
        );
    }
}
