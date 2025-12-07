//! Content-defined chunking for deduplication and efficient transfers.
//!
//! Uses the FlakeCache chunker for splitting large files into variable-sized chunks
//! based on content patterns, enabling deduplication across packages.

pub use flakecache_chunker::{Chunker, ChunkStream};

/// Creates a new chunker with default FastCDC parameters.
///
/// # Examples
/// ```no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use flakecache_cli::utils::chunker;
/// let mut chunker = chunker::new_chunker();
/// # Ok(())
/// # }
/// ```
pub fn new_chunker() -> Chunker {
    Chunker::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunker_creation() {
        let _chunker = new_chunker();
        // Chunker should be created successfully
    }
}
