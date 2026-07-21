use std::sync::Arc;
use std::time::Duration;

use jobs::utility_pool::UtilityPool;
use jobs::{JobEvent, JobGraph, JobPriority, JobScheduler, JobSpec};

#[test]
fn noop_job_crosses_real_utility_process_boundary() {
    let pool = Arc::new(UtilityPool::new(env!("CARGO_BIN_EXE_worker"), 1).unwrap());
    let executor = pool.clone();
    let scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        executor.execute(spec, context)
    })
    .unwrap();
    scheduler
        .submit(
            JobGraph::new(vec![
                JobSpec::new(1, "noop", JobPriority::UserInitiated).idempotent(),
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
