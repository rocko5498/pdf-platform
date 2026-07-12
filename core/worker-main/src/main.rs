//! Z1 worker binary. Never holds authoritative document state. [ADR-008, SDS §2.3]
//!
//! M0: IPC adopt + echo + optional structural inspect (pdf-cos). No PDFium, no confinement.

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use pdf_cos::scan::scan_structure;
use protocol::inspect::{encode_summary, StructuralSummary};
use protocol::transport::{TransportError, WorkerTransport as _};
use sandbox::spawn::{adopt_inherited, ENV_DOC_PATH};

fn main() -> ExitCode {
    let mut transport = match adopt_inherited() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("worker: adopt IPC failed: {e}");
            return ExitCode::from(1);
        }
    };

    // // ponytail: DOC_PATH in Z1 is temporary zone debt (design open-inspect).
    let doc_path = std::env::var(ENV_DOC_PATH).ok();

    loop {
        match transport.recv_timeout(Duration::from_secs(1)) {
            Ok(msg) if msg == b"quit" => break,
            Ok(msg) if msg == b"inspect" => {
                let Some(path) = doc_path.as_deref() else {
                    eprintln!("worker: inspect requested but no {ENV_DOC_PATH}");
                    return ExitCode::from(4);
                };
                match scan_and_encode(Path::new(path)) {
                    Ok(body) => {
                        if let Err(e) = transport.send(&body) {
                            eprintln!("worker: send summary failed: {e}");
                            return ExitCode::from(2);
                        }
                    }
                    Err(e) => {
                        eprintln!("worker: inspect failed: {e}");
                        return ExitCode::from(5);
                    }
                }
            }
            Ok(msg) => {
                if let Err(e) = transport.send(&msg) {
                    eprintln!("worker: send failed: {e}");
                    return ExitCode::from(2);
                }
            }
            Err(TransportError::Timeout) => continue,
            Err(TransportError::Disconnected) => break,
            Err(e) => {
                eprintln!("worker: recv failed: {e}");
                return ExitCode::from(3);
            }
        }
    }
    ExitCode::SUCCESS
}

fn scan_and_encode(path: &Path) -> Result<Vec<u8>, String> {
    let ds = scan_structure(path).map_err(|e| e.to_string())?;
    let summary = StructuralSummary {
        page_count: ds.page_count,
        has_acroform: ds.has_acroform,
        has_xfa: ds.has_xfa,
        has_js: ds.has_js,
        sig_count: ds.sig_count,
        leniency_count: ds.leniency.len() as u32,
        leniency_events: ds.leniency.iter().map(|e| e.to_string()).collect(),
    };
    Ok(encode_summary(&summary))
}
