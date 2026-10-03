//! The queue: the jobs in memory, in order, behind one `tokio` mutex, with
//! `jobs/` as their write-ahead record — each transition is written to the
//! job's file, then applied in memory, then published, all under the lock,
//! so the file never trails what a client was told and the events are in
//! the order the states were entered.
//!
//! Two lanes, one worker each: runs, through `Launcher::execute`, and
//! downloads, through `Registry::download`. A worker is spawned when its lane
//! has a queued job and no worker, and ends when the lane has none left; so
//! the run lane never executes two runs at once — a run measures latency, and
//! two sharing a machine would measure contention (the design document § 7).

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use ragondin_experiments::{lower_median, Run, RunId, RunStore, Trace, TraceDocument};
use ragondin_types::QueryId;
use tokio::sync::{broadcast, mpsc, Mutex, MutexGuard};

use super::{file, now, summary, Job, JobState, RunIdMismatch, Transition, Work, INTERRUPTED};
use crate::backends::{
    Backends, Cancellation, DownloadProgress, Launcher, LauncherError, ProgressSink, QueryProgress,
    Registry, RunObserver, Submission,
};
use crate::error::ApiError;
use crate::response::{JobFault, JobListing, JobSummary};

/// How many recent events the queue keeps for a client that reconnects,
/// and how many a slow client may fall behind before it is resynchronised.
const RECENT: usize = 1024;

/// The two lanes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lane {
    Runs,
    Downloads,
}

impl Lane {
    fn of(work: &Work) -> Self {
        match work {
            Work::Run { .. } => Self::Runs,
            Work::Download { .. } => Self::Downloads,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Runs => 0,
            Self::Downloads => 1,
        }
    }
}

/// One event of the stream: its sequence number, its name, and its data as
/// JSON, serialized once for every client.
#[derive(Debug)]
pub(crate) struct Published {
    pub(crate) seq: u64,
    pub(crate) name: &'static str,
    pub(crate) data: String,
}

/// The queue, shared by every handler and both workers.
pub(crate) struct Queue {
    dir: PathBuf,
    runs: Arc<dyn RunStore>,
    launcher: Arc<dyn Launcher>,
    registry: Arc<dyn Registry>,
    state: Mutex<State>,
    events: broadcast::Sender<Arc<Published>>,
    /// Names this process's events, so an id from another process's stream
    /// is never read as one of this one's.
    boot: String,
    /// The tail of a job id, so two accepted in one millisecond differ.
    counter: AtomicU64,
}

struct State {
    /// Every job, by position.
    jobs: Vec<Entry>,
    faults: Vec<JobFault>,
    /// Whether each lane has a worker.
    busy: [bool; 2],
    /// The most recent events, oldest first.
    recent: VecDeque<Arc<Published>>,
    /// The last event's sequence number; 0 before the first.
    seq: u64,
}

struct Entry {
    job: Job,
    cancel: Cancellation,
}

impl State {
    fn find(&self, id: &str) -> Result<usize, ApiError> {
        self.jobs
            .iter()
            .position(|entry| entry.job.id == id)
            .ok_or_else(|| ApiError::JobNotFound { id: id.to_owned() })
    }

    /// The queued job of `lane` its worker takes next: the lowest position.
    fn next(&self, lane: Lane) -> Option<usize> {
        self.jobs
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.job.state == JobState::Queued && Lane::of(&entry.job.work) == lane
            })
            .min_by_key(|(_, entry)| entry.job.position)
            .map(|(index, _)| index)
    }

    fn listing(&self) -> JobListing {
        JobListing {
            jobs: self.jobs.iter().map(|entry| summary(&entry.job)).collect(),
            faults: self.faults.clone(),
        }
    }

    fn sort(&mut self) {
        self.jobs.sort_by_key(|entry| entry.job.position);
    }
}

impl Queue {
    /// The queue over `dir`, read back from it: every job found `Running`
    /// is failed as interrupted — no process runs it now — and the queued
    /// ones wait in their stored order. When called inside a `tokio`
    /// runtime, as the binary does, the workers start on them at once; a
    /// lane without a runtime starts at its next submission.
    pub(crate) fn open(dir: PathBuf, backends: &Backends) -> Arc<Self> {
        let (jobs, mut faults) = file::load(&dir);
        let mut entries = Vec::with_capacity(jobs.len());
        for mut job in jobs {
            if matches!(job.state, JobState::Running { .. }) {
                let at = now();
                job.state = JobState::Failed {
                    error: INTERRUPTED.to_owned(),
                    at_node: None,
                    finished_at: at,
                };
                job.history.push(Transition {
                    state: job.state.name().to_owned(),
                    at,
                });
                // Not running either way: a failed write is reported, and the
                // next start finds it `Running` and fails it again.
                if let Err(reason) = file::write(&dir, &job) {
                    faults.push(fault(&dir, &job.id, reason));
                }
            }
            entries.push(Entry {
                job,
                cancel: Cancellation::new(),
            });
        }
        let (events, _) = broadcast::channel(RECENT);
        let queue = Arc::new(Self {
            dir,
            runs: Arc::clone(&backends.runs),
            launcher: Arc::clone(&backends.launcher),
            registry: Arc::clone(&backends.registry),
            state: Mutex::new(State {
                jobs: entries,
                faults,
                busy: [false; 2],
                recent: VecDeque::with_capacity(RECENT),
                seq: 0,
            }),
            events,
            boot: now().map_or(0, |at| at.get()).to_string(),
            counter: AtomicU64::new(0),
        });
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let queue = Arc::clone(&queue);
            runtime.spawn(async move {
                let mut state = queue.state.lock().await;
                for lane in [Lane::Runs, Lane::Downloads] {
                    queue.kick(&mut state, lane);
                }
            });
        }
        queue
    }

    /// Every job, by position, and the record's faults.
    pub(crate) async fn listing(&self) -> JobListing {
        self.state.lock().await.listing()
    }

    /// One job.
    pub(crate) async fn job(&self, id: &str) -> Result<JobSummary, ApiError> {
        let state = self.state.lock().await;
        let index = state.find(id)?;
        Ok(summary(&state.jobs[index].job))
    }

    /// Queues `submission` under the id `run_id` announced for it — unless a
    /// job not yet ended holds that id, or the store does: `run_exists`,
    /// linking whichever does. The check and the queueing are one step under
    /// the lock, so two submissions of one id cannot both be accepted.
    pub(crate) async fn submit_run(
        self: &Arc<Self>,
        submission: Submission,
        run_id: RunId,
    ) -> Result<String, ApiError> {
        let announced = run_id.to_string();
        let mut state = self.state.lock().await;
        let holder = state.jobs.iter().find(|entry| {
            !entry.job.state.is_terminal()
                && matches!(&entry.job.work, Work::Run { run_id, .. } if *run_id == announced)
        });
        if let Some(holder) = holder {
            return Err(ApiError::RunExists {
                link: format!("/api/v1/jobs/{}", holder.job.id),
                run_id: announced,
            });
        }
        let runs = Arc::clone(&self.runs);
        let stored = tokio::task::spawn_blocking(move || runs.ids())
            .await
            .map_err(|error| ApiError::BackendFailed {
                detail: format!("the run store call did not complete: {error}"),
            })?
            .map_err(|error| ApiError::BackendFailed {
                detail: format!("the run store cannot be listed: {error}"),
            })?;
        if stored.contains(&run_id) {
            return Err(ApiError::RunExists {
                link: format!("/api/v1/runs/{announced}"),
                run_id: announced,
            });
        }
        let work = Work::Run {
            run_id: announced,
            pipeline_name: submission.pipeline_name,
            pipeline: submission.pipeline,
            benchmark: submission.benchmark,
            bindings: submission.bindings,
            up_to: submission.up_to,
        };
        self.enqueue(&mut state, work).await
    }

    /// Queues a download of `benchmark` on the download lane.
    pub(crate) async fn submit_download(
        self: &Arc<Self>,
        benchmark: String,
    ) -> Result<String, ApiError> {
        let mut state = self.state.lock().await;
        self.enqueue(&mut state, Work::Download { benchmark }).await
    }

    async fn enqueue(
        self: &Arc<Self>,
        state: &mut MutexGuard<'_, State>,
        work: Work,
    ) -> Result<String, ApiError> {
        let created_at = now();
        let id = loop {
            let id = format!(
                "{}-{}",
                created_at.map_or(0, |at| at.get()),
                self.counter.fetch_add(1, Ordering::Relaxed) + 1
            );
            if state.find(&id).is_err() {
                break id;
            }
        };
        let lane = Lane::of(&work);
        let job = Job {
            id: id.clone(),
            position: state
                .jobs
                .iter()
                .map(|entry| entry.job.position)
                .max()
                .map_or(0, |last| last + 1),
            created_at,
            work,
            state: JobState::Queued,
            history: vec![Transition {
                state: JobState::Queued.name().to_owned(),
                at: created_at,
            }],
        };
        self.write(&job)
            .await
            .map_err(|detail| ApiError::BackendFailed { detail })?;
        state.jobs.push(Entry {
            job: job.clone(),
            cancel: Cancellation::new(),
        });
        state.sort();
        self.publish(state, job.state.name(), &summary(&job));
        self.kick(state, lane);
        Ok(id)
    }

    /// Cancels the job `id`: a queued one at once, never executed; the
    /// running one by setting its signal, which the launcher honours between
    /// queries — until then it stays `running`.
    pub(crate) async fn cancel(&self, id: &str) -> Result<JobSummary, ApiError> {
        let mut state = self.state.lock().await;
        let index = state.find(id)?;
        let entry = &state.jobs[index];
        match &entry.job.state {
            JobState::Queued => {
                let mut job = entry.job.clone();
                let at = now();
                job.state = JobState::Cancelled { finished_at: at };
                job.history.push(Transition {
                    state: job.state.name().to_owned(),
                    at,
                });
                self.write(&job)
                    .await
                    .map_err(|detail| ApiError::BackendFailed { detail })?;
                let view = summary(&job);
                state.jobs[index].job = job;
                self.publish(&mut state, "cancelled", &view);
                Ok(view)
            }
            JobState::Running { .. } => {
                entry.cancel.cancel();
                Ok(summary(&entry.job))
            }
            ended => Err(ApiError::JobFinished {
                id: id.to_owned(),
                state: ended.name().to_owned(),
            }),
        }
    }

    /// Moves the queued job `id` to `position` among its lane's queued jobs
    /// — 0 first, past the last moves it last. The positions those jobs held
    /// are dealt out again in the new order and every changed file written
    /// before memory changes, so the order survives a restart.
    pub(crate) async fn reorder(&self, id: &str, position: u64) -> Result<JobListing, ApiError> {
        let mut state = self.state.lock().await;
        let index = state.find(id)?;
        let moved = &state.jobs[index].job;
        if moved.state != JobState::Queued {
            return Err(ApiError::JobNotQueued {
                id: id.to_owned(),
                state: moved.state.name().to_owned(),
            });
        }
        let lane = Lane::of(&moved.work);
        let mut queued: Vec<usize> = (0..state.jobs.len())
            .filter(|&i| {
                state.jobs[i].job.state == JobState::Queued
                    && Lane::of(&state.jobs[i].job.work) == lane
            })
            .collect();
        queued.sort_by_key(|&i| state.jobs[i].job.position);
        let places: Vec<u64> = queued.iter().map(|&i| state.jobs[i].job.position).collect();
        queued.retain(|&i| i != index);
        let at = usize::try_from(position)
            .unwrap_or(usize::MAX)
            .min(queued.len());
        queued.insert(at, index);
        let changed: Vec<(usize, Job)> = queued
            .iter()
            .zip(places)
            .filter(|(&i, place)| state.jobs[i].job.position != *place)
            .map(|(&i, place)| {
                let mut job = state.jobs[i].job.clone();
                job.position = place;
                (i, job)
            })
            .collect();
        for (_, job) in &changed {
            if let Err(detail) = self.write(job).await {
                state.faults.push(fault(&self.dir, &job.id, format!(
                    "{detail}; the reorder was refused, and the files written before it keep their new positions"
                )));
                return Err(ApiError::BackendFailed { detail });
            }
        }
        for (i, job) in changed {
            let view = summary(&job);
            state.jobs[i].job = job;
            self.publish(&mut state, "reordered", &view);
        }
        state.sort();
        Ok(state.listing())
    }

    /// The events after `last` — the id a reconnecting client last saw —
    /// when this process sent it and still holds every event since;
    /// otherwise one `resync` event carrying the whole queue. And the
    /// receiver of every event after those. Both are taken under the lock
    /// every publication holds, so nothing falls between them.
    pub(crate) async fn subscribe(
        &self,
        last: Option<&str>,
    ) -> (Vec<Arc<Published>>, broadcast::Receiver<Arc<Published>>) {
        let state = self.state.lock().await;
        let receiver = self.events.subscribe();
        let since = last
            .and_then(|last| last.rsplit_once(':'))
            .filter(|(boot, _)| *boot == self.boot)
            .and_then(|(_, seq)| seq.parse::<u64>().ok())
            .filter(|&seq| seq <= state.seq)
            .filter(|&seq| match state.recent.front() {
                Some(oldest) => seq + 1 >= oldest.seq,
                None => true,
            });
        let backlog = match since {
            Some(since) => state
                .recent
                .iter()
                .filter(|event| event.seq > since)
                .cloned()
                .collect(),
            None => vec![self.resync(&state)],
        };
        (backlog, receiver)
    }

    /// A `resync` event: the whole queue, as of the last event.
    pub(crate) async fn resynchronised(&self) -> Arc<Published> {
        let state = self.state.lock().await;
        self.resync(&state)
    }

    fn resync(&self, state: &State) -> Arc<Published> {
        Arc::new(Published {
            seq: state.seq,
            name: "resync",
            data: to_json(&state.listing()),
        })
    }

    /// An event's id on the wire: this process's name and its number.
    pub(crate) fn event_id(&self, event: &Published) -> String {
        format!("{}:{}", self.boot, event.seq)
    }

    /// Publishes `data` as the event `name`, numbered after the last, kept
    /// among the recent ones. Called under the lock.
    fn publish(&self, state: &mut State, name: &'static str, data: &impl serde::Serialize) {
        state.seq += 1;
        let event = Arc::new(Published {
            seq: state.seq,
            name,
            data: to_json(data),
        });
        if state.recent.len() == RECENT {
            state.recent.pop_front();
        }
        state.recent.push_back(Arc::clone(&event));
        // No subscriber is not an error: the event is kept for one that
        // reconnects.
        let _ = self.events.send(event);
    }

    /// Writes `job`'s file on a blocking thread.
    async fn write(&self, job: &Job) -> Result<(), String> {
        let (dir, job) = (self.dir.clone(), job.clone());
        tokio::task::spawn_blocking(move || file::write(&dir, &job))
            .await
            .map_err(|error| format!("writing the job's file did not complete: {error}"))?
    }

    /// Starts `lane`'s worker when it has a queued job and none runs.
    fn kick(self: &Arc<Self>, state: &mut State, lane: Lane) {
        if state.busy[lane.index()] || state.next(lane).is_none() {
            return;
        }
        state.busy[lane.index()] = true;
        tokio::spawn(Arc::clone(self).work(lane));
    }

    /// A lane's worker: takes its next queued job, runs it to its end, and
    /// again, until the lane has none.
    async fn work(self: Arc<Self>, lane: Lane) {
        loop {
            let Some((job, cancel)) = self.start(lane).await else {
                return;
            };
            match lane {
                Lane::Runs => self.run(job, cancel).await,
                Lane::Downloads => self.download(job, cancel).await,
            }
        }
    }

    /// Moves `lane`'s next queued job to `running`, on disk first; `None`,
    /// and the worker marked gone, when the lane has none.
    async fn start(&self, lane: Lane) -> Option<(Job, Cancellation)> {
        loop {
            let mut state = self.state.lock().await;
            let Some(index) = state.next(lane) else {
                state.busy[lane.index()] = false;
                return None;
            };
            let mut job = state.jobs[index].job.clone();
            let at = now();
            job.state = JobState::Running {
                done: 0,
                total: None,
                started_at: at,
                median_latency_nanos: None,
            };
            job.history.push(Transition {
                state: job.state.name().to_owned(),
                at,
            });
            match self.write(&job).await {
                Ok(()) => {
                    let cancel = state.jobs[index].cancel.clone();
                    state.jobs[index].job = job.clone();
                    self.publish(&mut state, "running", &summary(&job));
                    return Some((job, cancel));
                }
                // Never executed without its record: failed, in memory, and
                // reported; the next start finds it queued and runs it.
                Err(reason) => {
                    let at = now();
                    job.state = JobState::Failed {
                        error: format!("the job could not be started: {reason}"),
                        at_node: None,
                        finished_at: at,
                    };
                    state.faults.push(fault(
                        &self.dir,
                        &job.id,
                        format!("{reason}; the job was failed without running, in memory only"),
                    ));
                    let view = summary(&job);
                    state.jobs[index].job = job;
                    self.publish(&mut state, "failed", &view);
                }
            }
        }
    }

    /// Runs a run job through the launcher, reporting each query as it
    /// completes, and files the run when it returns.
    async fn run(&self, job: Job, cancel: Cancellation) {
        let Some(submission) = job.work.submission() else {
            return;
        };
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let observer: Arc<dyn RunObserver> = Arc::new(Forward(sender));
        let launcher = Arc::clone(&self.launcher);
        let mut execution =
            Box::pin(async move { launcher.execute(&submission, observer, cancel).await });
        let mut tally = Tally::default();
        // Ticks first, so every query reported before execution returned is
        // applied before its end.
        let result = loop {
            tokio::select! {
                biased;
                Some(progress) = receiver.recv() => self.tick(&job.id, progress, &mut tally).await,
                result = &mut execution => break result,
            }
        };
        while let Ok(progress) = receiver.try_recv() {
            self.tick(&job.id, progress, &mut tally).await;
        }
        let at = now();
        let state = match result {
            Ok(run) => self.file(&job, run, &tally, at).await,
            Err(LauncherError::Cancelled) => {
                self.keep_partial(&job.id, &tally).await;
                JobState::Cancelled { finished_at: at }
            }
            Err(error) => {
                self.keep_partial(&job.id, &tally).await;
                let at_node = match &error {
                    LauncherError::Execution { at_node, .. } => at_node.clone(),
                    LauncherError::PipelineInvalid { node, .. } => node.clone(),
                    _ => None,
                };
                JobState::Failed {
                    error: error.to_string(),
                    at_node,
                    finished_at: at,
                }
            }
        };
        self.note_unread(&job.id, &tally).await;
        self.end(&job.id, state).await;
    }

    /// Files a run the launcher returned, under the id it carries — the one
    /// computed from what ran — as one block; the job records both ids when
    /// they differ. A store that refuses it fails the job, its traces kept
    /// as a partial run's are.
    async fn file(
        &self,
        job: &Job,
        run: Run,
        tally: &Tally,
        at: Option<ragondin_experiments::UnixMillis>,
    ) -> JobState {
        let decided = run.id.to_string();
        let announced = match &job.work {
            Work::Run { run_id, .. } => run_id.clone(),
            Work::Download { .. } => decided.clone(),
        };
        let runs = Arc::clone(&self.runs);
        let saved = tokio::task::spawn_blocking(move || runs.save(&run))
            .await
            .map_err(|error| format!("filing the run did not complete: {error}"))
            .and_then(|saved| {
                saved.map_err(|error| format!("the run could not be stored: {error}"))
            });
        match saved {
            Ok(()) => JobState::Done {
                id_mismatch: (decided != announced).then(|| RunIdMismatch {
                    announced,
                    decided: decided.clone(),
                }),
                run_id: Some(decided),
                finished_at: at,
            },
            Err(error) => {
                self.keep_partial(&job.id, tally).await;
                JobState::Failed {
                    error,
                    at_node: None,
                    finished_at: at,
                }
            }
        }
    }

    /// One query executed: its trace kept for a partial run, its latency —
    /// the trace's, by `ragondin-experiments`' one definition — added to the
    /// median, and the progress published. Held in memory, not written: a
    /// restart fails a running job whatever its count.
    async fn tick(&self, id: &str, progress: QueryProgress, tally: &mut Tally) {
        if let Some(latency) = Trace::try_from(&progress.trace)
            .ok()
            .and_then(|trace| trace.latency_nanos())
        {
            tally.latencies.push(latency);
        } else {
            // A trace whose latency does not read adds nothing to the median,
            // and says so once per job.
            tally.unread += 1;
        }
        tally.traces.insert(progress.query, progress.trace);
        let median = lower_median(tally.latencies.clone());
        self.progress(id, progress.position, progress.total, median)
            .await;
    }

    /// Publishes a running job's progress.
    async fn progress(&self, id: &str, done: u64, total: u64, median: Option<u64>) {
        let mut state = self.state.lock().await;
        let Ok(index) = state.find(id) else {
            return;
        };
        let JobState::Running {
            started_at,
            median_latency_nanos,
            ..
        } = state.jobs[index].job.state.clone()
        else {
            return;
        };
        state.jobs[index].job.state = JobState::Running {
            done,
            total: Some(total),
            started_at,
            median_latency_nanos: median.or(median_latency_nanos),
        };
        let view = summary(&state.jobs[index].job);
        self.publish(&mut state, "running", &view);
    }

    /// Writes a stopped run's traces under `jobs/<id>/partial/`; a failed
    /// write is reported among the faults.
    async fn keep_partial(&self, id: &str, tally: &Tally) {
        let (dir, id, traces) = (self.dir.clone(), id.to_owned(), tally.traces.clone());
        let written = {
            let (dir, id) = (dir.clone(), id.clone());
            tokio::task::spawn_blocking(move || file::write_partial(&dir, &id, &traces))
                .await
                .map_err(|error| format!("writing the partial traces did not complete: {error}"))
                .and_then(|written| written)
        };
        if let Err(reason) = written {
            self.state.lock().await.faults.push(JobFault {
                path: file::partial_path(&dir, &id).display().to_string(),
                reason,
            });
        }
    }

    /// Reports the traces whose latency did not read, which the live median
    /// left out.
    async fn note_unread(&self, id: &str, tally: &Tally) {
        if tally.unread > 0 {
            let reason = format!(
                "{} of its traces carry no latency that reads, and were left out of its median",
                tally.unread
            );
            self.state
                .lock()
                .await
                .faults
                .push(fault(&self.dir, id, reason));
        }
    }

    /// Runs a download job through the registry, its progress in bytes.
    async fn download(&self, job: Job, cancel: Cancellation) {
        let Work::Download { benchmark } = &job.work else {
            return;
        };
        let (sender, mut receiver) = mpsc::unbounded_channel::<DownloadProgress>();
        // The receiver outlives the download, so a send fails only once the
        // job has ended, when the progress no longer matters.
        let sink: ProgressSink = Arc::new(move |progress| {
            let _ = sender.send(progress);
        });
        let registry = Arc::clone(&self.registry);
        let name = benchmark.clone();
        let mut download =
            Box::pin(async move { registry.download(&name, sink, cancel.flag()).await });
        let mut published = None;
        let result = loop {
            tokio::select! {
                biased;
                Some(progress) = receiver.recv() => {
                    self.bytes(&job.id, progress, &mut published).await;
                }
                result = &mut download => break result,
            }
        };
        while let Ok(progress) = receiver.try_recv() {
            self.bytes(&job.id, progress, &mut published).await;
        }
        let at = now();
        let state = match result {
            Ok(_) => JobState::Done {
                run_id: None,
                id_mismatch: None,
                finished_at: at,
            },
            Err(ApiError::DownloadCancelled { .. }) => JobState::Cancelled { finished_at: at },
            Err(error) => JobState::Failed {
                error: error.to_string(),
                at_node: None,
                finished_at: at,
            },
        };
        self.end(&job.id, state).await;
    }

    /// A download's progress, published when it moved by a hundredth of the
    /// snapshot since the last one published, or completed it: the registry
    /// reports every chunk, and a stream of thousands would flood the
    /// clients and the buffer of recent events.
    async fn bytes(&self, id: &str, progress: DownloadProgress, published: &mut Option<u64>) {
        let step = (progress.total / 100).max(1);
        let due = match *published {
            None => true,
            Some(last) => progress.received >= last.saturating_add(step),
        } || progress.received >= progress.total;
        if due && *published != Some(progress.received) {
            *published = Some(progress.received);
            self.progress(id, progress.received, progress.total, None)
                .await;
        }
    }

    /// The job's terminal transition, written first; a write that fails is
    /// applied in memory anyway — the job has ended, and a client must not
    /// see it running — and reported, and a restart finds it interrupted.
    async fn end(&self, id: &str, terminal: JobState) {
        let mut state = self.state.lock().await;
        let Ok(index) = state.find(id) else {
            return;
        };
        let mut job = state.jobs[index].job.clone();
        let at = match &terminal {
            JobState::Done { finished_at, .. }
            | JobState::Failed { finished_at, .. }
            | JobState::Cancelled { finished_at } => *finished_at,
            JobState::Queued | JobState::Running { .. } => now(),
        };
        job.state = terminal;
        job.history.push(Transition {
            state: job.state.name().to_owned(),
            at,
        });
        if let Err(reason) = self.write(&job).await {
            state.faults.push(fault(
                &self.dir,
                &job.id,
                format!(
                "{reason}; its end is held in memory only, and a restart will find it interrupted"
            ),
            ));
        }
        let name = job.state.name();
        let view = summary(&job);
        state.jobs[index].job = job;
        self.publish(&mut state, name, &view);
    }
}

/// What a running job accumulated: each query's trace and latency.
#[derive(Default)]
struct Tally {
    traces: BTreeMap<QueryId, TraceDocument>,
    latencies: Vec<u64>,
    unread: u64,
}

/// The observer the queue hands the launcher: each query goes to the
/// worker's task, which applies it under the lock; the launcher's thread
/// never waits on the queue.
struct Forward(mpsc::UnboundedSender<QueryProgress>);

impl RunObserver for Forward {
    fn query_done(&self, progress: QueryProgress) {
        // The worker receives until execution returns; a query reported
        // after that is not part of the job.
        let _ = self.0.send(progress);
    }
}

fn fault(dir: &std::path::Path, id: &str, reason: String) -> JobFault {
    JobFault {
        path: file::path(dir, id).display().to_string(),
        reason,
    }
}

fn to_json(value: &impl serde::Serialize) -> String {
    // The values serialized here are this crate's response types: strings,
    // numbers and options, which always serialize.
    serde_json::to_string(value).unwrap_or_else(|error| {
        serde_json::json!({ "serialization_failed": error.to_string() }).to_string()
    })
}
