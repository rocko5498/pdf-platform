use std::sync::Arc;
use std::time::Duration;

use jobs::utility_pool::UtilityPool;
use jobs::{JobEvent, JobGraph, JobPriority, JobScheduler, JobSpec};
use protocol::utility_jobs::UtilityJobInput;

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
