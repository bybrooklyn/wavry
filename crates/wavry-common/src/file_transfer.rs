use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const DEFAULT_MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
pub const DEFAULT_CHUNK_SIZE: usize = 900;
pub const MAX_FILENAME_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOffer {
    pub file_id: u64,
    pub filename: String,
    pub file_size: u64,
    pub checksum_sha256: String,
    pub chunk_size: u32,
    pub total_chunks: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChunkData {
    pub file_id: u64,
    pub chunk_index: u32,
    pub payload: Vec<u8>,
}

pub struct OutgoingFile {
    offer: FileOffer,
    file: File,
    next_chunk: u32,
    header_sent: bool,
    paused: bool,
}

impl OutgoingFile {
    pub fn from_path(
        path: &Path,
        file_id: u64,
        chunk_size: usize,
        max_file_bytes: u64,
    ) -> Result<Self> {
        if file_id == 0 {
            return Err(anyhow!("file_id must be non-zero"));
        }
        if chunk_size == 0 {
            return Err(anyhow!("chunk_size must be non-zero"));
        }

        let metadata =
            fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
        if !metadata.is_file() {
            return Err(anyhow!("not a regular file: {}", path.display()));
        }
        let file_size = metadata.len();
        if file_size == 0 {
            return Err(anyhow!("empty files are not supported: {}", path.display()));
        }
        if file_size > max_file_bytes {
            return Err(anyhow!(
                "file {} exceeds max size {} bytes (got {})",
                path.display(),
                max_file_bytes,
                file_size
            ));
        }

        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(sanitize_filename)
            .ok_or_else(|| anyhow!("invalid filename: {}", path.display()))?;
        let total_chunks = chunk_count(file_size, chunk_size)?;
        let checksum_sha256 = sha256_file_hex(path)?;
        let file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;

        Ok(Self {
            offer: FileOffer {
                file_id,
                filename,
                file_size,
                checksum_sha256,
                chunk_size: chunk_size as u32,
                total_chunks,
            },
            file,
            next_chunk: 0,
            header_sent: false,
            paused: false,
        })
    }

    pub fn offer(&self) -> &FileOffer {
        &self.offer
    }

    pub fn mark_header_sent(&mut self) {
        self.header_sent = true;
    }

    pub fn header_sent(&self) -> bool {
        self.header_sent
    }

    pub fn reset_header(&mut self) {
        self.header_sent = false;
    }

    pub fn finished(&self) -> bool {
        self.next_chunk >= self.offer.total_chunks
    }

    pub fn next_chunk_index(&self) -> u32 {
        self.next_chunk
    }

    pub fn set_next_chunk(&mut self, chunk_index: u32) -> Result<()> {
        if chunk_index > self.offer.total_chunks {
            return Err(anyhow!(
                "chunk index {} out of range for {} chunks",
                chunk_index,
                self.offer.total_chunks
            ));
        }
        self.next_chunk = chunk_index;
        Ok(())
    }

    pub fn restart_from_beginning(&mut self) {
        self.next_chunk = 0;
        self.header_sent = false;
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn next_chunk(&mut self) -> Result<Option<FileChunkData>> {
        if self.finished() {
            return Ok(None);
        }

        let chunk_index = self.next_chunk;
        let offset = (chunk_index as u64)
            .checked_mul(self.offer.chunk_size as u64)
            .ok_or_else(|| anyhow!("chunk offset overflow"))?;
        let chunk_len = expected_chunk_len(&self.offer, chunk_index)?;
        if chunk_len == 0 {
            self.next_chunk = self.offer.total_chunks;
            return Ok(None);
        }

        self.file
            .seek(SeekFrom::Start(offset))
            .with_context(|| format!("seek failed for {}", self.offer.filename))?;
        let mut payload = vec![0u8; chunk_len];
        self.file
            .read_exact(&mut payload)
            .with_context(|| format!("read failed for {}", self.offer.filename))?;

        self.next_chunk = self.next_chunk.saturating_add(1);
        Ok(Some(FileChunkData {
            file_id: self.offer.file_id,
            chunk_index,
            payload,
        }))
    }
}

pub struct IncomingFile {
    offer: FileOffer,
    part_path: PathBuf,
    final_path: PathBuf,
    file: File,
    received: Vec<bool>,
    received_count: u32,
}

impl IncomingFile {
    pub fn new(output_dir: &Path, offer: FileOffer, max_file_bytes: u64) -> Result<Self> {
        validate_offer(&offer, max_file_bytes)?;
        fs::create_dir_all(output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;

        let sanitized = sanitize_filename(&offer.filename)
            .ok_or_else(|| anyhow!("invalid filename: {}", offer.filename))?;
        let final_path = output_dir.join(&sanitized);
        let part_path = output_dir.join(format!("{sanitized}.part"));

        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .read(true)
            .open(&part_path)
            .with_context(|| format!("failed to open {}", part_path.display()))?;
        file.set_len(offer.file_size)
            .with_context(|| format!("failed to size {}", part_path.display()))?;

        let received = vec![false; offer.total_chunks as usize];

        Ok(Self {
            offer,
            part_path,
            final_path,
            file,
            received,
            received_count: 0,
        })
    }

    pub fn offer(&self) -> &FileOffer {
        &self.offer
    }

    pub fn is_complete(&self) -> bool {
        self.received_count == self.offer.total_chunks
    }

    pub fn received_count(&self) -> u32 {
        self.received_count
    }

    pub fn next_missing_chunk(&self) -> u32 {
        self.received
            .iter()
            .position(|seen| !seen)
            .map(|idx| idx as u32)
            .unwrap_or(self.offer.total_chunks)
    }

    pub fn abort(self) -> Result<()> {
        let IncomingFile {
            part_path, file, ..
        } = self;
        drop(file);
        if part_path.exists() {
            fs::remove_file(&part_path)
                .with_context(|| format!("failed to remove {}", part_path.display()))?;
        }
        Ok(())
    }

    pub fn write_chunk(&mut self, chunk_index: u32, payload: &[u8]) -> Result<bool> {
        if chunk_index >= self.offer.total_chunks {
            return Err(anyhow!(
                "chunk index {} out of range for {} chunks",
                chunk_index,
                self.offer.total_chunks
            ));
        }

        let expected = expected_chunk_len(&self.offer, chunk_index)?;
        if payload.len() != expected {
            return Err(anyhow!(
                "unexpected payload length for chunk {}: expected {}, got {}",
                chunk_index,
                expected,
                payload.len()
            ));
        }

        let idx = chunk_index as usize;
        if !self.received[idx] {
            let offset = (chunk_index as u64)
                .checked_mul(self.offer.chunk_size as u64)
                .ok_or_else(|| anyhow!("chunk offset overflow"))?;
            self.file
                .seek(SeekFrom::Start(offset))
                .with_context(|| format!("seek failed for {}", self.offer.filename))?;
            self.file
                .write_all(payload)
                .with_context(|| format!("write failed for {}", self.offer.filename))?;
            self.received[idx] = true;
            self.received_count = self.received_count.saturating_add(1);
        }

        Ok(self.is_complete())
    }

    pub fn finalize(mut self) -> Result<PathBuf> {
        if !self.is_complete() {
            return Err(anyhow!(
                "file {} is incomplete ({}/{})",
                self.offer.filename,
                self.received_count,
                self.offer.total_chunks
            ));
        }

        self.file.flush()?;
        self.file.sync_all()?;
        drop(self.file);

        let checksum = sha256_file_hex(&self.part_path)?;
        if checksum != self.offer.checksum_sha256 {
            return Err(anyhow!(
                "checksum mismatch for {}: expected {}, got {}",
                self.offer.filename,
                self.offer.checksum_sha256,
                checksum
            ));
        }

        let destination = unique_destination_path(&self.final_path);
        fs::rename(&self.part_path, &destination).with_context(|| {
            format!(
                "failed to move {} to {}",
                self.part_path.display(),
                destination.display()
            )
        })?;
        Ok(destination)
    }
}

pub fn sanitize_filename(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let base = Path::new(trimmed).file_name()?.to_string_lossy();
    let mut clean = String::with_capacity(base.len().min(MAX_FILENAME_BYTES));
    for ch in base.chars() {
        let safe = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ' ');
        clean.push(if safe { ch } else { '_' });
        if clean.len() >= MAX_FILENAME_BYTES {
            break;
        }
    }
    let clean = clean.trim().trim_matches('.').to_string();
    if clean.is_empty() || clean == "." || clean == ".." {
        return None;
    }
    Some(clean)
}

pub fn sha256_file_hex(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn sha256_bytes_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn chunk_count(file_size: u64, chunk_size: usize) -> Result<u32> {
    if chunk_size == 0 {
        return Err(anyhow!("chunk_size must be non-zero"));
    }
    let count = file_size.div_ceil(chunk_size as u64);
    if count == 0 {
        return Err(anyhow!("file_size must be non-zero"));
    }
    if count > u32::MAX as u64 {
        return Err(anyhow!("chunk count exceeds protocol limits"));
    }
    Ok(count as u32)
}

pub fn validate_offer(offer: &FileOffer, max_file_bytes: u64) -> Result<()> {
    if offer.file_id == 0 {
        return Err(anyhow!("file_id must be non-zero"));
    }
    if sanitize_filename(&offer.filename).is_none() {
        return Err(anyhow!("invalid filename in offer"));
    }
    if offer.file_size == 0 {
        return Err(anyhow!("file_size must be non-zero"));
    }
    if offer.file_size > max_file_bytes {
        return Err(anyhow!("file exceeds configured maximum"));
    }
    if offer.chunk_size == 0 {
        return Err(anyhow!("chunk_size must be non-zero"));
    }
    if offer.total_chunks == 0 {
        return Err(anyhow!("total_chunks must be non-zero"));
    }
    let expected = chunk_count(offer.file_size, offer.chunk_size as usize)?;
    if expected != offer.total_chunks {
        return Err(anyhow!(
            "invalid total_chunks: expected {}, got {}",
            expected,
            offer.total_chunks
        ));
    }
    if offer.checksum_sha256.len() != 64
        || !offer.checksum_sha256.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(anyhow!("invalid checksum_sha256"));
    }
    Ok(())
}

fn expected_chunk_len(offer: &FileOffer, chunk_index: u32) -> Result<usize> {
    if chunk_index >= offer.total_chunks {
        return Err(anyhow!("chunk index out of bounds"));
    }
    let offset = (chunk_index as u64)
        .checked_mul(offer.chunk_size as u64)
        .ok_or_else(|| anyhow!("chunk offset overflow"))?;
    let remaining = offer.file_size.saturating_sub(offset);
    Ok(remaining.min(offer.chunk_size as u64) as usize)
}

fn unique_destination_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    for n in 1..=9_999u32 {
        let candidate = if ext.is_empty() {
            path.with_file_name(format!("{stem} ({n})"))
        } else {
            path.with_file_name(format!("{stem} ({n}).{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wavry-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn sanitize_filename_rejects_path_traversal() {
        assert_eq!(
            sanitize_filename("../../../../etc/passwd"),
            Some("passwd".to_string())
        );
        assert_eq!(sanitize_filename(""), None);
        assert_eq!(sanitize_filename(".."), None);
    }

    #[test]
    fn outgoing_offer_matches_input_file() {
        let dir = temp_dir("offer");
        let file_path = dir.join("hello.txt");
        fs::write(&file_path, b"hello wavry").unwrap();

        let out = OutgoingFile::from_path(&file_path, 42, 4, DEFAULT_MAX_FILE_BYTES).unwrap();
        assert_eq!(out.offer().file_id, 42);
        assert_eq!(out.offer().filename, "hello.txt");
        assert_eq!(out.offer().file_size, 11);
        assert_eq!(out.offer().total_chunks, 3);
        assert_eq!(
            out.offer().checksum_sha256,
            sha256_bytes_hex(b"hello wavry")
        );
    }

    #[test]
    fn chunk_count_rejects_zero_values() {
        assert!(chunk_count(0, 1).is_err());
        assert!(chunk_count(1, 0).is_err());
    }

    #[test]
    fn transfer_roundtrip_survives_reordered_chunks() {
        let dir = temp_dir("roundtrip");
        let send_path = dir.join("payload.bin");
        let recv_dir = dir.join("recv");

        let payload = vec![7u8; 10_000];
        fs::write(&send_path, &payload).unwrap();

        let mut outgoing =
            OutgoingFile::from_path(&send_path, 7, 900, DEFAULT_MAX_FILE_BYTES).unwrap();
        let offer = outgoing.offer().clone();
        let mut incoming = IncomingFile::new(&recv_dir, offer, DEFAULT_MAX_FILE_BYTES).unwrap();

        let mut chunks = Vec::new();
        while let Some(chunk) = outgoing.next_chunk().unwrap() {
            chunks.push(chunk);
        }
        chunks.reverse();

        for chunk in chunks {
            let _ = incoming
                .write_chunk(chunk.chunk_index, &chunk.payload)
                .unwrap();
        }

        assert!(incoming.is_complete());
        let out_path = incoming.finalize().unwrap();
        let received = fs::read(out_path).unwrap();
        assert_eq!(received, payload);
    }

    #[test]
    fn transfer_with_simulated_loss_and_retransmit() {
        let dir = temp_dir("loss");
        let send_path = dir.join("payload.bin");
        let recv_dir = dir.join("recv");

        let payload = vec![3u8; 8_192];
        fs::write(&send_path, &payload).unwrap();

        let mut outgoing =
            OutgoingFile::from_path(&send_path, 17, 700, DEFAULT_MAX_FILE_BYTES).unwrap();
        let offer = outgoing.offer().clone();
        let mut incoming = IncomingFile::new(&recv_dir, offer, DEFAULT_MAX_FILE_BYTES).unwrap();

        let mut dropped: Option<FileChunkData> = None;
        while let Some(chunk) = outgoing.next_chunk().unwrap() {
            if chunk.chunk_index == 2 {
                dropped = Some(chunk);
                continue;
            }
            let _ = incoming
                .write_chunk(chunk.chunk_index, &chunk.payload)
                .unwrap();
        }

        assert!(!incoming.is_complete());
        let dropped = dropped.expect("expected to drop one chunk");
        let _ = incoming
            .write_chunk(dropped.chunk_index, &dropped.payload)
            .unwrap();
        assert!(incoming.is_complete());

        let out_path = incoming.finalize().unwrap();
        let received = fs::read(out_path).unwrap();
        assert_eq!(received, payload);
    }

    #[test]
    fn checksum_mismatch_is_rejected() {
        let dir = temp_dir("checksum");
        let send_path = dir.join("payload.bin");
        let recv_dir = dir.join("recv");

        fs::write(&send_path, b"abcdef").unwrap();
        let mut outgoing =
            OutgoingFile::from_path(&send_path, 99, 2, DEFAULT_MAX_FILE_BYTES).unwrap();
        let offer = outgoing.offer().clone();
        let mut incoming = IncomingFile::new(&recv_dir, offer, DEFAULT_MAX_FILE_BYTES).unwrap();

        while let Some(mut chunk) = outgoing.next_chunk().unwrap() {
            if chunk.chunk_index == 1 {
                chunk.payload[0] ^= 0xFF;
            }
            let _ = incoming
                .write_chunk(chunk.chunk_index, &chunk.payload)
                .unwrap();
        }

        assert!(incoming.is_complete());
        assert!(incoming.finalize().is_err());
    }

    #[test]
    fn rejects_oversized_offer() {
        let offer = FileOffer {
            file_id: 1,
            filename: "big.bin".to_string(),
            file_size: DEFAULT_MAX_FILE_BYTES + 1,
            checksum_sha256: "0".repeat(64),
            chunk_size: 1024,
            total_chunks: 2,
        };
        assert!(validate_offer(&offer, DEFAULT_MAX_FILE_BYTES).is_err());
    }

    #[test]
    fn outgoing_file_seek_pause_and_restart() {
        let dir = temp_dir("seek-restart");
        let file_path = dir.join("payload.bin");
        fs::write(&file_path, vec![5u8; 5_000]).unwrap();

        let mut outgoing =
            OutgoingFile::from_path(&file_path, 123, 700, DEFAULT_MAX_FILE_BYTES).unwrap();
        assert_eq!(outgoing.next_chunk_index(), 0);

        outgoing.set_next_chunk(3).unwrap();
        assert_eq!(outgoing.next_chunk_index(), 3);

        outgoing.pause();
        assert!(outgoing.paused());
        outgoing.resume();
        assert!(!outgoing.paused());

        outgoing.mark_header_sent();
        assert!(outgoing.header_sent());
        outgoing.restart_from_beginning();
        assert_eq!(outgoing.next_chunk_index(), 0);
        assert!(!outgoing.header_sent());
    }

    #[test]
    fn transfer_resume_from_missing_chunk_index() {
        let dir = temp_dir("resume");
        let send_path = dir.join("payload.bin");
        let recv_dir = dir.join("recv");

        let payload = (0..15_000u32).map(|v| (v % 251) as u8).collect::<Vec<_>>();
        fs::write(&send_path, &payload).unwrap();

        let mut outgoing =
            OutgoingFile::from_path(&send_path, 777, 900, DEFAULT_MAX_FILE_BYTES).unwrap();
        let offer = outgoing.offer().clone();
        let mut incoming = IncomingFile::new(&recv_dir, offer, DEFAULT_MAX_FILE_BYTES).unwrap();

        for _ in 0..4 {
            let chunk = outgoing
                .next_chunk()
                .unwrap()
                .expect("expected chunk before interruption");
            incoming
                .write_chunk(chunk.chunk_index, &chunk.payload)
                .unwrap();
        }

        let resume_chunk = incoming.next_missing_chunk();
        assert_eq!(resume_chunk, outgoing.next_chunk_index());

        outgoing.pause();
        assert!(outgoing.paused());
        outgoing.resume();
        outgoing.set_next_chunk(resume_chunk).unwrap();

        while let Some(chunk) = outgoing.next_chunk().unwrap() {
            incoming
                .write_chunk(chunk.chunk_index, &chunk.payload)
                .unwrap();
        }

        assert!(incoming.is_complete());
        let out_path = incoming.finalize().unwrap();
        let received = fs::read(out_path).unwrap();
        assert_eq!(received, payload);
    }

    #[test]
    fn incoming_file_progress_and_abort() {
        let dir = temp_dir("incoming-abort");
        let recv_dir = dir.join("recv");
        fs::create_dir_all(&recv_dir).unwrap();

        let payload = vec![9u8; 2_400];
        let checksum = sha256_bytes_hex(&payload);
        let offer = FileOffer {
            file_id: 55,
            filename: "demo.bin".to_string(),
            file_size: payload.len() as u64,
            checksum_sha256: checksum,
            chunk_size: 600,
            total_chunks: 4,
        };

        let mut incoming = IncomingFile::new(&recv_dir, offer, DEFAULT_MAX_FILE_BYTES).unwrap();
        assert_eq!(incoming.received_count(), 0);
        assert_eq!(incoming.next_missing_chunk(), 0);

        incoming.write_chunk(0, &payload[0..600]).unwrap();
        incoming.write_chunk(1, &payload[600..1200]).unwrap();
        assert_eq!(incoming.received_count(), 2);
        assert_eq!(incoming.next_missing_chunk(), 2);

        let part_path = recv_dir.join("demo.bin.part");
        assert!(part_path.exists());
        incoming.abort().unwrap();
        assert!(!part_path.exists());
    }
}
