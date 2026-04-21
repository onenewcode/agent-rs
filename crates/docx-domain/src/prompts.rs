use agent_kernel::AgentFeedback;

pub struct WriterTemplates;

impl WriterTemplates {
    #[must_use]
    pub fn initial_task(goal: &str, doc: &str) -> String {
        format!(
            "You are a professional document editor. Your goal is to expand the following document based on the user prompt.\n\n\
            User Prompt: {goal}\n\n\
            Current Document:\n\n{doc}\n\n\
            Please provide the expanded document below."
        )
    }

    #[must_use]
    pub fn refinement_task(goal: &str, doc: &str, feedback: &AgentFeedback) -> String {
        format!(
            "You are a professional document editor. You previously expanded a document, but a reviewer provided the following feedback.\n\n\
            User Prompt: {goal}\n\n\
            Current Document:\n\n{doc}\n\n\
            Reviewer Feedback:\n\
            Score: {}/10\n\
            Suggestions: {:?}\n\
            Critical Errors: {:?}\n\n\
            Please revise the document based on the feedback above.",
            feedback.score, feedback.suggestions, feedback.critical_errors
        )
    }
}

pub struct ReviewerTemplates;

impl ReviewerTemplates {
    #[must_use]
    pub fn evaluation_task(goal: &str, doc: &str) -> String {
        format!(
            "You are a professional document reviewer. Evaluate the following document based on the user's expansion goal.\n\n\
            Expansion Goal: {goal}\n\n\
            Document Content:\n\n{doc}\n\n\
            Provide your review in JSON format with the following fields:\n\
            - score: 0-10\n\
            - passed: true/false (true if score >= 8 and no critical errors)\n\
            - suggestions: array of strings\n\
            - critical_errors: array of strings"
        )
    }
}

#[must_use]
pub fn count_tokens(text: &str) -> usize {
    text.split_whitespace().count() // Crude estimation
}
