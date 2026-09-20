//! Provider presets (specs/phase-1/spec.md 1.2): one client, many presets.
//! A preset only changes base URL + default model — never the HTTP stack.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum PresetId {
    Glm,
    Minimax,
    Openai,
    Anymodel,
    Custom,
}

impl PresetId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Glm => "glm",
            Self::Minimax => "minimax",
            Self::Openai => "openai",
            Self::Anymodel => "anymodel",
            Self::Custom => "custom",
        }
    }

    pub fn all() -> [PresetId; 5] {
        [Self::Glm, Self::Minimax, Self::Openai, Self::Anymodel, Self::Custom]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub id: PresetId,
    pub display_name: &'static str,
    pub base_url: &'static str,
    pub default_model: &'static str,
}

pub fn preset(id: PresetId) -> Preset {
    match id {
        PresetId::Glm => Preset {
            id,
            display_name: "GLM (Z.AI Coding)",
            base_url: "https://api.z.ai/api/coding/paas/v4",
            default_model: "glm-5.3",
        },
        PresetId::Minimax => Preset {
            id,
            display_name: "MiniMax",
            base_url: "https://api.minimax.io/v1",
            default_model: "MiniMax-M3",
        },
        PresetId::Openai => Preset {
            id,
            display_name: "OpenAI",
            base_url: "https://api.openai.com/v1",
            default_model: "gpt-4o-mini",
        },
        PresetId::Anymodel => Preset {
            id,
            display_name: "AnyModel",
            base_url: "https://anymodel.org/v1",
            default_model: "gpt-5.6-sol",
        },
        PresetId::Custom => Preset {
            id,
            display_name: "Custom",
            base_url: "",
            default_model: "",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_presets_have_ids() {
        for id in PresetId::all() {
            assert!(!id.as_str().is_empty());
        }
        assert_eq!(PresetId::Anymodel.as_str(), "anymodel");
    }
}
