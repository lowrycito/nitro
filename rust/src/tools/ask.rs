//! AskUser tool. Mirrors `src/tools/ask.tsx`.
//!
//! No execution — the tool is a structured prompt to the user. The UI (Phase
//! 5+) collects answers; `AskTool::execute` simply records them.

use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct QuestionChoice {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Question {
    pub title: String,
    pub question: String,
    pub choices: Vec<QuestionChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct QuestionResponse {
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AskModelInput {
    pub questions: Vec<Question>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AskToolOutput {
    pub answers: Vec<QuestionResponse>,
}

#[derive(Debug, Default)]
pub struct AskTool;

impl AskTool {
    pub const NAME: &'static str = "AskUser";

    pub fn description() -> &'static str {
        ASK_TOOL_DESCRIPTION
    }

    pub fn input_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["questions"],
            "additionalProperties": false,
            "properties": {
                "questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["title", "question", "choices"],
                        "additionalProperties": false,
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Short title describing the question (5 words max)"
                            },
                            "question": {
                                "type": "string",
                                "description": "Question for the user to answer. Limit to 1 to 3 sentences."
                            },
                            "choices": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "required": ["label"],
                                    "additionalProperties": false,
                                    "properties": {
                                        "label": {
                                            "type": "string",
                                            "description": "Short label describing the choice (5 words max)"
                                        },
                                        "description": {
                                            "type": "string",
                                            "description": "1 to 2 sentence description of the choice. Provide this if the label is not self-explanatory."
                                        }
                                    }
                                },
                                "description": "Choices the user can select to answer the question. Limit choices to 2-4. Provide only the most common choices; if none are adequate, the user can type their own answer. The user may select only one choice per question."
                            }
                        }
                    }
                }
            }
        })
    }

    /// Pass-through: the UI collects answers and calls this to confirm the
    /// shape; the result is what the model sees on the next turn.
    pub fn execute(answers: Vec<QuestionResponse>) -> AskToolOutput {
        AskToolOutput { answers }
    }
}

const ASK_TOOL_DESCRIPTION: &str = "Ask the user one or more questions. Each question should come with predetermined choices for users to choose from. If no choices are adequate, users can choose to type their own answer.
Use this tool to clarify ambiguous requests or gather user decisions.

Tool Usage Guidelines:
- Use the tool only if:
  - The user's intention is unclear and cannot be determined with 80% accuracy; or
  - The user's request is dangerous and may result in unintended consequences.
- Do not use the tool if the user's intention is clear and the request is reasonably safe or reversible.
- **Do not manually add a \"Type your own answer\" option. This option is automatically provided by the UI.**

Example Usage:
<example>
Context:
- The user wants to remove all \"old files\" inside a projects folder to reclaim disk space
- The definition of \"old\" is ambiguous. What is old? 3 months? 1 year?
- In addition, you notice that most of the disk space is occupied by node_modules folders; may be better to only delete node_modules

Agent Action: Ask two questions:

\"What is the specific timeframe for 'old'?\"
Choices:
- 3 months
- 6 months
- 1 year

\"Should I remove entire projects or only the node_modules folders? The node_modules folders are taking up a majority of the space.\"
Choices:
- Remove entire projects
- Remove only node_modules folders
</example>";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn questions_round_trip() {
        let raw = json!({
            "questions": [
                {
                    "title": "Define old",
                    "question": "What is old?",
                    "choices": [
                        { "label": "3 months" },
                        { "label": "1 year", "description": "More than a year." }
                    ]
                }
            ]
        });
        let m: AskModelInput = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(m.questions.len(), 1);
        let back = serde_json::to_value(&m).unwrap();
        // Round-trip through serde without losing fields.
        assert_eq!(back["questions"][0]["choices"][0]["label"], "3 months");
        assert_eq!(
            back["questions"][0]["choices"][1]["description"],
            "More than a year."
        );
    }

    #[test]
    fn execute_passes_answers_through() {
        let out = AskTool::execute(vec![QuestionResponse {
            question: "Q?".into(),
            answer: "A".into(),
        }]);
        assert_eq!(out.answers.len(), 1);
        assert_eq!(out.answers[0].question, "Q?");
    }

    #[test]
    fn schema_requires_questions() {
        let s = AskTool::input_schema();
        let req = s["required"].as_array().unwrap();
        let strs: Vec<&str> = req.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(strs, vec!["questions"]);
    }
}
