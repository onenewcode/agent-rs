use agent_kernel::{AgentFeedback, SourceMaterial};

pub struct WriterTemplates;

impl WriterTemplates {
    #[must_use]
    pub fn initial_task(goal: &str, doc: &str) -> String {
        format!(
            "You are a professional document editor. Your goal is to expand the following document based on the user prompt.\n\n\
            User Prompt: {goal}\n\n\
            Current Document:\n\n{doc}\n\n\
            Please provide the expanded document below. If you use tools to edit the document, you don't need to output the full document, just confirm you are done."
        )
    }

    #[must_use]
    pub fn refinement_task(
        goal: &str,
        doc: &str,
        feedback_history: &[AgentFeedback],
        search_results: &[SourceMaterial],
    ) -> String {
        let feedback_str = feedback_history
            .iter()
            .enumerate()
            .map(|(i, f)| {
                format!(
                    "Round {} Feedback:\nScore: {}/10\nSuggestions: {:?}\nCritical Errors: {:?}",
                    i + 1,
                    f.score,
                    f.suggestions,
                    f.critical_errors
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let search_str = if search_results.is_empty() {
            "No previous search results.".to_string()
        } else {
            search_results
                .iter()
                .map(|s| {
                    format!(
                        "URL: {}\nContent Preview: {}",
                        s.url,
                        s.content.chars().take(300).collect::<String>()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        };

        format!(
            "You are a professional document editor. You previously expanded a document, but a reviewer provided feedback. You have access to the full feedback history.\n\n\
            User Prompt: {goal}\n\n\
            Current Document:\n\n{doc}\n\n\
            Previous Search Results:\n{search_str}\n\n\
            Reviewer Feedback History:\n\
            {feedback_str}\n\n\
            Please revise the document based on the feedback above. Use your tools to surgically edit the document (`edit_document`). If you must, output the entire document, but `edit_document` is preferred for long documents."
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
            - score: 0-10 (integer)\n\
            - passed: true/false (boolean, true if score >= 8 and no critical errors)\n\
            - suggestions: array of strings\n\
            - critical_errors: array of strings\n\n\
            OUTPUT ONLY VALID JSON. Do not include markdown code blocks like ```json."
        )
    }
}

#[must_use]
pub fn count_tokens(text: &str) -> usize {
    text.split_whitespace().count() // Crude estimation
}
