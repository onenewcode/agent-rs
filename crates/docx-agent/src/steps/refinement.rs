use agent_core::{
    BoxFuture, ExpansionError, ExpansionRequest, ExpansionResult, ResearchResult, Step,
};
use tracing::info;

pub struct RefinementStep {
    refinement_template: String,
}

impl RefinementStep {
    #[must_use]
    pub fn new(refinement_template: String) -> Self {
        Self {
            refinement_template,
        }
    }
}

impl Step for RefinementStep {
    fn name(&self) -> &str {
        "Refinement"
    }

    fn execute<'a>(
        &self,
        request: &'a mut ExpansionRequest,
        current_result: Option<ExpansionResult>,
        research: Option<ResearchResult>,
    ) -> BoxFuture<'a, Result<(Option<ExpansionResult>, Option<ResearchResult>), ExpansionError>>
    {
        let template = self.refinement_template.clone();

        Box::pin(async move {
            info!("Starting refinement preparation step");
            let result = current_result.ok_or_else(|| {
                ExpansionError::Internal("Refinement step requires a prior result".to_owned())
            })?;

            // Update the request prompt for the next generation attempt
            let refinement_prompt = template
                .replace("{prompt}", &request.prompt)
                .replace("{content}", &result.content)
                .replace(
                    "{reason}",
                    result.evaluation_reason.as_deref().unwrap_or(""),
                );

            request.prompt = refinement_prompt;

            Ok((Some(result), research))
        })
    }
}
