//! How much memory one result set may hold (`docs/preview-audit.md`, A8):
//! about a quarter of the machine's physical memory, so Fetch All or No limit
//! on a big table stops and says so instead of ending the app.

/// A result set stops at this share of physical memory.
const SHARE: u64 = 4;

/// The byte budget of one result set; `None` when the memory size cannot be read.
pub fn result_budget() -> Option<u64> {
    let mut system = sysinfo::System::new().ok()?;
    system.refresh_memory();
    match system.total_memory() {
        0 => None,
        total => Some(total / SHARE),
    }
}
