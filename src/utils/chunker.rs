//! Content-defined chunking for deduplication and efficient transfers.
//!
//! Uses the FlakeCache chunker for splitting large files into variable-sized chunks
//! based on content patterns, enabling deduplication across packages.

use std::io::Read;

pub use flakecache_chunker::ChunkStream;

/// Creates a streaming chunker with default FastCDC parameters.
///
/// # Examples
/// ```no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use flakecache_cli::utils::chunker;
/// let reader = std::io::Cursor::new(b"example".to_vec());
/// let mut chunker = chunker::new_chunk_stream(reader)?;
/// # Ok(())
/// # }
/// ```
pub fn new_chunk_stream<R: Read>(
    reader: R,
) -> Result<ChunkStream<R>, flakecache_chunker::chunking::ChunkingError> {
    ChunkStream::new(reader, None, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunker_creation() {
        let reader = std::io::Cursor::new(b"example".to_vec());
        let _chunker = new_chunk_stream(reader).expect("chunk stream should be created");
        // Chunker should be created successfully
    }
}
