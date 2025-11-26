#![allow(clippy::cast_precision_loss)] // Progress display - precision loss acceptable

use colored::Colorize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

// Upload progress tracking (currently unused but available for future UI improvements)
#[allow(dead_code)]
#[derive(Clone)]
pub struct FileProgress {
    pub name: String,
    pub total_size: u64,
    pub hash_verified: bool,
    pub decomposed_bytes: Arc<AtomicU64>,
    pub chunks_count: Arc<AtomicU64>,
    pub uploaded_bytes: Arc<AtomicU64>,
    pub start_time: Instant,
    pub decompose_start: Option<Instant>,
    pub upload_start: Option<Instant>,
}

#[allow(dead_code)]
impl FileProgress {
    pub fn new(name: String, total_size: u64) -> Self {
        Self {
            name,
            total_size,
            hash_verified: false,
            decomposed_bytes: Arc::new(AtomicU64::new(0)),
            chunks_count: Arc::new(AtomicU64::new(0)),
            uploaded_bytes: Arc::new(AtomicU64::new(0)),
            start_time: Instant::now(),
            decompose_start: None,
            upload_start: None,
        }
    }

    pub const fn set_hash_verified(&mut self) {
        self.hash_verified = true;
    }

    pub fn start_decompose(&mut self) {
        self.decompose_start = Some(Instant::now());
    }

    pub fn start_upload(&mut self) {
        self.upload_start = Some(Instant::now());
    }

    pub fn add_decomposed(&self, bytes: u64) {
        let _ = self.decomposed_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn set_chunks_count(&self, count: u64) {
        self.chunks_count.store(count, Ordering::Relaxed);
    }

    pub fn add_uploaded(&self, bytes: u64) {
        let _ = self.uploaded_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn decomposed_pct(&self) -> f64 {
        let bytes = self.decomposed_bytes.load(Ordering::Relaxed);
        (bytes as f64 / self.total_size as f64) * 100.0
    }

    pub fn uploaded_pct(&self) -> f64 {
        let bytes = self.uploaded_bytes.load(Ordering::Relaxed);
        (bytes as f64 / self.total_size as f64) * 100.0
    }

    pub fn decomposed_bytes(&self) -> u64 {
        self.decomposed_bytes.load(Ordering::Relaxed)
    }

    pub fn uploaded_bytes(&self) -> u64 {
        self.uploaded_bytes.load(Ordering::Relaxed)
    }

    pub fn chunks_count(&self) -> u64 {
        self.chunks_count.load(Ordering::Relaxed)
    }
}

// Upload session tracking (currently unused but available for future UI improvements)
#[allow(dead_code)]
pub struct UploadSession {
    pub files: Vec<FileProgress>,
    pub start_time: Instant,
    pub total_batches_sent: Arc<AtomicU64>,
    pub total_batches: Arc<AtomicU64>,
}

#[allow(dead_code)]
impl UploadSession {
    pub fn new(files: Vec<FileProgress>) -> Self {
        println!("{}", "📦 flakecache upload".bold());

        Self {
            files,
            start_time: Instant::now(),
            total_batches_sent: Arc::new(AtomicU64::new(0)),
            total_batches: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn render(&self) {
        // Clear screen and move cursor up
        print!("\x1B[2J\x1B[H");

        println!("{}", "📦 flakecache upload".bold());

        // Render each file
        for (idx, file) in self.files.iter().enumerate() {
            let is_last = idx == self.files.len() - 1;
            let prefix = if is_last { "└─" } else { "├─" };
            let continuation = if is_last { "   " } else { "│  " };

            // File header
            let file_size_mb = file.total_size as f64 / 1024.0 / 1024.0;
            println!(
                "{} {} {}",
                prefix,
                file.name.cyan(),
                format!("({file_size_mb:.1}MB)").dimmed()
            );

            // Hash verified
            let hash_status = if file.hash_verified {
                "✓".green()
            } else {
                "⊙".yellow()
            };
            println!("{continuation}├─ hash verified {hash_status}");

            // Decomposing
            let decomposed_bytes = file.decomposed_bytes();
            let decomposed_pct = file.decomposed_pct();
            let file_size_mb = file.total_size as f64 / 1024.0 / 1024.0;
            let decomposed_mb = decomposed_bytes as f64 / 1024.0 / 1024.0;

            let decompose_status = if decomposed_pct >= 100.0 {
                format!("{} 100%", "✓".green())
            } else {
                format!("{} {:.0}%", "⊙".yellow(), decomposed_pct)
            };

            println!(
                "{continuation}├─ decomposing {decompose_status} ({decomposed_mb:.1}MB / {file_size_mb:.1}MB)"
            );

            // Chunks count
            let chunks = file.chunks_count();
            if chunks > 0 {
                println!(
                    "{}├─ chunks created: {}",
                    continuation,
                    chunks.to_string().cyan()
                );
            }

            // Uploading
            let uploaded_bytes = file.uploaded_bytes();
            let uploaded_pct = file.uploaded_pct();
            let uploaded_mb = uploaded_bytes as f64 / 1024.0 / 1024.0;

            let upload_status = if uploaded_pct >= 100.0 {
                format!("{} 100%", "✓".green())
            } else {
                format!("{} {:.0}%", "⊙".yellow(), uploaded_pct)
            };

            let upload_prefix = "└─";
            println!(
                "{continuation}{upload_prefix} uploading {upload_status} ({uploaded_mb:.1}MB / {file_size_mb:.1}MB)"
            );
        }

        // Summary
        println!();
        println!("{}", "📊 Summary".bold());

        let total_size: u64 = self.files.iter().map(|f| f.total_size).sum();
        let total_uploaded: u64 = self.files.iter().map(FileProgress::uploaded_bytes).sum();
        let total_pct = (total_uploaded as f64 / total_size as f64) * 100.0;

        let total_size_mb = total_size as f64 / 1024.0 / 1024.0;
        let total_uploaded_mb = total_uploaded as f64 / 1024.0 / 1024.0;

        println!("├─ Total size: {total_size_mb:.1}MB");

        println!("├─ Uploaded: {total_uploaded_mb:.1}MB ({total_pct:.0}%)");

        // Speed calculation
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let speed_mbs = total_uploaded_mb / elapsed;
        println!("├─ Speed: {speed_mbs:.2}MB/s");

        // Active files
        let active_count = self
            .files
            .iter()
            .filter(|f| f.uploaded_pct() < 100.0)
            .count();
        println!("├─ Active: {active_count} files");

        // Batches
        let batches_sent = self.total_batches_sent.load(Ordering::Relaxed);
        let batches_total = self.total_batches.load(Ordering::Relaxed);
        if batches_total > 0 {
            println!("├─ Batches sent: {batches_sent}/{batches_total}");
        }

        // ETA
        if speed_mbs > 0.0 && total_uploaded < total_size {
            let remaining = (total_size - total_uploaded) as f64 / 1024.0 / 1024.0;
            let eta_secs = remaining / speed_mbs;
            let eta_mins = eta_secs / 60.0;

            if eta_mins > 1.0 {
                println!("└─ ETA: {:.0}m {:.0}s", eta_mins.floor(), eta_secs % 60.0);
            } else {
                println!("└─ ETA: {eta_secs:.0}s");
            }
        }
    }

    pub fn total_uploaded(&self) -> u64 {
        self.files.iter().map(FileProgress::uploaded_bytes).sum()
    }

    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.total_size).sum()
    }

    pub fn all_done(&self) -> bool {
        self.total_uploaded() >= self.total_size()
            && self.files.iter().all(|f| f.uploaded_pct() >= 100.0)
    }
}
