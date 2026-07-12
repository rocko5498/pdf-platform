//! Z1 worker binary. Never holds authoritative document state. [ADR-008, SDS §2.3]
//!
//! M0: IPC adopt + echo + structural inspect via **inherited document handle**
//! (no filesystem path). [SDS §3.1, GR-1]

use std::fs::File;
use std::process::ExitCode;
use std::time::Duration;

use pdf_cos::scan::scan_file;
use protocol::inspect::{encode_summary, StructuralSummary};
use protocol::transport::{TransportError, WorkerTransport as _};
use sandbox::spawn::{adopt_document_file, adopt_inherited};

fn main() -> ExitCode {
    let mut transport = match adopt_inherited() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("worker: adopt IPC failed: {e}");
            return ExitCode::from(1);
        }
    };

    let doc_file: Option<File> = match adopt_document_file() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("worker: adopt document failed: {e}");
            return ExitCode::from(6);
        }
    };

    loop {
        match transport.recv_timeout(Duration::from_secs(1)) {
            Ok(msg) if msg == b"quit" => break,
            Ok(msg) if msg == b"inspect" => {
                let Some(file) = doc_file.as_ref() else {
                    eprintln!("worker: inspect requested but no inherited document");
                    return ExitCode::from(4);
                };
                match scan_and_encode(file) {
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

fn scan_and_encode(file: &File) -> Result<Vec<u8>, String> {
    let ds = scan_file(file).map_err(|e| e.to_string())?;
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
