use agent_kernel::{
    AgentContext, AgentTrajectory, BError, Error, ErrorType, Result, SearchProvider, SourceFetcher,
    TrajectoryStep,
};
use rig::completion::request::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Tool to edit a document by replacing old text with new text.
pub struct EditDocumentTool {
    pub current_content: Arc<RwLock<String>>,
    pub trajectory: Arc<Mutex<AgentTrajectory>>,
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
        traj.steps.push(TrajectoryStep::Thought {
            text: format!(
                "Surgically replaced `{}` with `{}`",
                args.old_text, args.new_text
            ),
            usage: None,
        });

        Ok(format!(
            "Successfully replaced text. New document length: {} characters.",
            content.len()
        ))
    }
}

/// Tool to search the web for information.
pub struct WebSearchTool {
    pub provider: Arc<dyn SearchProvider>,
    pub context: Arc<RwLock<AgentContext>>,
    pub trajectory: Arc<Mutex<AgentTrajectory>>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct WebSearchArgs {
    /// The search query
    pub query: String,
}

impl Tool for WebSearchTool {
    const NAME: &'static str = "web_search";
    type Error = BError;
    type Args = WebSearchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Searches the web for information using a search engine. Use this when you lack specific knowledge to fulfill the task.".to_string(),
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
        let results = self.provider.search(&args.query, 5).await?;

        // Format detailed results for the LLM to see immediately
        let mut formatted_results = Vec::new();
        for (i, res) in results.iter().enumerate() {
            formatted_results.push(json!({
                "index": i + 1,
                "title": res.title.clone().unwrap_or_else(|| "No Title".to_string()),
                "url": res.url,
                "snippet": res.content.chars().take(500).collect::<String>()
            }));
        }

        let mut context = self.context.write().await;
        context.search_results.extend(results.clone());

        let mut traj = self.trajectory.lock().await;
        traj.steps.push(TrajectoryStep::Action {
            tool: Self::NAME.to_string(),
            input: json!(args),
            output: format!("Found {} search results", results.len()),
        });

        Ok(format!(
            "Search completed. Found {} results. Use the actual URLs provided below to avoid 404s. Results: {}",
            results.len(),
            serde_json::to_string(&formatted_results).unwrap_or_default()
        ))
    }
}

/// Tool to fetch the content of a specific URL.
pub struct FetchUrlTool {
    pub fetcher: Arc<dyn SourceFetcher>,
    pub context: Arc<RwLock<AgentContext>>,
    pub trajectory: Arc<Mutex<AgentTrajectory>>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct FetchUrlArgs {
    /// The URL to fetch
    pub url: String,
}

impl Tool for FetchUrlTool {
    const NAME: &'static str = "fetch_url";
    type Error = BError;
    type Args = FetchUrlArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Fetches the content of a specific URL. Use this if the user provided a URL in the prompt or if you found a relevant URL in search results.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "The URL to fetch" }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let material = self.fetcher.fetch(&args.url).await?;

        let mut context = self.context.write().await;
        context.search_results.push(material.clone());

        let mut traj = self.trajectory.lock().await;
        traj.steps.push(TrajectoryStep::Action {
            tool: Self::NAME.to_string(),
            input: json!(args),
            output: format!("Fetched content from {}", args.url),
        });

        Ok(format!(
            "Successfully fetched content from {}. Content preview: {}",
            args.url,
            material.content.chars().take(1000).collect::<String>()
        ))
    }
}
