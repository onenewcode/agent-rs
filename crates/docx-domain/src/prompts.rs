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
    pub fn evaluation_task(goal: &str, doc: &str, sources: &[SourceMaterial]) -> String {
        let sources_str = if sources.is_empty() {
            "No grounding sources available.".to_string()
        } else {
            sources
                .iter()
                .map(|s| format!("URL: {}\nContent: {}", s.url, s.content))
                .collect::<Vec<_>>()
                .join("\n\n")
        };

        format!(
            "You are a scientific document auditor. Your mission is to perform a rigorous evaluation of the expanded document.\n\n\
            ## Evaluation Criteria\n\
            1. Groundedness (40%): Every claim must be supported by the provided sources. Detect hallucinations.\n\
            2. Relevance (30%): Does it fulfill the user's expansion goal?\n\
            3. Coherence & Accuracy (30%): Is the logical flow and factual information correct?\n\n\
            ## Task Context\n\
            Goal: {goal}\n\n\
            Grounding Sources:\n\
            {sources_str}\n\n\
            ## Document to Review\n\
            {doc}\n\n\
            ## Your Analysis Protocol\n\
            1. Step 1: Identify all specific claims in the document.\n\
            2. Step 2: For each claim, find the supporting evidence in the Grounding Sources.\n\
            3. Step 3: Flag any claims that are unsupported (hallucinations) or contradict the sources.\n\
            4. Step 4: Calculate the final score (0-100).\n\n\
            ## Output Format\n\
            Provide your final verdict in valid JSON format. \n\
            - score: 0-100 (integer)\n\
            - passed: true if score >= 80 and no hallucinations/critical errors.\n\
            - suggestions: Specific, actionable instructions for the Writer.\n\
            - critical_errors: Hallucinations, factual contradictions, or failure to meet the goal.\n\n\
            JSON Output ONLY."
        )
    }
}

#[must_use]
pub fn count_tokens(text: &str) -> usize {
    text.split_whitespace().count() // Crude estimation
}
