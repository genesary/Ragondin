//! The handlers of the workspace's endpoints — pipelines, benchmarks,
//! services — and of `POST /compare`, beside the read endpoints of
//! `handlers.rs`. Each reads or
//! writes through the backends, converts into this crate's own types, and
//! answers; every failure is an [`ApiError`].

pub(crate) mod benchmarks;
pub(crate) mod compare;
pub(crate) mod matrix;
pub(crate) mod pipelines;
pub(crate) mod services;
