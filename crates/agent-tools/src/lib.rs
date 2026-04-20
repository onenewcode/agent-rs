use agent_kernel::AgentTrajectory;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

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

#[derive(Debug, thiserror::Error)]
#[error("Tool error: {0}")]
pub struct ToolError(String);

impl Tool for EditDocumentTool {
    const NAME: &'static str = "edit_document";
    type Error = ToolError;
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
        let result = if content.contains(&args.old_text) {
            info!(old = %args.old_text, new = %args.new_text, "EditDocumentTool: replacing text");
            *content = content.replace(&args.old_text, &args.new_text);
            "Successfully updated document.".to_string()
        } else {
            format!(
                "Error: Could not find exact text '{}' in document.",
                args.old_text
            )
        };

        let mut traj = self.trajectory.lock().await;
        traj.steps.push(agent_kernel::TrajectoryStep::Action {
            tool: Self::NAME.to_string(),
            input: serde_json::to_value(&args).map_err(|e| ToolError(e.to_string()))?,
            output: result.clone(),
        });

        Ok(result)
    }
}

/// Tool to perform a web search for missing information.
pub struct WebSearchTool {
    pub provider: Arc<dyn agent_kernel::SearchProvider>,
    pub trajectory: Arc<tokio::sync::Mutex<AgentTrajectory>>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct WebSearchArgs {
    /// The search query
    pub query: String,
}

impl Tool for WebSearchTool {
    const NAME: &'static str = "web_search";
    type Error = ToolError;
    type Args = WebSearchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description:
                "Searches the web for specific information or facts to improve the document."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query" }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        info!(query = %args.query, "WebSearchTool: searching");
        let result = match self.provider.search(&args.query, 3).await {
            Ok(results) => results
                .into_iter()
                .map(|r| {
                    format!(
                        "Title: {}\nURL: {}\nContent: {}\n",
                        r.title.unwrap_or_default(),
                        r.url,
                        r.content
                    )
                })
                .collect::<Vec<_>>()
                .join("\n---\n"),
            Err(e) => format!("Search failed: {e}"),
        };

        let mut traj = self.trajectory.lock().await;
        traj.steps.push(agent_kernel::TrajectoryStep::Action {
            tool: Self::NAME.to_string(),
            input: serde_json::to_value(&args).map_err(|e| ToolError(e.to_string()))?,
            output: result.clone(),
        });

        Ok(result)
    }
}
