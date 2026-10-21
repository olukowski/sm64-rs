//! Signature scanning is deliberately generic, not a revision-specific asset map.

use serde::Serialize;

const MAX_CANDIDATES: usize = 4096;
const MAX_SCAN_OUTPUT: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NotChecked,
    Valid,
    Invalid,
    Skipped,
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub offset: usize,
    pub decoded_size: Option<u32>,
    pub compressed_offset: Option<u32>,
    pub literal_offset: Option<u32>,
    pub encoded_size: Option<usize>,
    pub status: Status,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Inventory {
    pub candidate_count: usize,
    pub reported_count: usize,
    pub valid_count: usize,
    pub invalid_count: usize,
    pub skipped_count: usize,
    pub not_checked_count: usize,
    pub validation_requested: bool,
    pub per_block_output_limit: usize,
    pub aggregate_output_limit: usize,
    pub candidate_limit: usize,
    pub scan_truncated: bool,
    pub entries: Vec<Entry>,
    pub note: &'static str,
}

pub fn inventory(bytes: &[u8], validate: bool, max_output: usize) -> Inventory {
    let mut report = Inventory {
        candidate_count: 0,
        reported_count: 0,
        valid_count: 0,
        invalid_count: 0,
        skipped_count: 0,
        not_checked_count: 0,
        validation_requested: validate,
        per_block_output_limit: max_output,
        aggregate_output_limit: MAX_SCAN_OUTPUT,
        candidate_limit: MAX_CANDIDATES,
        scan_truncated: false,
        entries: Vec::new(),
        note: "MIO0 signatures are candidates, not an asset table. Successful decoding does not identify asset type. Offsets use canonical big-endian ROM bytes.",
    };
    let mut charged_output = 0usize;
    for (offset, _) in bytes
        .windows(4)
        .enumerate()
        .filter(|(_, bytes)| *bytes == b"MIO0")
    {
        report.candidate_count += 1;
        if report.entries.len() == MAX_CANDIDATES {
            report.scan_truncated = true;
            continue;
        }
        let mut entry = Entry {
            offset,
            decoded_size: None,
            compressed_offset: None,
            literal_offset: None,
            encoded_size: None,
            status: Status::NotChecked,
            error: None,
        };
        match sm64_mio0::Header::parse(&bytes[offset..]) {
            Err(error) => {
                entry.status = Status::Invalid;
                entry.error = Some(error.to_string());
            }
            Ok(header) => {
                entry.decoded_size = Some(header.decoded_size);
                entry.compressed_offset = Some(header.compressed_offset);
                entry.literal_offset = Some(header.literal_offset);
                if validate {
                    let size = header.decoded_size as usize;
                    if size > max_output {
                        entry.status = Status::Skipped;
                        entry.error = Some(format!(
                            "decoded size {size} exceeds per-block limit {max_output}"
                        ));
                    } else if size > MAX_SCAN_OUTPUT - charged_output {
                        entry.status = Status::Skipped;
                        entry.error = Some("aggregate scan output budget exhausted".into());
                    } else {
                        // Charge even failed decodes: they may have done substantial work.
                        charged_output += size;
                        match sm64_mio0::decode(&bytes[offset..], max_output) {
                            Ok(decoded) => {
                                entry.status = Status::Valid;
                                entry.encoded_size = Some(decoded.consumed);
                            }
                            Err(error) => {
                                entry.status = Status::Invalid;
                                entry.error = Some(error.to_string());
                            }
                        }
                    }
                }
            }
        }
        match entry.status {
            Status::Valid => report.valid_count += 1,
            Status::Invalid => report.invalid_count += 1,
            Status::Skipped => report.skipped_count += 1,
            Status::NotChecked => report.not_checked_count += 1,
        }
        report.entries.push(entry);
    }
    report.reported_count = report.entries.len();
    report
}

impl Inventory {
    pub fn print(&self, output: &mut impl std::io::Write) -> std::io::Result<()> {
        writeln!(
            output,
            "MIO0 candidates: {} ({} reported; {} valid, {} invalid, {} skipped, {} unchecked)",
            self.candidate_count,
            self.reported_count,
            self.valid_count,
            self.invalid_count,
            self.skipped_count,
            self.not_checked_count
        )?;
        writeln!(output, "OFFSET      DECODED      ENCODED      STATUS")?;
        for entry in &self.entries {
            let decoded = entry
                .decoded_size
                .map_or_else(|| "-".into(), |size| size.to_string());
            let encoded = entry
                .encoded_size
                .map_or_else(|| "-".into(), |size| size.to_string());
            writeln!(
                output,
                "0x{:08x}  {:>10}  {:>10}  {:?}{}",
                entry.offset,
                decoded,
                encoded,
                entry.status,
                entry
                    .error
                    .as_ref()
                    .map_or_else(String::new, |error| format!(": {error}"))
            )?;
        }
        writeln!(output, "Note: {}", self.note)?;
        if !self.validation_requested {
            writeln!(
                output,
                "Note: pass --validate to decode candidates and check stream structure."
            )?;
        }
        if self.scan_truncated {
            writeln!(
                output,
                "Warning: only the first {} candidates are reported/validated.",
                self.candidate_limit
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> Vec<u8> {
        let mut bytes = b"MIO0".to_vec();
        for word in [1u32, 17, 17] {
            bytes.extend(word.to_be_bytes());
        }
        bytes.extend([0x80, b'X']);
        bytes
    }

    #[test]
    fn scans_unaligned_signatures_and_validates_when_requested() {
        let mut bytes = vec![0; 3];
        bytes.extend(block());
        bytes.extend(b"MIO0"); // Truncated header, not hidden or treated as valid.
        let report = inventory(&bytes, true, 1024);
        assert_eq!(report.candidate_count, 2);
        assert_eq!(report.valid_count, 1);
        assert_eq!(report.invalid_count, 1);
        assert_eq!(report.entries[0].offset, 3);
        assert_eq!(report.entries[0].encoded_size, Some(18));
    }

    #[test]
    fn header_scan_does_not_claim_decoding_validation() {
        let report = inventory(&block(), false, 1024);
        assert_eq!(report.not_checked_count, 1);
        assert_eq!(report.valid_count, 0);
        assert_eq!(report.entries[0].encoded_size, None);
    }

    #[test]
    fn oversized_candidates_are_skipped_without_claiming_invalidity() {
        let mut bytes = block();
        bytes[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
        let report = inventory(&bytes, true, 1024);
        assert_eq!(report.skipped_count, 1);
        assert_eq!(report.invalid_count, 0);
        assert!(report.entries[0].error.as_ref().unwrap().contains("limit"));
    }

    #[test]
    fn valid_block_over_budget_is_skipped_not_malformed() {
        let mut bytes = block();
        bytes[4..8].copy_from_slice(&3u32.to_be_bytes());
        bytes[16] = 0xe0;
        bytes.extend(b"YZ");
        assert_eq!(inventory(&bytes, true, 3).valid_count, 1);
        let report = inventory(&bytes, true, 2);
        assert_eq!(report.skipped_count, 1);
        assert_eq!(report.invalid_count, 0);
    }

    #[test]
    fn aggregate_budget_charges_failed_attempts_too() {
        let mut bytes = block();
        bytes[4..8].copy_from_slice(&(16u32 * 1024 * 1024).to_be_bytes());
        let report = inventory(&bytes.repeat(17), true, 16 * 1024 * 1024);
        assert_eq!(report.invalid_count, 16);
        assert_eq!(report.skipped_count, 1);
        assert!(
            report.entries[16]
                .error
                .as_ref()
                .unwrap()
                .contains("aggregate")
        );
    }

    #[test]
    fn candidate_cap_is_explicit_and_total_is_still_counted() {
        let bytes = b"MIO0".repeat(MAX_CANDIDATES + 3);
        let report = inventory(&bytes, false, 1024);
        assert_eq!(report.candidate_count, MAX_CANDIDATES + 3);
        assert_eq!(report.reported_count, MAX_CANDIDATES);
        assert!(report.scan_truncated);
    }
}
