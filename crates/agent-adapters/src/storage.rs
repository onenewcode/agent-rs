use std::path::PathBuf;

use agent_kernel::{ArtifactStore, RunError, RunReport};
use tokio::fs;

#[derive(Debug, Clone)]
pub struct JsonFileArtifactStore {
    dir: PathBuf,
}

impl JsonFileArtifactStore {
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
}

impl ArtifactStore for JsonFileArtifactStore {
    fn persist(&self, report: &RunReport) -> agent_kernel::BoxFuture<'_, Result<(), RunError>> {
        let dir = self.dir.clone();
        let filename = format!("{}-{}.json", report.workflow, report.run_id);
        let payload = serde_json::to_vec_pretty(report).map_err(|error| {
            RunError::Internal(format!("failed to serialize run report for persistence: {error}"))
        });

        Box::pin(async move {
            let payload = payload?;
            fs::create_dir_all(&dir)
                .await
                .map_err(|error| RunError::Internal(error.to_string()))?;
            let path = dir.join(filename);
            fs::write(&path, payload)
                .await
                .map_err(|error| RunError::Internal(error.to_string()))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use agent_kernel::ArtifactStore;
    use filetime::FileTime;
    use tokio::fs;

    use super::JsonFileArtifactStore;

    #[tokio::test]
    async fn writes_json_reports() {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let dir = std::env::temp_dir().join(format!("agent-rs-artifacts-{millis}"));
        let store = JsonFileArtifactStore::new(&dir);
        let report = agent_kernel::RunReport {
            run_id: "run-1".to_owned(),
            workflow: "docx.expand".to_owned(),
            qualified: true,
            output_artifact: None,
            artifacts: Vec::new(),
            events: Vec::new(),
            total_duration_ms: 1,
        };

        store.persist(&report).await.expect("store should write");

        let mut entries = fs::read_dir(&dir).await.expect("dir should exist");
        let entry = entries
            .next_entry()
            .await
            .expect("dir should be readable")
            .expect("report file should exist");
        let mtime = FileTime::from_last_modification_time(
            &std::fs::metadata(entry.path()).expect("metadata should be readable"),
        );
        assert!(mtime.unix_seconds() > 0);
    }
}
