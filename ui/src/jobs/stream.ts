// What a screen that follows the job queue says while the stream is down
// (the front-end design, § 8: the state never pretends to be current). One
// wording for every such screen — Setup's downloads and Runs' jobs.

/** The job stream is down and retrying. */
export const STREAM_DOWN = 'Job stream disconnected, retrying.';

/** The same, while a job is under way, whose progress on screen is then the last known. */
export const STREAM_DOWN_LIVE = 'Job stream disconnected, retrying: progress shown is the last known.';
