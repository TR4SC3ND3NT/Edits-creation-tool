//! Following `.edits/activity.jsonl` (`edits_core::activity`): the tool calls
//! of agents that work outside the studio (Claude Code or Codex in a
//! terminal, through `edits-mcp`).

use edits_core::activity::{self, Entry};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Most of the log read when a project is opened.
const FIRST_READ: u64 = 256 * 1024;

/// Reads what was appended to a log since the last poll.
#[derive(Default)]
pub struct LogTail {
    path: Option<PathBuf>,
    offset: u64,
    /// A line not finished yet.
    carry: String,
}

impl LogTail {
    /// New entries of the log at `path`. The first poll of a path (another
    /// project, or the first one) returns its last `replay` entries with
    /// `true`, so the page can list the agent's recent steps without
    /// highlighting them. A log that got shorter (moved aside when it grew
    /// too large) is read again from its start.
    pub fn poll(&mut self, path: &Path, replay: usize) -> (Vec<Entry>, bool) {
        let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if self.path.as_deref() != Some(path) {
            self.path = Some(path.to_path_buf());
            self.carry.clear();
            let from = len.saturating_sub(FIRST_READ);
            self.offset = len;
            let text = read_range(path, from, len);
            // A read that starts inside the file starts inside a line.
            let text = if from > 0 {
                text.split_once('\n')
                    .map(|(_, t)| t.to_string())
                    .unwrap_or_default()
            } else {
                text
            };
            let (whole, rest) = split_whole(&text);
            self.carry = rest.to_string();
            let entries = activity::parse(whole);
            let skip = entries.len().saturating_sub(replay);
            return (entries.into_iter().skip(skip).collect(), true);
        }
        if len < self.offset {
            self.offset = 0;
            self.carry.clear();
        }
        if len == self.offset {
            return (Vec::new(), false);
        }
        let mut text = std::mem::take(&mut self.carry);
        text.push_str(&read_range(path, self.offset, len));
        self.offset = len;
        let (whole, rest) = split_whole(&text);
        let entries = activity::parse(whole);
        self.carry = rest.to_string();
        (entries, false)
    }
}

/// The complete lines of `text`, and what follows the last newline.
fn split_whole(text: &str) -> (&str, &str) {
    match text.rfind('\n') {
        Some(i) => (&text[..=i], &text[i + 1..]),
        None => ("", text),
    }
}

fn read_range(path: &Path, from: u64, to: u64) -> String {
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    if f.seek(SeekFrom::Start(from)).is_err() {
        return String::new();
    }
    let mut buf = Vec::with_capacity((to - from) as usize);
    let _ = f.take(to - from).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use edits_core::activity::{append, Phase};
    use std::io::Write;

    fn entry(call: &str, label: &str) -> Entry {
        Entry {
            at: "20261114T100000.000Z".into(),
            call: call.into(),
            tool: "preset_apply".into(),
            actor: "agent".into(),
            label: label.into(),
            layers: vec!["d".into()],
            time: Some("b12".into()),
            t: Some(12.0),
            ..Default::default()
        }
    }

    #[test]
    fn the_tail_replays_the_end_then_reads_what_is_appended_line_by_line() {
        let dir = std::env::temp_dir().join(format!("edits-studio-tail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("project.json");
        let log = activity::path(&project);
        let mut tail = LogTail::default();
        // No log yet: nothing, and the first log that appears is read from its start.
        assert_eq!(tail.poll(&log, 2), (vec![], true));
        for i in 0..3 {
            append(&log, &entry(&format!("c{i}"), &format!("step {i}"))).unwrap();
        }
        let (got, replay) = tail.poll(&log, 2);
        assert!(!replay);
        assert_eq!(
            got.iter().map(|e| e.call.as_str()).collect::<Vec<_>>(),
            ["c0", "c1", "c2"]
        );

        // A second follower, opened now, replays the last two.
        let mut late = LogTail::default();
        let (got, replay) = late.poll(&log, 2);
        assert!(replay);
        assert_eq!(
            got.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
            ["step 1", "step 2"]
        );

        // A line written in two pieces is read once it is whole.
        let line = serde_json::to_string(&Entry {
            phase: Phase::End,
            ok: Some(true),
            ..entry("c2", "step 2")
        })
        .unwrap();
        let (a, b) = line.split_at(20);
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        f.write_all(a.as_bytes()).unwrap();
        f.flush().unwrap();
        assert_eq!(tail.poll(&log, 2).0, vec![]);
        f.write_all(format!("{b}\n").as_bytes()).unwrap();
        f.flush().unwrap();
        let got = tail.poll(&log, 2).0;
        assert_eq!(
            (got.len(), got[0].phase, got[0].ok),
            (1, Phase::End, Some(true))
        );

        // A log moved aside starts again from the top.
        std::fs::remove_file(&log).unwrap();
        append(&log, &entry("c9", "after the move")).unwrap();
        assert_eq!(tail.poll(&log, 2).0[0].call, "c9");
        std::fs::remove_dir_all(&dir).ok();
    }
}
