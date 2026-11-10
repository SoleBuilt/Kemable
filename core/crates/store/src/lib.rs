//! Result sets held in columnar memory, read by a grid one window at a time
//! (`docs/decisions/0002-result-windows.md`).

mod filter;
mod order;
mod store;
pub mod window;

#[cfg(test)]
mod tests;

pub use store::{CHUNK_ROWS, CellRange, CellValue, MAX_COPY_CELLS, ResultStore, SortKey, StoreState};
pub use window::{WindowReader, WindowRequest};
