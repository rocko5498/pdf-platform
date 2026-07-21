use std::sync::Arc;
use std::time::Duration;

use jobs::utility_pool::UtilityPool;
use jobs::{JobEvent, JobGraph, JobPriority, JobScheduler, JobSpec};
use protocol::utility_jobs::UtilityJobInput;

#[test]
fn low_priority_utility_worker_inherits_document_and_shmem_handles() {
    use protocol::commands::{encode_command, Command};
    use protocol::events::{decode_worker_event, WorkerEvent};
    use sandbox::shmem::SharedRegion;
    use sandbox::spawn::spawn_utility_worker_with_io;

    let path = std::env::temp_dir().join(format!(
        "pdf-platform-utility-attachment-{}.bin",
        std::process::id()
    ));
    std::fs::write(&path, b"not a PDF").unwrap();
    let document = std::fs::File::open(&path).unwrap();
    let output_path = path.with_extension("out");
    let output = std::fs::File::create(&output_path).unwrap();
    let shmem = SharedRegion::create(4096).unwrap();
    let mut child = spawn_utility_worker_with_io(
        std::path::Path::new(env!("CARGO_BIN_EXE_worker")),
        &document,
        shmem.file(),
        &output,
        None,
    )
    .unwrap();
    child
        .transport
        .send(&encode_command(&Command::Inspect { correlation_id: 91 }))
        .unwrap();
    let response = child
        .transport
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    match decode_worker_event(&response).unwrap() {
        WorkerEvent::RenderError { message, .. } => {
            assert_ne!(message, "no inherited document");
        }
        other => panic!("unexpected inspect response: {other:?}"),
    }
    let _ = child.child.kill();
    let _ = child.child.wait();
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(output_path);
}

#[test]
fn document_bound_pool_job_replaces_and_then_scrubs_worker() {
    let invalidated = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_invalidated = invalidated.clone();
    let pool = Arc::new(
        UtilityPool::new_with_replacement_hook(env!("CARGO_BIN_EXE_worker"), 1, move |identity| {
            observed_invalidated.lock().unwrap().push(identity)
        })
        .unwrap(),
    );
    let path = std::env::temp_dir().join(format!(
        "pdf-platform-utility-pool-document-{}.bin",
        std::process::id()
    ));
    std::fs::write(&path, b"brokered bytes").unwrap();
    let document = Arc::new(std::fs::File::open(&path).unwrap());
    let prepared_identity = Arc::new(std::sync::Mutex::new(None));
    let observed_identity = prepared_identity.clone();
    let executor = pool.clone();
    let scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        executor
            .execute_prepared_with_document(spec, context, &document, None, |preparation| {
                *observed_identity.lock().unwrap() = Some(preparation.identity);
                Ok(Vec::new())
            })
            .map(|_| ())
    })
    .unwrap();
    scheduler
        .submit(JobGraph::new(vec![JobSpec::new(19, "noop", JobPriority::UserInitiated)]).unwrap())
        .unwrap();
    loop {
        match scheduler.recv_event_timeout(Duration::from_secs(5)) {
            Some(JobEvent::Completed { job: 19 }) => break,
            Some(JobEvent::Failed { message, .. }) => panic!("utility job failed: {message}"),
            Some(_) => {}
            None => panic!("timed out waiting for utility job"),
        }
    }
    scheduler.shutdown();
    let identity = prepared_identity.lock().unwrap().unwrap();
    assert_eq!(identity.generation, 1);
    assert_eq!(
        invalidated.lock().unwrap().as_slice(),
        &[
            jobs::utility_pool::UtilityWorkerIdentity {
                slot: 0,
                generation: 0
            },
            jobs::utility_pool::UtilityWorkerIdentity {
                slot: 0,
                generation: 1
            },
        ]
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn thumbnail_crosses_document_bound_utility_process() {
    use protocol::utility_thumbnails::{
        decode_thumbnail_result, encode_thumbnail_request, ThumbnailRequest,
    };

    let path = std::env::temp_dir().join(format!(
        "pdf-platform-utility-thumbnail-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, one_page_pdf()).unwrap();
    let document = Arc::new(std::fs::File::open(&path).unwrap());
    let pool = Arc::new(UtilityPool::new(env!("CARGO_BIN_EXE_worker"), 1).unwrap());
    let result = Arc::new(std::sync::Mutex::new(None));
    let observed = result.clone();
    let executor = pool.clone();
    let scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        let bytes =
            executor.execute_prepared_with_document(spec, context, &document, None, |_| {
                let request = ThumbnailRequest {
                    page: 0,
                    width: 16,
                    height: 16,
                    scale: 0.25,
                    generation: 6,
                    revision: 4,
                };
                Ok(vec![
                    UtilityJobInput::Inline(encode_thumbnail_request(&request).unwrap()),
                    UtilityJobInput::SharedMemory {
                        grant_id: [3; 16],
                        offset: 0,
                        length: 16 * 16 * 4,
                    },
                ])
            })?;
        *observed.lock().unwrap() = Some(bytes);
        Ok(())
    })
    .unwrap();
    scheduler
        .submit(
            JobGraph::new(vec![JobSpec::new(
                20,
                "thumbnail",
                JobPriority::Maintenance,
            )])
            .unwrap(),
        )
        .unwrap();
    loop {
        match scheduler.recv_event_timeout(Duration::from_secs(10)) {
            Some(JobEvent::Completed { job: 20 }) => break,
            Some(JobEvent::Failed { message, .. }) => panic!("thumbnail failed: {message}"),
            Some(_) => {}
            None => panic!("timed out waiting for thumbnail"),
        }
    }
    scheduler.shutdown();
    let bytes = result.lock().unwrap().take().unwrap();
    let thumbnail = decode_thumbnail_result(&bytes).unwrap();
    assert_eq!(thumbnail.page, 0);
    assert!(thumbnail.is_current(6, 4));
    let _ = std::fs::remove_file(path);
}

#[test]
fn ocr_job_reaches_the_dispatch_handler_not_the_unsupported_fallback() {
    use ocr_bridge::{
        decode_utility_result, encode_utility_request, OcrUtilityRequest, PreprocessOptions,
    };

    let path = std::env::temp_dir().join(format!(
        "pdf-platform-utility-ocr-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, one_page_pdf()).unwrap();
    let document = Arc::new(std::fs::File::open(&path).unwrap());
    let pool = Arc::new(UtilityPool::new(env!("CARGO_BIN_EXE_worker"), 1).unwrap());
    let result = Arc::new(std::sync::Mutex::new(None));
    let observed = result.clone();
    let executor = pool.clone();
    let scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        let bytes =
            executor.execute_prepared_with_document(spec, context, &document, None, |_| {
                let request = OcrUtilityRequest {
                    page_index: 0,
                    language: "eng".into(),
                    options: PreprocessOptions::default(),
                };
                Ok(vec![UtilityJobInput::Inline(
                    encode_utility_request(&request).unwrap(),
                )])
            })?;
        *observed.lock().unwrap() = Some(bytes);
        Ok(())
    })
    .unwrap();
    scheduler
        .submit(JobGraph::new(vec![JobSpec::new(21, "ocr", JobPriority::Maintenance)]).unwrap())
        .unwrap();
    // Dispatch-wiring check, not a Tesseract-installed check: whichever way the
    // worker's real OCR engine resolves, it must not be the "unsupported
    // operation" fallback that a missing match arm would produce.
    loop {
        match scheduler.recv_event_timeout(Duration::from_secs(10)) {
            Some(JobEvent::Completed { job: 21 }) => break,
            Some(JobEvent::Failed { job: 21, message }) => {
                assert!(
                    !message.contains("unsupported utility operation"),
                    "ocr job hit the fallback branch: {message}"
                );
                scheduler.shutdown();
                let _ = std::fs::remove_file(path);
                return;
            }
            Some(_) => {}
            None => panic!("timed out waiting for ocr job"),
        }
    }
    scheduler.shutdown();
    let bytes = result.lock().unwrap().take().unwrap();
    let ocr_result = decode_utility_result(&bytes).unwrap();
    assert_eq!(ocr_result.page_index, 0);
    let _ = std::fs::remove_file(path);
}

fn one_page_pdf() -> Vec<u8> {
    use std::io::Write as _;

    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for object in [
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
    ] {
        offsets.push(bytes.len());
        bytes.extend_from_slice(object.as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for offset in offsets {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        bytes,
        "trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    )
    .unwrap();
    bytes
}

#[test]
fn noop_job_crosses_real_utility_process_boundary() {
    let pool = Arc::new(UtilityPool::new(env!("CARGO_BIN_EXE_worker"), 1).unwrap());
    let executor = pool.clone();
    let scheduler =
        JobScheduler::new_typed(1, 1, move |spec, context| executor.execute(spec, context))
            .unwrap();
    scheduler
        .submit(
            JobGraph::new(vec![
                JobSpec::new(1, "noop", JobPriority::UserInitiated).idempotent()
            ])
            .unwrap(),
        )
        .unwrap();

    loop {
        match scheduler.recv_event_timeout(Duration::from_secs(5)) {
            Some(JobEvent::Completed { job: 1 }) => break,
            Some(JobEvent::Failed { message, .. }) => panic!("utility job failed: {message}"),
            Some(_) => {}
            None => panic!("timed out waiting for utility job"),
        }
    }
    scheduler.shutdown();
}

#[test]
fn utility_pool_returns_operation_output() {
    let pool = UtilityPool::new(env!("CARGO_BIN_EXE_worker"), 1).unwrap();
    let output = Arc::new(std::sync::Mutex::new(None));
    let observed = output.clone();
    let scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        let bytes = pool.execute_prepared_result(spec, context, |_| Ok(Vec::new()))?;
        *observed.lock().unwrap() = Some(bytes);
        Ok(())
    })
    .unwrap();
    scheduler
        .submit(JobGraph::new(vec![JobSpec::new(7, "noop", JobPriority::UserInitiated)]).unwrap())
        .unwrap();
    loop {
        match scheduler.recv_event_timeout(Duration::from_secs(5)) {
            Some(JobEvent::Completed { job: 7 }) => break,
            Some(JobEvent::Failed { message, .. }) => panic!("utility job failed: {message}"),
            Some(_) => {}
            None => panic!("timed out waiting for utility job"),
        }
    }
    scheduler.shutdown();
    assert_eq!(*output.lock().unwrap(), Some(Vec::new()));
}

#[test]
fn inputs_are_prepared_for_the_selected_worker_generation() {
    let pool = UtilityPool::new(env!("CARGO_BIN_EXE_worker"), 1).unwrap();
    let selected = Arc::new(std::sync::Mutex::new(None));
    let observed = selected.clone();
    let context_scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        pool.execute_prepared(spec, context, |preparation| {
            preparation.shared_memory[..4].copy_from_slice(b"test");
            *observed.lock().unwrap() = Some(preparation.identity);
            Ok(Vec::new())
        })
    })
    .unwrap();
    context_scheduler
        .submit(JobGraph::new(vec![JobSpec::new(2, "noop", JobPriority::UserInitiated)]).unwrap())
        .unwrap();
    loop {
        if matches!(
            context_scheduler.recv_event_timeout(Duration::from_secs(5)),
            Some(JobEvent::Completed { job: 2 })
        ) {
            break;
        }
    }
    context_scheduler.shutdown();
    let identity = selected.lock().unwrap().expect("worker identity");
    assert_eq!(identity.slot, 0);
    assert_eq!(identity.generation, 0);
}

#[test]
fn replacing_worker_notifies_old_generation() {
    let invalidated = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = invalidated.clone();
    let pool =
        UtilityPool::new_with_replacement_hook(env!("CARGO_BIN_EXE_worker"), 1, move |identity| {
            observed.lock().unwrap().push(identity)
        })
        .unwrap();
    pool.restart_worker(0).unwrap();
    let identities = invalidated.lock().unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].slot, 0);
    assert_eq!(identities[0].generation, 0);
}

#[test]
fn replacing_worker_revokes_its_broker_grants() {
    use coordinator::broker::{
        utility_grant_revocation_hook, UtilityGrantError, UtilityGrantKind, UtilityGrantRegistry,
    };

    let grants = Arc::new(std::sync::Mutex::new(UtilityGrantRegistry::new()));
    let pool = Arc::new(
        UtilityPool::new_with_replacement_hook(
            env!("CARGO_BIN_EXE_worker"),
            1,
            utility_grant_revocation_hook(grants.clone()),
        )
        .unwrap(),
    );
    let issued = Arc::new(std::sync::Mutex::new(None));
    let executor_pool = pool.clone();
    let executor_grants = grants.clone();
    let executor_issued = issued.clone();
    let scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        executor_pool.execute_prepared(spec.clone(), context, |preparation| {
            let worker = preparation.identity;
            let grant = executor_grants
                .lock()
                .unwrap()
                .issue(
                    UtilityGrantKind::SharedMemoryRead,
                    spec.id,
                    worker,
                    16,
                    Duration::from_secs(60),
                )
                .map_err(|error| jobs::JobRunError::Execution(format!("{error:?}")))?;
            *executor_issued.lock().unwrap() = Some((grant, worker));
            Ok(vec![UtilityJobInput::SharedMemory {
                grant_id: grant,
                offset: 0,
                length: 16,
            }])
        })
    })
    .unwrap();
    scheduler
        .submit(JobGraph::new(vec![JobSpec::new(3, "noop", JobPriority::UserInitiated)]).unwrap())
        .unwrap();
    loop {
        if matches!(
            scheduler.recv_event_timeout(Duration::from_secs(5)),
            Some(JobEvent::Completed { job: 3 })
        ) {
            break;
        }
    }
    scheduler.shutdown();
    let (grant, worker) = issued.lock().unwrap().expect("issued grant");
    pool.restart_worker(worker.slot).unwrap();
    assert_eq!(
        grants
            .lock()
            .unwrap()
            .validate(grant, UtilityGrantKind::SharedMemoryRead, 3, worker, 0, 1,),
        Err(UtilityGrantError::Unknown)
    );
}

#[test]
fn worker_rejects_shared_memory_descriptor_past_slot_bound() {
    let pool = Arc::new(
        UtilityPool::new_with_capacity_and_hook(env!("CARGO_BIN_EXE_worker"), 1, 64, |_| {})
            .unwrap(),
    );
    let executor = pool.clone();
    let scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        executor.execute_prepared(spec, context, |_preparation| {
            Ok(vec![UtilityJobInput::SharedMemory {
                grant_id: [1; 16],
                offset: 60,
                length: 8,
            }])
        })
    })
    .unwrap();
    scheduler
        .submit(JobGraph::new(vec![JobSpec::new(4, "noop", JobPriority::UserInitiated)]).unwrap())
        .unwrap();
    loop {
        match scheduler.recv_event_timeout(Duration::from_secs(5)) {
            Some(JobEvent::Failed { job: 4, message }) => {
                assert!(message.contains("out of bounds"));
                break;
            }
            Some(JobEvent::Completed { job: 4 }) => panic!("out-of-bounds job completed"),
            Some(_) => {}
            None => panic!("timed out waiting for bounded-input rejection"),
        }
    }
    scheduler.shutdown();
}
