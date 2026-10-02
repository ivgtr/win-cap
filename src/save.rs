use std::{
    fs::{File, OpenOptions},
    path::Path,
};

// The recording stays intact until the complete copy has been flushed. This
// also handles a destination on another drive, where rename would fail.
pub fn recording(source: &Path, destination: &Path) -> Result<Option<String>, String> {
    let mut input = File::open(source)
        .map_err(|e| format!("録画ファイルを開けません: {e}\n録画: {}", source.display()))?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|e| format!("保存先を作成できません（既存ファイルは上書きしません）: {e}"))?;
    let copied = std::io::copy(&mut input, &mut output).and_then(|_| output.sync_all());
    drop(output);
    drop(input);
    if let Err(error) = copied {
        let cleanup = match std::fs::remove_file(destination) {
            Ok(()) => String::new(),
            Err(e) => format!(
                "\n不完全な保存先を削除できません: {e}\n保存先: {}",
                destination.display()
            ),
        };
        return Err(format!(
            "MP4を保存できません: {error}{cleanup}\n録画は残っています: {}",
            source.display()
        ));
    }
    Ok(std::fs::remove_file(source).err().map(|e| {
        format!(
            "保存は完了しましたが、一時録画を削除できません: {e}\n一時録画: {}",
            source.display()
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn failed_save_keeps_source_and_success_removes_it() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder =
            std::env::temp_dir().join(format!("win-cap-save-test-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&folder).unwrap();
        let source = folder.join("recording.mp4");
        let existing = folder.join("existing.mp4");
        let saved = folder.join("saved.mp4");
        std::fs::write(&source, b"recorded-video").unwrap();
        std::fs::write(&existing, b"keep-existing").unwrap();
        assert!(recording(&source, &existing).is_err());
        assert_eq!(std::fs::read(&existing).unwrap(), b"keep-existing");
        assert_eq!(std::fs::read(&source).unwrap(), b"recorded-video");
        assert!(recording(&source, &folder.join("missing").join("video.mp4")).is_err());
        assert!(source.exists());
        assert!(recording(&source, &saved).unwrap().is_none());
        assert_eq!(std::fs::read(&saved).unwrap(), b"recorded-video");
        assert!(!source.exists());
        std::fs::remove_dir_all(folder).unwrap();
    }
}
