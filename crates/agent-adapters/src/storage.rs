use agent_kernel::{ArtifactStore, ErrorType, OrErr, Result, RunReport};
use std::path::PathBuf;

pub struct FileArtifactStore {
    output_dir: PathBuf,
}

impl FileArtifactStore {
    #[must_use]
    pub fn new(output_dir: PathBuf) -> Self {
        Self { output_dir }
    }
}

impl ArtifactStore for FileArtifactStore {
    fn persist(&self, report: &RunReport) -> agent_kernel::BoxFuture<'_, Result<()>> {
        let output_dir = self.output_dir.clone();
        let report = report.clone();

        Box::pin(async move {
            if !output_dir.exists() {
                tokio::fs::create_dir_all(&output_dir)
                    .await
                    .or_err(ErrorType::Artifact, "failed to create artifact directory")?;
            }

            let file_path = output_dir.join(format!("{}.json", report.run_id));
            let json = serde_json::to_string_pretty(&report)
                .or_err(ErrorType::Artifact, "failed to serialize run report")?;

            tokio::fs::write(file_path, json)
                .await
                .or_err(ErrorType::Artifact, "failed to write run report to disk")?;

            Ok(())
        })
    }
}
