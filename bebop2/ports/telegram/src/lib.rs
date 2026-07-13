//! DK-02 — Telegram Notify port, implemented as a `wasm32-wasip2` component.
//!
//! This guest is the FIRST real Port-as-WASM-component (WR-01). It implements the
//! `NotificationPort` contract from `proto-cap` (`required_scope() ==
//! Order::Notify`) as a WASI-p2 component whose ONLY host import is
//! `notify-telegram` — the single function the host grants for the
//! `Resource::Order x Action::Notify` scope (scope.rs discriminants 0x06 / 0x08).
//!
//! ZERO-AMBIENT-AUTHORITY (proven, not asserted): the guest is `#![no_std]` with a
//! private bump allocator, so the produced component imports EXACTLY ONE host
//! function — `notify-telegram`. There is no `wasi:filesystem`, `wasi:sockets`,
//! `wasi:cli`, or `wasi:io` import anywhere (verified with `wasm-tools component
//! wit`). The host (DK-03) mechanically rejects any component that asks for more
//! than its `Scope` allows, at instantiation time. The guest therefore literally
//! cannot open a socket, read a file, or take any action outside `Order::Notify`.
//!
//! Built with `cargo component build --target wasm32-wasip2` (see
//! `tooling/build-wasm-component.sh`). Not exercised by the default
//! `cargo test --workspace` (no rust `#[test]`s; no `bebop2-core` dep — that crate
//! defines a `panic_impl` lang item that collides with a guest allocator).

#![no_std]

extern crate alloc;
use alloc::format;
use alloc::string::String;

// ── minimal bump allocator over a static heap (no std, no __heap_base) ──────
static mut HEAP: [u8; 65_536] = [0u8; 65_536];

struct BumpAlloc {
    next: core::sync::atomic::AtomicUsize,
}

unsafe impl alloc::alloc::GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: alloc::alloc::Layout) -> *mut u8 {
        let start = HEAP.as_ptr() as usize;
        let mut pos = self.next.load(core::sync::atomic::Ordering::Relaxed);
        // align up
        let aligned = (pos + layout.align() - 1) & !(layout.align() - 1);
        let new_pos = aligned + layout.size();
        if new_pos > start + HEAP.len() {
            // Heap exhausted: trap (no host channel to report on).
            loop {}
        }
        self.next.store(new_pos, core::sync::atomic::Ordering::Relaxed);
        (start + aligned) as *mut u8
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: alloc::alloc::Layout) {}
}

#[global_allocator]
static ALLOC: BumpAlloc = BumpAlloc {
    next: core::sync::atomic::AtomicUsize::new(0),
};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // Trapped component: no host output channel by design (zero ambient authority).
    loop {}
}

#[allow(warnings)]
mod bindings;

use bindings::Guest;

/// Scope fixed-layout tag for this port: `Order::Notify` (scope.rs discriminants).
/// Pure `const` — no host import required to know who we are.
const SCOPE_TAG: [u8; 2] = [0x06u8, 0x08u8]; // Resource::Order, Action::Notify

/// The component instance. `cargo-component` generates the `Guest` trait for our
/// world (`notify-telegram`); we implement `run` — the single exported entry the
/// host invokes after authorizing `Order::Notify`.
struct Component;

impl Guest for Component {
    fn run() -> u32 {
        // Deliver the notification through the ONLY permitted host import.
        // `notify-telegram` is the host-side function mapped to Order::Notify.
        // The payload embeds the scope tag so the host can correlate the call to
        // the exact `(resource, action)` it authorized.
        let message = format!(
            "bebop order-notify scope={:02x}{:02x}",
            SCOPE_TAG[0], SCOPE_TAG[1]
        );
        bindings::notify_telegram(&message)
    }
}

bindings::export!(Component with_types_in bindings);
