//! `GET /jobs/events`: every transition, every progress tick and every fault
//! reported beside a job, as server-sent events. Each event's name is the
//! state entered — `queued`, `running`, `done`, `failed`, `cancelled` — or
//! `reordered`, or `fault`, and its data is the job as `GET /jobs/{id}`
//! answers it. Its id is `<process>:<number>`;
//! a client that reconnects with `Last-Event-ID` gets every event it missed,
//! once, when this process still holds them all. Otherwise — no
//! `Last-Event-ID`, one from another process, one older than the events kept
//! — the stream begins with `resync`, whose data is the whole queue as
//! `GET /jobs` answers it, so a client never acts on a gap.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver;

use super::queue::{Published, Queue};
use crate::extract::{ApiHeaders, ApiQuery, NoParameters};
use crate::handlers::AppState;
use crate::request::EventsHeaders;

/// How often an idle stream sends a comment, so a proxy or the browser does
/// not close a connection that only waits for the next job.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// `GET /jobs/events`.
pub(crate) async fn events(
    State(state): State<AppState>,
    _: ApiQuery<NoParameters>,
    ApiHeaders(headers): ApiHeaders<EventsHeaders>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let queue = Arc::clone(&state.jobs);
    let (backlog, receiver) = queue.subscribe(headers.last_event_id.as_deref()).await;
    let last = backlog.last().map(|event| event.seq);
    let stream = stream::unfold(
        Cursor {
            queue,
            backlog: backlog.into(),
            receiver,
            last,
        },
        Cursor::next,
    );
    Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}

/// Where one client's stream stands: the events still to replay, the live
/// receiver, and the number of the last event sent, so none is sent twice.
struct Cursor {
    queue: Arc<Queue>,
    backlog: VecDeque<Arc<Published>>,
    receiver: Receiver<Arc<Published>>,
    last: Option<u64>,
}

impl Cursor {
    async fn next(mut self) -> Option<(Result<Event, Infallible>, Self)> {
        if let Some(event) = self.backlog.pop_front() {
            let sent = self.event(&event);
            return Some((Ok(sent), self));
        }
        loop {
            match self.receiver.recv().await {
                Ok(event) if self.last.is_some_and(|last| event.seq <= last) => continue,
                Ok(event) => {
                    self.last = Some(event.seq);
                    let sent = self.event(&event);
                    return Some((Ok(sent), self));
                }
                // The client fell further behind than the channel holds:
                // what it missed is gone, so it is given the whole queue.
                // Subscribed again first, so an event published meanwhile is
                // either in the queue it is given or after it — the number
                // check drops the former.
                Err(RecvError::Lagged(_)) => {
                    self.receiver = self.receiver.resubscribe();
                    let event = self.queue.resynchronised().await;
                    self.last = Some(event.seq);
                    let sent = self.event(&event);
                    return Some((Ok(sent), self));
                }
                Err(RecvError::Closed) => return None,
            }
        }
    }

    fn event(&self, event: &Published) -> Event {
        Event::default()
            .id(self.queue.event_id(event))
            .event(event.name)
            .data(&event.data)
    }
}
