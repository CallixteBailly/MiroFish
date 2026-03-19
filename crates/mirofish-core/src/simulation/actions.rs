//! Action types for Twitter and Reddit simulation platforms.

use serde::{Deserialize, Serialize};

/// Actions available on the Twitter platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TwitterAction {
    CreatePost,
    LikePost,
    Repost,
    QuotePost,
    Follow,
    DoNothing,
}

impl TwitterAction {
    /// All available Twitter actions.
    pub fn all() -> Vec<Self> {
        vec![
            Self::CreatePost,
            Self::LikePost,
            Self::Repost,
            Self::Follow,
            Self::DoNothing,
            Self::QuotePost,
        ]
    }

    /// Display name for the action.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CreatePost => "CREATE_POST",
            Self::LikePost => "LIKE_POST",
            Self::Repost => "REPOST",
            Self::QuotePost => "QUOTE_POST",
            Self::Follow => "FOLLOW",
            Self::DoNothing => "DO_NOTHING",
        }
    }

    /// Parse from string.
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_uppercase().trim() {
            "CREATE_POST" | "CREATEPOST" => Some(Self::CreatePost),
            "LIKE_POST" | "LIKEPOST" => Some(Self::LikePost),
            "REPOST" => Some(Self::Repost),
            "QUOTE_POST" | "QUOTEPOST" => Some(Self::QuotePost),
            "FOLLOW" => Some(Self::Follow),
            "DO_NOTHING" | "DONOTHING" => Some(Self::DoNothing),
            _ => None,
        }
    }
}

/// Actions available on the Reddit platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RedditAction {
    CreatePost,
    CreateComment,
    LikePost,
    DislikePost,
    LikeComment,
    DislikeComment,
    SearchPosts,
    SearchUser,
    Trend,
    Refresh,
    Follow,
    Mute,
    DoNothing,
}

impl RedditAction {
    /// All available Reddit actions.
    pub fn all() -> Vec<Self> {
        vec![
            Self::LikePost,
            Self::DislikePost,
            Self::CreatePost,
            Self::CreateComment,
            Self::LikeComment,
            Self::DislikeComment,
            Self::SearchPosts,
            Self::SearchUser,
            Self::Trend,
            Self::Refresh,
            Self::DoNothing,
            Self::Follow,
            Self::Mute,
        ]
    }

    /// Display name for the action.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CreatePost => "CREATE_POST",
            Self::CreateComment => "CREATE_COMMENT",
            Self::LikePost => "LIKE_POST",
            Self::DislikePost => "DISLIKE_POST",
            Self::LikeComment => "LIKE_COMMENT",
            Self::DislikeComment => "DISLIKE_COMMENT",
            Self::SearchPosts => "SEARCH_POSTS",
            Self::SearchUser => "SEARCH_USER",
            Self::Trend => "TREND",
            Self::Refresh => "REFRESH",
            Self::Follow => "FOLLOW",
            Self::Mute => "MUTE",
            Self::DoNothing => "DO_NOTHING",
        }
    }

    /// Parse from string.
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_uppercase().trim() {
            "CREATE_POST" | "CREATEPOST" => Some(Self::CreatePost),
            "CREATE_COMMENT" | "CREATECOMMENT" => Some(Self::CreateComment),
            "LIKE_POST" | "LIKEPOST" => Some(Self::LikePost),
            "DISLIKE_POST" | "DISLIKEPOST" => Some(Self::DislikePost),
            "LIKE_COMMENT" | "LIKECOMMENT" => Some(Self::LikeComment),
            "DISLIKE_COMMENT" | "DISLIKECOMMENT" => Some(Self::DislikeComment),
            "SEARCH_POSTS" | "SEARCHPOSTS" => Some(Self::SearchPosts),
            "SEARCH_USER" | "SEARCHUSER" => Some(Self::SearchUser),
            "TREND" => Some(Self::Trend),
            "REFRESH" => Some(Self::Refresh),
            "FOLLOW" => Some(Self::Follow),
            "MUTE" => Some(Self::Mute),
            "DO_NOTHING" | "DONOTHING" => Some(Self::DoNothing),
            _ => None,
        }
    }
}

/// A generic action result that records what an agent did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub round_num: u64,
    pub timestamp: String,
    pub platform: String,
    pub agent_id: u64,
    pub agent_name: String,
    pub action_type: String,
    #[serde(default)]
    pub action_args: serde_json::Value,
    pub result: Option<String>,
    pub success: bool,
}
