//! The job model and the queue that runs it (the design document § 7).
//!
//! A [`Job`] is a run or a benchmark download asked for through the API: what
//! was asked, where it stands, and every transition it went through. It is
//! the record `jobs/<id>.json` holds, written before the queue's memory
//! changes at each transition — so the file is the write-ahead record, and a
//! restart reads the queue back from it.
//!
//! The queue (`queue.rs`) has two lanes, each with one worker: runs, which
//! execute through `Launcher::execute` one at a time, and downloads, which go
//! through `Registry::download` alongside them. The event stream
//! (`stream.rs`) serves every transition and every progress tick over
//! server-sent events, with a buffer of recent ones for a client that
//! reconnects. `ARCHITECTURE.md` § The job queue holds the state machine.

use ragondin_experiments::UnixMillis;
use ragondin_pipeline::PipelineHash;
use serde::{Deserialize, Serialize};

use crate::backends::Submission;
use crate::response::{
    JobStatus, JobSummary, JobWork, ReportedFault, RunIdMismatch as RunIdMismatchSummary,
    ServiceBinding,
};

mod file;
mod queue;
mod stream;

pub(crate) use queue::{Partial, Queue};
pub(crate) use stream::events;

/// A job: a run or a download asked for through the API, as `jobs/<id>.json`
/// holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// The job's id, which names its file.
    pub id: String,
    /// Its place among the jobs: a lane's worker takes the queued job with
    /// the lowest, and a reorder exchanges them.
    pub position: u64,
    /// When it was accepted; `None` when the clock read before the epoch.
    pub created_at: Option<UnixMillis>,
    /// What it does.
    pub work: Work,
    /// Where it stands.
    pub state: JobState,
    /// Every transition it went through, in order, the current one last.
    pub history: Vec<Transition>,
    /// What went wrong beside it without stopping it, in the order reported.
    /// Absent in a file written before faults were recorded on the job.
    #[serde(default)]
    pub faults: Vec<Fault>,
    /// When a person dismissed it, once it had ended: it is then left out of
    /// what a client shows, and nothing else about it changes. Absent in a
    /// file written before dismissals were recorded, and while it is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed_at: Option<UnixMillis>,
}

/// A fault beside a job: what went wrong without changing its state, and
/// what the queue did about it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    /// What went wrong, and what the queue did about it.
    pub reason: String,
    /// When it was reported; `None` when the clock read before the epoch.
    pub at: Option<UnixMillis>,
    /// While the fault, or the transition it accompanies, is held in memory
    /// only: what that means, which the API appends to the reason. Never
    /// written — the write that would carry it is the one that makes it
    /// false — and cleared by the job's next write that succeeds
    /// (`Job::written`).
    #[serde(skip)]
    pub unwritten: Option<String>,
}

impl Fault {
    /// `reason`, reported now.
    pub(crate) fn now(reason: String) -> Self {
        Self {
            reason,
            at: now(),
            unwritten: None,
        }
    }

    /// `reason`, reported now, and held in memory only: `clause` says what
    /// that means until a write of the job carries it.
    pub(crate) fn held(reason: String, clause: String) -> Self {
        Self {
            unwritten: Some(clause),
            ..Self::now(reason)
        }
    }
}

impl Job {
    /// The job was just written whole, its faults with it: none is held in
    /// memory only any more.
    pub(crate) fn written(&mut self) {
        for fault in &mut self.faults {
            fault.unwritten = None;
        }
    }
}

/// What a job does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Work {
    /// A run of a pipeline over a benchmark, on the run lane.
    Run {
        /// The run id announced at submission (`Launcher::identity`), stored
        /// as it was announced: the queue never computes one (INV-8).
        run_id: String,
        /// The pipeline's name in the workspace.
        pipeline_name: String,
        /// The pipeline document, snapshotted at submission.
        pipeline: String,
        /// The benchmark to evaluate it on.
        benchmark: String,
        /// The `Remote` bindings in force at submission.
        bindings: Vec<ServiceBinding>,
        /// The node a prefix run stops at; then `pipeline` is the cut, and
        /// `pipeline_name` the parent's name.
        up_to: Option<String>,
        /// For a prefix run, the canonical hash of the parent document the
        /// cut was made from: with `up_to`, the prefix's provenance, outside
        /// the run's identity. Absent in a file written before it was
        /// recorded.
        #[serde(default)]
        parent_pipeline_hash: Option<PipelineHash>,
    },
    /// A benchmark the manifest names, downloaded and verified, on the
    /// download lane.
    Download {
        /// The benchmark's selector.
        benchmark: String,
    },
}

impl Work {
    /// The submission a run job executes; `None` for a download.
    pub(crate) fn submission(&self) -> Option<Submission> {
        match self {
            Self::Run {
                pipeline_name,
                pipeline,
                benchmark,
                bindings,
                up_to,
                parent_pipeline_hash,
                ..
            } => Some(Submission {
                pipeline_name: pipeline_name.clone(),
                pipeline: pipeline.clone(),
                benchmark: benchmark.clone(),
                bindings: bindings.clone(),
                up_to: up_to.clone(),
                parent_pipeline_hash: *parent_pipeline_hash,
            }),
            Self::Download { .. } => None,
        }
    }
}

/// Where a job stands. Every time is `None` when the clock read before the
/// epoch — unknown, never `0`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobState {
    /// Waiting for its lane's worker.
    Queued,
    /// Executing. A run counts queries; a download counts bytes.
    Running {
        /// Queries executed, or bytes received.
        done: u64,
        /// Queries in the benchmark, or bytes in the snapshot; `None` until
        /// the first tick reports it.
        total: Option<u64>,
        /// When the worker took it.
        started_at: Option<UnixMillis>,
        /// For a run, the lower median of the latencies of the queries
        /// executed so far, each read from its trace; `None` before the
        /// first, and for a download.
        median_latency_nanos: Option<u64>,
    },
    /// Finished.
    Done {
        /// For a run, the id it is filed under: the one the harness computed
        /// from what ran. `None` for a download.
        run_id: Option<String>,
        /// For a run whose filed id is not the announced one, both.
        id_mismatch: Option<RunIdMismatch>,
        /// When it finished.
        finished_at: Option<UnixMillis>,
    },
    /// Failed — or, with `interrupted`, found running when the service
    /// started.
    Failed {
        /// What failed.
        error: String,
        /// The node that failed, when one did.
        at_node: Option<String>,
        /// When it failed.
        finished_at: Option<UnixMillis>,
        /// How many queries' traces it kept under `jobs/<id>/partial/`:
        /// `Some(0)` for a job that executed none, failed before running, or
        /// was interrupted by a crash. `None` only in a file written before
        /// the count was recorded; the queue counts the traces such a job
        /// kept when it reads the file back.
        #[serde(default)]
        partial_traces: Option<u64>,
    },
    /// Cancelled before it finished.
    Cancelled {
        /// When it was cancelled.
        finished_at: Option<UnixMillis>,
        /// How many queries' traces it kept, as for `Failed`.
        #[serde(default)]
        partial_traces: Option<u64>,
    },
}

/// The error a job found running at start-up is failed with.
pub const INTERRUPTED: &str = "interrupted";

impl JobState {
    /// The state's name, as the history and the event stream spell it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running { .. } => "running",
            Self::Done { .. } => "done",
            Self::Failed { .. } => "failed",
            Self::Cancelled { .. } => "cancelled",
        }
    }

    /// Whether the job has ended.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Done { .. } | Self::Failed { .. } | Self::Cancelled { .. }
        )
    }
}

/// A run filed under another id than the one it announced (ADR-C36 § 1):
/// both, so the job reports the difference. The run is filed under
/// `decided`, never under `announced`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunIdMismatch {
    /// The id `Launcher::identity` announced at submission.
    pub announced: String,
    /// The id the harness computed from what ran.
    pub decided: String,
}

/// One transition: the state entered, and when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    /// The state's name ([`JobState::name`]).
    pub state: String,
    /// When it was entered.
    pub at: Option<UnixMillis>,
}

/// The time now, as a job records it.
pub(crate) fn now() -> Option<UnixMillis> {
    UnixMillis::from_system_time(std::time::SystemTime::now())
}

/// The job as the API answers it. The pipeline document is left out: it can
/// be large, and the workspace holds it under the job's pipeline name.
pub(crate) fn summary(job: &Job) -> JobSummary {
    let millis = |at: Option<UnixMillis>| at.map(UnixMillis::get);
    JobSummary {
        id: job.id.clone(),
        position: job.position,
        created_at_ms: millis(job.created_at),
        work: match &job.work {
            Work::Run {
                run_id,
                pipeline_name,
                benchmark,
                bindings,
                up_to,
                parent_pipeline_hash,
                ..
            } => JobWork::Run {
                run_id: run_id.clone(),
                pipeline: pipeline_name.clone(),
                benchmark: benchmark.clone(),
                bindings: bindings.clone(),
                up_to: up_to.clone(),
                parent_pipeline_hash: parent_pipeline_hash.map(|hash| hash.to_string()),
            },
            Work::Download { benchmark } => JobWork::Download {
                benchmark: benchmark.clone(),
            },
        },
        state: match &job.state {
            JobState::Queued => JobStatus::Queued,
            JobState::Running {
                done,
                total,
                started_at,
                median_latency_nanos,
            } => JobStatus::Running {
                done: *done,
                total: *total,
                started_at_ms: millis(*started_at),
                median_latency_nanos: *median_latency_nanos,
            },
            JobState::Done {
                run_id,
                id_mismatch,
                finished_at,
            } => JobStatus::Done {
                run_id: run_id.clone(),
                id_mismatch: id_mismatch.as_ref().map(|mismatch| RunIdMismatchSummary {
                    announced: mismatch.announced.clone(),
                    decided: mismatch.decided.clone(),
                }),
                finished_at_ms: millis(*finished_at),
            },
            // The queue counts the traces of a file that recorded no count
            // when it reads it back, so `None` is not met once a job is in it.
            JobState::Failed {
                error,
                at_node,
                finished_at,
                partial_traces,
            } => JobStatus::Failed {
                error: error.clone(),
                at_node: at_node.clone(),
                finished_at_ms: millis(*finished_at),
                partial_traces: partial_traces.unwrap_or(0),
            },
            JobState::Cancelled {
                finished_at,
                partial_traces,
            } => JobStatus::Cancelled {
                finished_at_ms: millis(*finished_at),
                partial_traces: partial_traces.unwrap_or(0),
            },
        },
        faults: job
            .faults
            .iter()
            .map(|fault| ReportedFault {
                reason: match &fault.unwritten {
                    Some(clause) => format!("{}; {clause}", fault.reason),
                    None => fault.reason.clone(),
                },
                at_ms: millis(fault.at),
            })
            .collect(),
        dismissed_at_ms: millis(job.dismissed_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_with(fault: Fault) -> Job {
        Job {
            id: "1-1".to_owned(),
            position: 0,
            created_at: None,
            work: Work::Download {
                benchmark: "beir/mini".to_owned(),
            },
            state: JobState::Queued,
            history: Vec::new(),
            faults: vec![fault],
            dismissed_at: None,
        }
    }

    /// A fault that could not be written says so while it is in memory
    /// only, and never on disk: the write that would carry the clause is the
    /// one that makes it false.
    #[test]
    fn a_fault_held_in_memory_says_so_until_a_write_carries_it() {
        let mut job = job_with(Fault::held(
            "the layout could not be copied".to_owned(),
            "this fault is held in memory only: disk full".to_owned(),
        ));
        assert_eq!(
            summary(&job).faults[0].reason,
            "the layout could not be copied; this fault is held in memory only: disk full"
        );
        let written = serde_json::to_value(&job).unwrap();
        let on_disk = written["faults"][0].as_object().unwrap();
        assert_eq!(
            on_disk.keys().collect::<Vec<_>>(),
            ["at", "reason"],
            "{on_disk:?}"
        );
        assert_eq!(on_disk["reason"], "the layout could not be copied");

        job.written();
        assert_eq!(
            summary(&job).faults[0].reason,
            "the layout could not be copied"
        );
    }
}
