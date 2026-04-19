use agent_core::{
    BoxFuture, EvaluationRequest, EvaluationResult, EvaluatorRuntime, ExpansionError,
};
use rig::completion::Prompt;
use tracing::{info, warn};

pub struct EvaluatorService<P: Prompt> {
    agent: P,
    template: String,
    max_attempts: usize,
}

impl<P: Prompt> EvaluatorService<P> {
    #[must_use]
    pub fn new(agent: P, template: String, max_attempts: usize) -> Self {
        Self {
            agent,
            template,
            max_attempts,
        }
    }

    fn sanitize_json_response(content: &str) -> String {
        let trimmed = content.trim();
        // Try to find the first { and last } to extract JSON from potentially messy LLM output
        if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}'))
            && start <= end
        {
            return trimmed[start..=end].to_owned();
        }
        trimmed.to_owned()
    }
}

impl<P: Prompt + Send + Sync> EvaluatorRuntime for EvaluatorService<P> {
    fn evaluate(
        &self,
        request: EvaluationRequest,
    ) -> BoxFuture<'_, Result<EvaluationResult, ExpansionError>> {
        let mut sources_text = String::new();
        for (i, source) in request.sources.iter().enumerate() {
            sources_text.push_str(&format!(
                "Source {}:\nURL: {}\nTitle: {:?}\nContent Summary: {}\n\n",
                i + 1,
                source.url,
                source.title,
                agent_core::truncate_chars(&source.content, 1000)
            ));
        }

        let prompt = self
            .template
            .replace("{prompt}", &request.prompt)
            .replace("{content}", &request.content)
            .replace("{sources}", &sources_text);

        Box::pin(async move {
            for attempt in 1..=self.max_attempts {
                info!(attempt, "Attempting evaluation");
                match self.agent.prompt(&prompt).await {
                    Ok(response) => {
                        let sanitized = Self::sanitize_json_response(&response);
                        match serde_json::from_str::<EvaluationResult>(&sanitized) {
                            Ok(result) => {
                                info!(score = result.score, "Evaluation successful");
                                return Ok(result);
                            }
                            Err(e) => {
                                warn!(
                                    attempt,
                                    error = %e,
                                    response = %response,
                                    "Failed to parse evaluation result as JSON"
                                );
                                if attempt == self.max_attempts {
                                    return Err(ExpansionError::Evaluation(format!(
                                        "Failed to parse JSON after {} attempts: {}",
                                        self.max_attempts, e
                                    )));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(attempt, error = %e, "LLM prompt failed during evaluation");
                        if attempt == self.max_attempts {
                            return Err(ExpansionError::Evaluation(format!(
                                "LLM prompt failed: {}",
                                e
                            )));
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Err(ExpansionError::Evaluation(
                "Evaluation exhausted retries".to_owned(),
            ))
        })
    }
}
