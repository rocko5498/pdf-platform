//! PDFium engine backend. Implements engine-api traits. [ADR-005]
//! SAFETY: FFI calls into PDFium (C/C++). Each unsafe block carries SAFETY comment. [ADR-027]

pub mod backend;

pub use backend::PdfiumEngine;
