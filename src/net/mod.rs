//! Network layer for kernel bypass experimentation (Phase 5).
//!
//! Routes:
//! - A: io_uring for async I/O (pure software, easiest to start)
//! - B: Raw socket capture (simulates packet reception)
//! - C: AF_XDP placeholder (requires kernel support)

pub mod raw_socket;
pub mod io_uring_bench;
pub mod packet_timer;
