use agent_kernel::AgentFeedback;

#[must_use]
pub fn count_tokens(text: &str) -> usize {
    text.chars().count() / 4
}

pub struct WriterTemplates;

impl WriterTemplates {
    pub fn initial_task(goal: &str, doc: &str) -> String {
        format!(
            "Role: You are a Senior Technical Writer and Subject Matter Expert.\n\
             Goal: {goal}\n\n\
             Task: Expand the current document to be more comprehensive, detailed, and insightful.\n\n\
             Current Document Content:\n\
             ---\n\
             {doc}\n\
             ---\n\n\
             Instructions:\n\
             1. **Research First**: Use the `web_search` tool to gather latest data, technical details, and expert context related to the goal.\n\
             2. **Substantial Expansion**: Do not just add 'filler' text. Add new sections, technical explanations, real-world examples, and supporting evidence.\n\
             3. **Maintain Flow**: Ensure the new content integrates seamlessly with the existing structure.\n\
             4. **Professional Tone**: Keep the tone professional, objective, and clear.\n\
             5. **Markdown Format**: Use standard Markdown (headings, lists, bold text) for better readability.\n\n\
             Please begin the expansion process now using your tools."
        )
    }

    pub fn refinement_task(goal: &str, doc: &str, feedback: &AgentFeedback) -> String {
        format!(
            "Role: You are a Senior Technical Writer improving a document based on peer review.\n\
             Goal: {goal}\n\n\
             Current Document Content:\n\
             ---\n\
             {doc}\n\
             ---\n\n\
             Reviewer Feedback (Score: {}/100):\n\
             - Suggestions: {:?}\n\
             - Critical Errors: {:?}\n\n\
             Task: Address all feedback and errors while further expanding the document's depth.\n\n\
             Instructions:\n\
             1. Specifically target the 'Suggestions' and 'Critical Errors' mentioned by the reviewer.\n\
             2. Use `web_search` if additional information is needed to fix accuracy issues.\n\
             3. Ensure the final document is cohesive and hits the goal perfectly.",
            feedback.score, feedback.suggestions, feedback.critical_errors
        )
    }
}

pub struct ReviewerTemplates;

impl ReviewerTemplates {
    pub fn evaluation_task(goal: &str, doc: &str) -> String {
        format!(
            "Role: You are a Critical Editor and Quality Assurance Specialist.\n\
             Goal: {goal}\n\n\
             Document to Evaluate:\n\
             ---\n\
             {doc}\n\
             ---\n\n\
             Evaluation Criteria:\n\
             1. **Depth (40%)**: Has the writer provided substantial, deep information? Is it too surface-level?\n\
             2. **Relevance (30%)**: Does the new content directly serve the expansion goal?\n\
             3. **Factuality (20%)**: Does the information seem accurate and well-supported?\n\
             4. **Structure (10%)**: Is the document logical, well-formatted, and professional?\n\n\
             Output Format: You MUST return a JSON object with these fields:\n\
             - score: integer (0-100)\n\
             - passed: boolean (true if the document is high quality and meets the goal)\n\
             - suggestions: array of strings (constructive feedback for improvement)\n\
             - critical_errors: array of strings (factual errors, formatting breaks, or goal misses)\n\n\
             Example: {{ \"score\": 85, \"passed\": true, \"suggestions\": [\"Add more examples\"], \"critical_errors\": [] }}"
        )
    }
}
