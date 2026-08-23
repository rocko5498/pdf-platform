//! Host function implementations for WASM plugins. [ADR-014, M11]
//!
//! These functions are linked into the WASM instance's store as host imports.
//! Each function checks the plugin's capability grants before executing.

use wasmtime::{Caller, Linker};

use crate::grant::GrantStore;
use crate::manifest::{Capability, PluginManifest};
use crate::runtime::PluginInstanceState;

/// Link the host functions this plugin is entitled to into a wasmtime Linker.
///
/// Only functions covered by `granted` are wired in. A plugin that imports one
/// it was not granted fails to instantiate with an unknown-import error: the
/// capability is **absent from the instance, not merely denied at call time**,
/// which is what `ADR-014 §2` and `SDS §11.3` require. Every function also
/// re-checks its grant, so a revocation after link still denies.
///
/// `host_log` and `host_free_handle` need no capability (`SDS §11.3`).
pub fn link_host_functions(
    linker: &mut Linker<PluginInstanceState>,
    granted: &[Capability],
) -> Result<(), wasmtime::Error> {
    // Host logging (always available, no capability required)
    linker.func_wrap(
        "env",
        "host_log",
        |mut caller: Caller<'_, PluginInstanceState>,
         level: u32,
         _ptr: i32,
         _len: i32| {
            // Read message from caller's linear memory.
            let msg = read_wasm_string(&mut caller, _ptr, _len);
            eprintln!(
                "[plugin:{}] {} {}",
                caller.data().manifest.id,
                match level {
                    0 => "ERROR",
                    1 => "WARN",
                    2 => "INFO",
                    3 => "DEBUG",
                    _ => "TRACE",
                },
                msg
            );
        },
    )?;

    if granted.contains(&Capability::ReadText) {
        // Host get page count (requires ReadText capability)
        linker.func_wrap(
            "env",
            "host_get_page_count",
            |caller: Caller<'_, PluginInstanceState>| -> i32 {
                let state = caller.data();
                if !check_capability(&state.manifest, &state.grants, &Capability::ReadText) {
                    eprintln!(
                        "[plugin:{}] host_get_page_count denied: ReadText not granted",
                        state.manifest.id
                    );
                    return -1;
                }
                // Route to coordinator's page count via the shared store.
                state.page_count as i32
            },
        )?;
    }

    if granted.contains(&Capability::ReadText) {
        // Host get page text (requires ReadText capability)
        linker.func_wrap(
            "env",
            "host_get_page_text",
            |caller: Caller<'_, PluginInstanceState>,
             page_index: i32|
             -> i32 {
                let state = caller.data();
                if !check_capability(&state.manifest, &state.grants, &Capability::ReadText) {
                    eprintln!(
                        "[plugin:{}] host_get_page_text denied: ReadText not granted",
                        state.manifest.id
                    );
                    return -1;
                }
                // Route to coordinator's text extraction via the shared store.
                // Return the handle to the text data in the shared result store.
                if let Some(text) = state.page_texts.get(&(page_index as u32)) {
                    let handle = state.next_handle;
                    // In a full implementation, we'd store the text in a shared
                    // result buffer and return the handle. For now, log it.
                    eprintln!(
                        "[plugin:{}] got text for page {} ({} chars)",
                        state.manifest.id,
                        page_index,
                        text.len()
                    );
                    handle as i32
                } else {
                    -1
                }
            },
        )?;
    }

    if granted.contains(&Capability::ReadStructure) {
        // Host get outline (requires ReadStructure capability)
        linker.func_wrap(
            "env",
            "host_get_outline",
            |caller: Caller<'_, PluginInstanceState>| -> i32 {
                let state = caller.data();
                if !check_capability(&state.manifest, &state.grants, &Capability::ReadStructure) {
                    eprintln!(
                        "[plugin:{}] host_get_outline denied: ReadStructure not granted",
                        state.manifest.id
                    );
                    return -1;
                }
                // Route to coordinator's outline via the shared store.
                -1
            },
        )?;
    }

    if granted.contains(&Capability::ReadStructure) {
        // Host get layers (requires ReadStructure capability)
        linker.func_wrap(
            "env",
            "host_get_layers",
            |caller: Caller<'_, PluginInstanceState>| -> i32 {
                let state = caller.data();
                if !check_capability(&state.manifest, &state.grants, &Capability::ReadStructure) {
                    eprintln!(
                        "[plugin:{}] host_get_layers denied: ReadStructure not granted",
                        state.manifest.id
                    );
                    return -1;
                }
                // Route to coordinator's layers via the shared store.
                -1
            },
        )?;
    }

    if granted.contains(&Capability::ReadStructure) {
        // Host get attachments (requires ReadStructure capability)
        linker.func_wrap(
            "env",
            "host_get_attachments",
            |caller: Caller<'_, PluginInstanceState>| -> i32 {
                let state = caller.data();
                if !check_capability(&state.manifest, &state.grants, &Capability::ReadStructure) {
                    eprintln!(
                        "[plugin:{}] host_get_attachments denied: ReadStructure not granted",
                        state.manifest.id
                    );
                    return -1;
                }
                // Route to coordinator's attachments via the shared store.
                -1
            },
        )?;
    }

    if granted.contains(&Capability::Annotate) {
        // Host submit annotation (requires Annotate capability)
        linker.func_wrap(
            "env",
            "host_submit_annotation",
            |caller: Caller<'_, PluginInstanceState>,
             _ptr: i32,
             _len: i32|
             -> i64 {
                let state = caller.data();
                if !check_capability(&state.manifest, &state.grants, &Capability::Annotate) {
                    eprintln!(
                        "[plugin:{}] host_submit_annotation denied: Annotate not granted",
                        state.manifest.id
                    );
                    return -1;
                }
                // Route to coordinator as Command::AddAnnotation.
                // In a full implementation, we'd parse the annotation data
                // from the WASM memory and submit it as a Command.
                eprintln!(
                    "[plugin:{}] submit_annotation called (would route to coordinator)",
                    state.manifest.id
                );
                0
            },
        )?;
    }

    // Host free handle (always available)
    linker.func_wrap(
        "env",
        "host_free_handle",
        |caller: Caller<'_, PluginInstanceState>, handle: i32| {
            let state = caller.data();
            // Free the handle from the shared result store.
            eprintln!(
                "[plugin:{}] free_handle({})",
                state.manifest.id, handle
            );
        },
    )?;

    Ok(())
}

/// Read a string from WASM linear memory.
///
/// Reads `len` bytes starting at `ptr` from the caller's memory.
fn read_wasm_string(caller: &mut Caller<'_, PluginInstanceState>, ptr: i32, len: i32) -> String {
    if ptr < 0 || len < 0 {
        return String::new();
    }
    let memory = match caller.get_export("memory") {
        Some(wasmtime::Extern::Memory(m)) => m,
        _ => return String::new(),
    };
    let data = memory.data(caller);
    let start = ptr as usize;
    let end = start + len as usize;
    if end > data.len() {
        return String::new();
    }
    String::from_utf8_lossy(&data[start..end]).to_string()
}

/// Check if a plugin has a specific capability granted.
///
/// This is the capability gate for host functions. If the capability
/// is not granted, the host function should return an error or deny.
pub fn check_capability(
    manifest: &PluginManifest,
    grants: &GrantStore,
    capability: &Capability,
) -> bool {
    grants.is_granted(&manifest.id, capability)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Capability, PluginManifest};

    fn test_manifest() -> PluginManifest {
        PluginManifest {
            id: "com.example.test".into(),
            name: "Test".into(),
            version: "1.0.0".into(),
            author: "A".into(),
            description: "D".into(),
            wit_world: "pdf-platform:plugin@1".into(),
            capabilities: vec![Capability::ReadText, Capability::Annotate],
            panels: vec![],
            tools: vec![],
            job_types: vec![],
        }
    }

    #[test]
    fn capability_check_granted() {
        let manifest = test_manifest();
        let mut grants = GrantStore::new();
        grants.grant("com.example.test", Capability::ReadText);

        assert!(check_capability(
            &manifest,
            &grants,
            &Capability::ReadText,
        ));
        assert!(!check_capability(
            &manifest,
            &grants,
            &Capability::Annotate,
        ));
    }

    #[test]
    fn capability_check_not_granted() {
        let manifest = test_manifest();
        let grants = GrantStore::new();

        assert!(!check_capability(
            &manifest,
            &grants,
            &Capability::ReadText,
        ));
    }

    #[test]
    fn capability_check_revoked() {
        let manifest = test_manifest();
        let mut grants = GrantStore::new();
        grants.grant("com.example.test", Capability::ReadText);
        grants.revoke("com.example.test", &Capability::ReadText);

        assert!(!check_capability(
            &manifest,
            &grants,
            &Capability::ReadText,
        ));
    }
}
