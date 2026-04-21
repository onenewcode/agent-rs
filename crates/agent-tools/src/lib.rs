use agent_kernel::{AgentTrajectory, BError, Error, ErrorType, Result, TrajectoryStep};
use rig::completion::request::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// Tool to edit a document by replacing old text with new text.
pub struct EditDocumentTool {
    pub current_content: Arc<tokio::sync::RwLock<String>>,
    pub trajectory: Arc<tokio::sync::Mutex<AgentTrajectory>>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct EditDocumentArgs {
    /// The exact text to be replaced
    pub old_text: String,
    /// The new text to insert instead
    pub new_text: String,
}

impl Tool for EditDocumentTool {
    const NAME: &'static str = "edit_document";
    type Error = BError;
    type Args = EditDocumentArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Replaces specific text in the current document draft with new content. Use this to fix identified issues surgically.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "old_text": { "type": "string", "description": "The exact text to be replaced" },
                    "new_text": { "type": "string", "description": "The new text to insert instead" }
                },
                "required": ["old_text", "new_text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut content = self.current_content.write().await;
        if !content.contains(&args.old_text) {
            return Err(Box::new(Error::explain(
                ErrorType::Tool,
                format!("could not find text to replace: `{}`", args.old_text),
            )));
        }

        let new_content = content.replace(&args.old_text, &args.new_text);
        *content = new_content;

        let mut traj = self.trajectory.lock().await;
        traj.steps.push(TrajectoryStep::Thought(format!(
            "Surgically replaced `{}` with `{}`",
            args.old_text, args.new_text
        )));

        Ok(format!(
            "Successfully replaced text. New document length: {} characters.",
            content.len()
        ))
    }
}
