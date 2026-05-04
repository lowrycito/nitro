//! Built-in provider presets. Mirrors `src/logic/defaultProviders.ts`.
//!
//! The model-list fetch is part of Phase 3 (LLM clients); this module only
//! exposes the static catalogue for now.

use super::provider::ApiType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultProvider {
    pub name: &'static str,
    pub base_url: &'static str,
    pub api_type: ApiType,
}

pub const DEFAULT_PROVIDERS: &[DefaultProvider] = &[
    DefaultProvider {
        name: "openai",
        base_url: "https://api.openai.com/v1",
        api_type: ApiType::OpenAiResponses,
    },
    DefaultProvider {
        name: "anthropic",
        base_url: "https://api.anthropic.com/v1",
        api_type: ApiType::Anthropic,
    },
    DefaultProvider {
        name: "zai-coding-plan",
        base_url: "https://api.z.ai/api/anthropic/v1",
        api_type: ApiType::Anthropic,
    },
    DefaultProvider {
        name: "zai-api",
        base_url: "https://api.z.ai/api/paas/v4",
        api_type: ApiType::OpenAiCompatible,
    },
    DefaultProvider {
        name: "qwen-us",
        base_url: "https://dashscope-us.aliyuncs.com/compatible-mode/v1",
        api_type: ApiType::OpenAiCompatible,
    },
    DefaultProvider {
        name: "deepseek",
        base_url: "https://api.deepseek.com/v1",
        api_type: ApiType::OpenAiCompatible,
    },
    DefaultProvider {
        name: "mistral",
        base_url: "https://api.mistral.ai/v1",
        api_type: ApiType::OpenAiCompatible,
    },
    DefaultProvider {
        name: "groq",
        base_url: "https://api.groq.com/openai/v1",
        api_type: ApiType::OpenAiCompatible,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_matches_ts() {
        // The TS file declares exactly 8 presets. Keep this assertion as a
        // tripwire: changing the catalogue requires updating both sides.
        assert_eq!(DEFAULT_PROVIDERS.len(), 8);
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = DEFAULT_PROVIDERS.iter().map(|p| p.name).collect();
        names.sort_unstable();
        let len_before = names.len();
        names.dedup();
        assert_eq!(names.len(), len_before);
    }
}
