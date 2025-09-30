mod common;
mod test_async;
mod test_async_blocking;
mod test_blocking_async;
#[allow(unused_imports)]
mod test_blocking_context;
mod test_hanging_issue;

// we don't want to import smol-timeout
#[cfg(not(feature = "smol"))]
mod test_type_switch;
