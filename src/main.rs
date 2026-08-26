#![cfg_attr(target_family = "wasm", no_main)]

use codex_arg0::Arg0DispatchPaths;
use codex_gui::{init_tracing, run_app};

#[cfg(not(target_family = "wasm"))]
fn main() {
    let arg0_guard = codex_arg0::arg0_dispatch();
    let current_exe = std::env::current_exe().ok();
    let arg0_paths = Arg0DispatchPaths {
        codex_self_exe: current_exe.clone(),
        codex_linux_sandbox_exe: if cfg!(target_os = "linux") {
            arg0_guard
                .as_ref()
                .and_then(|guard| guard.paths().codex_linux_sandbox_exe.clone())
                .or(current_exe)
        } else {
            None
        },
        main_execve_wrapper_exe: arg0_guard
            .as_ref()
            .and_then(|guard| guard.paths().main_execve_wrapper_exe.clone()),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
        .expect("failed to create embedded Codex runtime");

    init_tracing();
    run_app(runtime.handle().clone(), arg0_paths);
    runtime.shutdown_timeout(std::time::Duration::from_secs(5));
    drop(arg0_guard);
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    panic!("the embedded Codex runtime is not available on wasm");
}
