// SPDX-FileCopyrightText: 2026 The igs-iec61850-mms contributors
//
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use igs_iso9506_mms::vmd::FileStore;
use libfuzzer_sys::fuzz_target;

/// One of a few names, so that the operations meet the same files.
fn name(choice: u8) -> Vec<String> {
    match choice % 4 {
        0 => vec!["a".to_string()],
        1 => vec!["a".to_string(), "b".to_string()],
        2 => vec!["b".to_string()],
        _ => Vec::new(),
    }
}

fuzz_target!(|data: &[u8]| {
    // Four octets per operation: the kind, the name, the value, and the content length. The operations are the services of the
    // file store: add, open, read, close, delete, list and file. Every input answers with a value or a refusal.
    let mut store = FileStore::default();
    let mut open: Vec<i64> = Vec::new();
    for operation in data.chunks(4) {
        let kind = operation.first().copied().unwrap_or(0);
        let path = name(operation.get(1).copied().unwrap_or(0));
        let value = operation.get(2).copied().unwrap_or(0);
        let length = usize::from(operation.get(3).copied().unwrap_or(0) % 32);
        match kind % 7 {
            0 => {
                store.add(path.clone(), vec![value; length], None);
                assert!(store.file(&path).is_some(), "an added file is stored");
            }
            1 => {
                if let Ok(frsm) = store.open(&path, u64::from(value)) {
                    open.push(frsm);
                }
            }
            2 => {
                if let Some(&frsm) = open.last() {
                    if let Ok((part, _more)) = store.read(frsm, usize::from(value)) {
                        assert!(part.len() <= usize::from(value), "a read gives at most its limit");
                    }
                }
            }
            3 => {
                if let Some(frsm) = open.pop() {
                    let _ = store.close(frsm);
                }
            }
            4 => {
                let _ = store.delete(&path);
            }
            5 => {
                let _ = store.list(&path, None);
            }
            _ => {
                let _ = store.size_of_open(open.last().copied().unwrap_or(0));
            }
        }
    }
});
