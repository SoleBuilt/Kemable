//! Result sets held in columnar memory, read by a grid one window at a time
//! (`docs/decisions/0002-result-windows.md`).

mod store;
pub mod window;

pub use store::{CHUNK_ROWS, CellValue, ResultStore, StoreState};
pub use window::{WindowReader, WindowRequest};
