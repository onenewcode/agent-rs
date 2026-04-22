use agent_kernel::{
    AgentContext, AgentTrajectory, BError, Error, ErrorType, Result, SearchProvider, SourceFetcher,
    TrajectoryStep,
};
use rig::completion::request::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

/// Tool to edit a document by replacing old text with new text.
pub struct EditDocumentTool {
    pub context: Arc<RwLock<AgentContext>>,
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
        let start = Instant::now();
        let mut context = self.context.write().await;
        if !context.current_document.contains(&args.old_text) {
            return Err(Box::new(Error::explain(
                ErrorType::Tool,
                format!("could not find text to replace: `{}`", args.old_text),
            )));
        }

        let new_content = context
            .current_document
            .replacen(&args.old_text, &args.new_text, 1);
        context.current_document = new_content;

        let duration = start.elapsed().as_millis();
        #[allow(clippy::cast_possible_truncation)]
        let duration = duration as u64;
        let mut traj = self.trajectory.lock().await;
        traj.steps.push(TrajectoryStep::Thought {
            text: format!(
                "Surgically replaced `{old_text}` with `{new_text}`",
                old_text = args.old_text,
                new_text = args.new_text
            ),
            usage: None,
            duration_ms: Some(duration),
        });

        Ok(format!(
            "Successfully replaced text. New document length: {len} characters.",
            len = context.current_document.len()
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
        let start = Instant::now();
        let results = self.provider.search(&args.query, 5).await?;
        let duration = start.elapsed().as_millis();
        #[allow(clippy::cast_possible_truncation)]
        let duration = duration as u64;

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
            output: format!("Found {len} search results", len = results.len()),
            is_error: false,
            duration_ms: Some(duration),
        });

        Ok(format!(
            "Search completed. Found {len} results. Use the actual URLs provided below to avoid 404s. Results: {res}",
            len = results.len(),
            res = serde_json::to_string(&formatted_results).unwrap_or_default()
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
        let start = Instant::now();
        let material = self.fetcher.fetch(&args.url).await?;
        let duration = start.elapsed().as_millis();
        #[allow(clippy::cast_possible_truncation)]
        let duration = duration as u64;

        let mut context = self.context.write().await;
        context.search_results.push(material.clone());

        let mut traj = self.trajectory.lock().await;
        traj.steps.push(TrajectoryStep::Action {
            tool: Self::NAME.to_string(),
            input: json!(args),
            output: format!("Fetched content from {url}", url = args.url),
            is_error: false,
            duration_ms: Some(duration),
        });

        Ok(format!(
            "Successfully fetched content from {url}. Content preview: {content}",
            url = args.url,
            content = material.content.chars().take(1000).collect::<String>()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_kernel::AgentContext;

    #[tokio::test]
    async fn test_edit_document_tool_replaces_only_first_occurrence() {
        let context = Arc::new(RwLock::new(AgentContext::new("test".to_string(), "a b a c".to_string())));
        let trajectory = Arc::new(Mutex::new(AgentTrajectory::default()));
        let tool = EditDocumentTool {
            context: context.clone(),
            trajectory,
        };

        let args = EditDocumentArgs {
            old_text: "a".to_string(),
            new_text: "X".to_string(),
        };

        let result = tool.call(args).await.unwrap();
        assert!(result.contains("New document length: 7 characters."));

        let final_doc = context.read().await.current_document.clone();
        assert_eq!(final_doc, "X b a c");
    }
}
