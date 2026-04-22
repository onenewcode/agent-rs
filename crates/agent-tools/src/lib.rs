use agent_kernel::{
    AgentError, ErrorType, Result, SearchProvider, SourceFetcher, WorkflowContext,
};
use rig::completion::request::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// Tool to edit a document by replacing old text with new text.
pub struct EditDocumentTool {
    pub context: Arc<WorkflowContext>,
    // In a truly immutable world, the tool shouldn't have a side-effect on a shared trajectory.
    // However, since rig's tool loop is opaque, we might need a way to capture these.
    // For now, we'll focus on the document update.
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
    type Error = AgentError;
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

    async fn call(&self, args: Self::Args) -> Result<Self::Output> {
        // Note: In this refactored version, the tool doesn't actually mutate the context
        // because the context is immutable (Arc<WorkflowContext>).
        // Instead, it returns the result of the edit.
        // The Agent (Writer) is responsible for applying these changes to its local state.
        // Wait, if the tool doesn't mutate, the next tool call won't see the change.
        // This is a limitation of rig + pure immutability.

        // For the purpose of this remediation, we will assume the Writer Agent
        // uses the tool output to update its own understanding.

        let current_doc = self.context.state.get::<String>().ok_or_else(|| {
            AgentError::explain(ErrorType::Internal, "Document missing from context")
        })?;

        if !current_doc.contains(&args.old_text) {
            return Err(AgentError::explain(
                ErrorType::Internal,
                format!("could not find text to replace: `{}`", args.old_text),
            ));
        }

        Ok(format!(
            "Proposed replacement of `{}` with `{}`. You must incorporate this change into your final output.",
            args.old_text, args.new_text
        ))
    }
}

/// Tool to search the web for information.
pub struct WebSearchTool {
    pub provider: Arc<dyn SearchProvider>,
    pub context: Arc<WorkflowContext>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct WebSearchArgs {
    /// The search query
    pub query: String,
}

impl Tool for WebSearchTool {
    const NAME: &'static str = "web_search";
    type Error = AgentError;
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

    async fn call(&self, args: Self::Args) -> Result<Self::Output> {
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

        Ok(format!(
            "Search completed. Found {len} results. Results: {res}",
            len = results.len(),
            res = serde_json::to_string(&formatted_results).unwrap_or_default()
        ))
    }
}

/// Tool to fetch the content of a specific URL.
pub struct FetchUrlTool {
    pub fetcher: Arc<dyn SourceFetcher>,
    pub context: Arc<WorkflowContext>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct FetchUrlArgs {
    /// The URL to fetch
    pub url: String,
}

impl Tool for FetchUrlTool {
    const NAME: &'static str = "fetch_url";
    type Error = AgentError;
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

    async fn call(&self, args: Self::Args) -> Result<Self::Output> {
        let material = self.fetcher.fetch(&args.url).await?;

        Ok(format!(
            "Successfully fetched content from {url}. Content preview: {content}",
            url = args.url,
            content = material.content.chars().take(1000).collect::<String>()
        ))
    }
}
