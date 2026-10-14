//! A bounded three-stage pipeline for publishing tracks: prepare one item at a time, in
//! order, while earlier items upload and wait for their conversion.
//!
//! Each stage has its own bound: preparation runs at most [`Limits::ahead`] items ahead of
//! the uploads, at most [`Limits::uploads`] items upload at once and at most
//! [`Limits::waits`] wait at once. Results keep the order of the items. The first failure
//! stops everything: no further item is prepared, running stages see [`Halt::halted`] and
//! pending tasks are aborted.
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use yoto_domain::{Failure, Result};

/// Concurrency bounds of a pipeline.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Items prepared but not yet uploading.
    pub ahead: usize,
    /// Items uploading at once.
    pub uploads: usize,
    /// Items waiting for their conversion at once.
    pub waits: usize,
}
/// Publication: encoding stays one at a time but runs ahead; two uploads share the
/// connection; three conversions are followed on Yoto at once.
pub const PUBLISH: Limits = Limits {
    ahead: 2,
    uploads: 2,
    waits: 3,
};
/// How often a waiting pipeline looks at its stop flags.
const WATCH: Duration = Duration::from_millis(100);

/// Whether the pipeline must stop: the caller's cancellation or a failure inside it.
#[derive(Clone)]
pub struct Halt {
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}
impl Halt {
    pub fn halted(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) || self.stop.load(Ordering::SeqCst)
    }
    /// Both flags, for child processes that must end when either is raised.
    pub fn flags(&self) -> Vec<Arc<AtomicBool>> {
        vec![self.cancel.clone(), self.stop.clone()]
    }
    fn raise(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    fn check(&self) -> Result<()> {
        if self.halted() {
            Err(Failure::Cancelled)
        } else {
            Ok(())
        }
    }
    /// Wait for a permit, giving up when the pipeline stops.
    async fn acquire(&self, gate: &Arc<Semaphore>) -> Result<OwnedSemaphorePermit> {
        loop {
            self.check()?;
            tokio::select! {
                permit = gate.clone().acquire_owned() => {
                    return permit.map_err(|_| Failure::Cancelled);
                }
                _ = tokio::time::sleep(WATCH) => {}
            }
        }
    }
}
/// What the last stage may use again: an upload slot, to send a file once more.
#[derive(Clone)]
pub struct Gates {
    uploads: Arc<Semaphore>,
    halt: Halt,
}
impl Gates {
    /// Hold an upload slot for as long as the returned permit lives.
    pub async fn upload(&self) -> Result<OwnedSemaphorePermit> {
        self.halt.acquire(&self.uploads).await
    }
    pub fn halt(&self) -> &Halt {
        &self.halt
    }
}

/// Keep the most useful failure: a real one over a cancellation it caused, then the
/// earliest item's.
fn keep(failure: &mut Option<(usize, Failure)>, index: usize, error: Failure) {
    let better = match failure {
        None => true,
        Some((at, existing)) => match (matches!(existing, Failure::Cancelled), &error) {
            (true, Failure::Cancelled) => false,
            (true, _) => true,
            (false, Failure::Cancelled) => false,
            (false, _) => index < *at,
        },
    };
    if better {
        *failure = Some((index, error));
    }
}

/// Run every item through `prepare` (in order, one at a time), then `send` under an
/// upload slot, then `finish` under a wait slot. Returns the outputs in item order, or
/// the first item's real failure once every stage has stopped.
pub async fn run<T, P, S, O, Prepare, Prepared, Send, Sent, Finish, Finished>(
    items: Vec<T>,
    limits: Limits,
    cancel: Arc<AtomicBool>,
    mut prepare: Prepare,
    send: Send,
    finish: Finish,
) -> Result<Vec<O>>
where
    P: std::marker::Send + 'static,
    S: std::marker::Send + 'static,
    O: std::marker::Send + 'static,
    Prepare: FnMut(T, Halt) -> Prepared,
    Prepared: Future<Output = Result<P>>,
    Send: Fn(P, Halt) -> Sent + Clone + std::marker::Send + Sync + 'static,
    Sent: Future<Output = Result<S>> + std::marker::Send + 'static,
    Finish: Fn(S, Gates) -> Finished + Clone + std::marker::Send + Sync + 'static,
    Finished: Future<Output = Result<O>> + std::marker::Send + 'static,
{
    let halt = Halt {
        cancel,
        stop: Arc::new(AtomicBool::new(false)),
    };
    let ahead = Arc::new(Semaphore::new(limits.ahead.max(1)));
    let uploads = Arc::new(Semaphore::new(limits.uploads.max(1)));
    let waits = Arc::new(Semaphore::new(limits.waits.max(1)));
    let mut outputs: Vec<Option<O>> = items.iter().map(|_| None).collect();
    let mut failure: Option<(usize, Failure)> = None;
    let mut tasks = tokio::task::JoinSet::new();
    let mut record = |done: std::result::Result<(usize, Result<O>), tokio::task::JoinError>,
                      failure: &mut Option<(usize, Failure)>| {
        match done {
            Ok((index, Ok(output))) => outputs[index] = Some(output),
            Ok((index, Err(error))) => keep(failure, index, error),
            // An aborted task already left its failure behind.
            Err(error) if error.is_cancelled() => {}
            Err(error) => keep(failure, usize::MAX, Failure::Storage(error.to_string())),
        }
    };
    for (index, item) in items.into_iter().enumerate() {
        while let Some(done) = tasks.try_join_next() {
            record(done, &mut failure);
        }
        if failure.is_some() || halt.halted() {
            break;
        }
        let slot = match halt.acquire(&ahead).await {
            Ok(slot) => slot,
            Err(error) => {
                keep(&mut failure, index, error);
                break;
            }
        };
        let prepared = match prepare(item, halt.clone()).await {
            Ok(prepared) => prepared,
            Err(error) => {
                keep(&mut failure, index, error);
                halt.raise();
                break;
            }
        };
        let (send, finish, halt, uploads, waits) = (
            send.clone(),
            finish.clone(),
            halt.clone(),
            uploads.clone(),
            waits.clone(),
        );
        tasks.spawn(async move {
            let result = async {
                let upload = halt.acquire(&uploads).await?;
                drop(slot);
                let sent = send(prepared, halt.clone()).await?;
                drop(upload);
                let _wait = halt.acquire(&waits).await?;
                halt.check()?;
                finish(
                    sent,
                    Gates {
                        uploads,
                        halt: halt.clone(),
                    },
                )
                .await
            }
            .await;
            if result.is_err() {
                halt.raise();
            }
            (index, result)
        });
    }
    // Drain the stages, aborting what remains as soon as anything stops the pipeline.
    while !tasks.is_empty() {
        if failure.is_some() || halt.halted() {
            halt.raise();
            tasks.abort_all();
        }
        tokio::select! {
            Some(done) = tasks.join_next() => record(done, &mut failure),
            _ = tokio::time::sleep(WATCH) => {}
        }
    }
    if let Some((_, error)) = failure {
        return Err(error);
    }
    if halt.halted() {
        return Err(Failure::Cancelled);
    }
    outputs
        .into_iter()
        .map(|output| output.ok_or(Failure::Cancelled))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    /// Tracks how many calls run at once and the highest count reached.
    #[derive(Default)]
    struct Load {
        now: Mutex<(usize, usize)>,
    }
    impl Load {
        fn enter(&self) {
            let mut n = self.now.lock().unwrap();
            n.0 += 1;
            n.1 = n.1.max(n.0);
        }
        fn leave(&self) {
            self.now.lock().unwrap().0 -= 1;
        }
        fn peak(&self) -> usize {
            self.now.lock().unwrap().1
        }
    }
    struct Probe {
        prepared: Mutex<Vec<usize>>,
        uploads: Load,
        waits: Load,
        /// Items prepared but not yet uploading, sampled at each preparation.
        backlog: Mutex<(usize, usize)>,
    }
    fn probe() -> Arc<Probe> {
        Arc::new(Probe {
            prepared: Mutex::new(vec![]),
            uploads: Load::default(),
            waits: Load::default(),
            backlog: Mutex::new((0, 0)),
        })
    }
    /// Delays that vary per item so later items can overtake earlier ones.
    fn delay(index: usize, salt: usize) -> Duration {
        Duration::from_millis(((index * 7 + salt * 3) % 5) as u64 * 4 + 2)
    }
    async fn pipeline(
        items: usize,
        fail: Option<usize>,
        probe: Arc<Probe>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Vec<usize>> {
        let (send_probe, finish_probe, prepare_probe) = (probe.clone(), probe.clone(), probe);
        run(
            (0..items).collect(),
            PUBLISH,
            cancel,
            move |index, halt: Halt| {
                let probe = prepare_probe.clone();
                async move {
                    halt.check()?;
                    tokio::time::sleep(delay(index, 1)).await;
                    probe.prepared.lock().unwrap().push(index);
                    let mut backlog = probe.backlog.lock().unwrap();
                    backlog.0 += 1;
                    backlog.1 = backlog.1.max(backlog.0);
                    Ok(index)
                }
            },
            move |index, _halt| {
                let probe = send_probe.clone();
                async move {
                    probe.backlog.lock().unwrap().0 -= 1;
                    probe.uploads.enter();
                    tokio::time::sleep(delay(index, 2)).await;
                    probe.uploads.leave();
                    Ok(index)
                }
            },
            move |index, gates: Gates| {
                let probe = finish_probe.clone();
                async move {
                    probe.waits.enter();
                    tokio::time::sleep(delay(index, 3)).await;
                    if index % 4 == 1 {
                        // A stalled conversion sends its file once more.
                        let _slot = gates.upload().await?;
                        tokio::time::sleep(delay(index, 4)).await;
                    }
                    probe.waits.leave();
                    if Some(index) == fail {
                        return Err(Failure::Connection);
                    }
                    Ok(index * 10)
                }
            },
        )
        .await
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn outputs_keep_item_order_within_every_bound() {
        let probe = probe();
        let out = pipeline(12, None, probe.clone(), Arc::default())
            .await
            .unwrap();
        assert_eq!(out, (0..12).map(|i| i * 10).collect::<Vec<_>>());
        assert_eq!(*probe.prepared.lock().unwrap(), (0..12).collect::<Vec<_>>());
        assert!(probe.uploads.peak() <= PUBLISH.uploads);
        assert!(probe.waits.peak() <= PUBLISH.waits);
        assert!(probe.backlog.lock().unwrap().1 <= PUBLISH.ahead);
        // The stages really overlapped.
        assert!(probe.waits.peak() > 1);
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_failure_stops_preparing_and_is_reported() {
        let probe = probe();
        let error = pipeline(40, Some(3), probe.clone(), Arc::default())
            .await
            .unwrap_err();
        assert!(matches!(error, Failure::Connection), "{error:?}");
        assert!(probe.prepared.lock().unwrap().len() < 40);
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancellation_stops_every_stage() {
        let probe = probe();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            flag.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        let error = pipeline(200, None, probe.clone(), cancel)
            .await
            .unwrap_err();
        assert!(matches!(error, Failure::Cancelled), "{error:?}");
        assert!(probe.prepared.lock().unwrap().len() < 200);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn a_real_failure_wins_over_the_cancellations_it_caused() {
        let mut failure = None;
        keep(&mut failure, 5, Failure::Cancelled);
        keep(&mut failure, 7, Failure::Connection);
        keep(&mut failure, 2, Failure::Cancelled);
        keep(&mut failure, 6, Failure::Ambiguous);
        assert!(matches!(failure, Some((6, Failure::Ambiguous))));
    }
}
