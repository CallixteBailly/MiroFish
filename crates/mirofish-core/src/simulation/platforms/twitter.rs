//! Twitter/X platform simulation model.
//!
//! Maintains in-memory social media state: posts, users, followers, likes.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A tweet / post.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tweet {
    pub post_id: u64,
    pub author_id: u64,
    pub author_name: String,
    pub content: String,
    pub created_at: String,
    pub likes: HashSet<u64>,
    pub reposts: HashSet<u64>,
    pub quote_posts: Vec<u64>,
    pub is_repost: bool,
    pub original_post_id: Option<u64>,
    pub quote_content: Option<String>,
}

/// A Twitter user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwitterUser {
    pub user_id: u64,
    pub username: String,
    pub name: String,
    pub bio: String,
    pub followers: HashSet<u64>,
    pub following: HashSet<u64>,
    pub post_ids: Vec<u64>,
}

/// In-memory Twitter platform state.
pub struct TwitterPlatform {
    pub users: HashMap<u64, TwitterUser>,
    pub posts: HashMap<u64, Tweet>,
    next_post_id: u64,
}

impl TwitterPlatform {
    /// Create a new empty platform.
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
            posts: HashMap::new(),
            next_post_id: 1,
        }
    }

    /// Register a user on the platform.
    pub fn add_user(&mut self, user_id: u64, username: &str, name: &str, bio: &str) {
        self.users.insert(user_id, TwitterUser {
            user_id,
            username: username.to_string(),
            name: name.to_string(),
            bio: bio.to_string(),
            followers: HashSet::new(),
            following: HashSet::new(),
            post_ids: Vec::new(),
        });
    }

    /// Create a new post.
    pub fn create_post(&mut self, author_id: u64, content: &str) -> Option<u64> {
        let author_name = self.users.get(&author_id)?.name.clone();
        let post_id = self.next_post_id;
        self.next_post_id += 1;

        let tweet = Tweet {
            post_id,
            author_id,
            author_name,
            content: content.to_string(),
            created_at: Utc::now().to_rfc3339(),
            likes: HashSet::new(),
            reposts: HashSet::new(),
            quote_posts: Vec::new(),
            is_repost: false,
            original_post_id: None,
            quote_content: None,
        };

        self.posts.insert(post_id, tweet);
        if let Some(user) = self.users.get_mut(&author_id) {
            user.post_ids.push(post_id);
        }

        Some(post_id)
    }

    /// Like a post.
    pub fn like_post(&mut self, user_id: u64, post_id: u64) -> bool {
        if let Some(post) = self.posts.get_mut(&post_id) {
            post.likes.insert(user_id);
            true
        } else {
            false
        }
    }

    /// Repost a post.
    pub fn repost(&mut self, user_id: u64, post_id: u64) -> Option<u64> {
        let original = self.posts.get(&post_id)?;
        let author_name = self.users.get(&user_id)?.name.clone();
        let original_content = original.content.clone();

        // Record on original
        if let Some(p) = self.posts.get_mut(&post_id) {
            p.reposts.insert(user_id);
        }

        let new_id = self.next_post_id;
        self.next_post_id += 1;

        let tweet = Tweet {
            post_id: new_id,
            author_id: user_id,
            author_name,
            content: original_content,
            created_at: Utc::now().to_rfc3339(),
            likes: HashSet::new(),
            reposts: HashSet::new(),
            quote_posts: Vec::new(),
            is_repost: true,
            original_post_id: Some(post_id),
            quote_content: None,
        };

        self.posts.insert(new_id, tweet);
        if let Some(user) = self.users.get_mut(&user_id) {
            user.post_ids.push(new_id);
        }

        Some(new_id)
    }

    /// Quote-post (repost with additional comment).
    pub fn quote_post(&mut self, user_id: u64, post_id: u64, quote: &str) -> Option<u64> {
        let original = self.posts.get(&post_id)?;
        let author_name = self.users.get(&user_id)?.name.clone();
        let original_content = original.content.clone();

        if let Some(p) = self.posts.get_mut(&post_id) {
            p.quote_posts.push(self.next_post_id);
        }

        let new_id = self.next_post_id;
        self.next_post_id += 1;

        let tweet = Tweet {
            post_id: new_id,
            author_id: user_id,
            author_name,
            content: original_content,
            created_at: Utc::now().to_rfc3339(),
            likes: HashSet::new(),
            reposts: HashSet::new(),
            quote_posts: Vec::new(),
            is_repost: false,
            original_post_id: Some(post_id),
            quote_content: Some(quote.to_string()),
        };

        self.posts.insert(new_id, tweet);
        if let Some(user) = self.users.get_mut(&user_id) {
            user.post_ids.push(new_id);
        }

        Some(new_id)
    }

    /// Follow another user.
    pub fn follow(&mut self, follower_id: u64, target_id: u64) -> bool {
        if follower_id == target_id {
            return false;
        }
        if !self.users.contains_key(&follower_id) || !self.users.contains_key(&target_id) {
            return false;
        }
        self.users.get_mut(&follower_id).map(|u| u.following.insert(target_id));
        self.users.get_mut(&target_id).map(|u| u.followers.insert(follower_id));
        true
    }

    /// Generate a feed for a user: recent posts from followed users + trending.
    pub fn generate_feed(&self, user_id: u64, max_items: usize) -> Vec<&Tweet> {
        let following = match self.users.get(&user_id) {
            Some(u) => &u.following,
            None => return Vec::new(),
        };

        let mut feed: Vec<&Tweet> = self
            .posts
            .values()
            .filter(|p| following.contains(&p.author_id) || p.author_id == user_id)
            .collect();

        // Sort by recency
        feed.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Add trending posts (high like count) that the user hasn't seen
        let mut trending: Vec<&Tweet> = self
            .posts
            .values()
            .filter(|p| !following.contains(&p.author_id) && p.author_id != user_id)
            .collect();
        trending.sort_by(|a, b| b.likes.len().cmp(&a.likes.len()));

        feed.extend(trending.into_iter().take(max_items / 3));
        feed.truncate(max_items);
        feed
    }

    /// Format the feed as a text description for LLM prompt.
    pub fn format_feed_for_prompt(&self, user_id: u64, max_items: usize) -> String {
        let feed = self.generate_feed(user_id, max_items);
        if feed.is_empty() {
            return "No posts in your timeline yet.".to_string();
        }

        let mut lines = Vec::new();
        for (i, tweet) in feed.iter().enumerate() {
            let likes = tweet.likes.len();
            let reposts = tweet.reposts.len();
            let mut line = format!(
                "{}. [Post #{}] @{}: {}\n   Likes: {}, Reposts: {}",
                i + 1,
                tweet.post_id,
                tweet.author_name,
                tweet.content,
                likes,
                reposts,
            );
            if let Some(ref quote) = tweet.quote_content {
                line.push_str(&format!("\n   Quote: \"{}\"", quote));
            }
            if tweet.is_repost {
                line.push_str(&format!(" [Repost of #{}]", tweet.original_post_id.unwrap_or(0)));
            }
            lines.push(line);
        }

        lines.join("\n\n")
    }

    /// Get total post count.
    pub fn post_count(&self) -> usize {
        self.posts.len()
    }

    /// Get total user count.
    pub fn user_count(&self) -> usize {
        self.users.len()
    }
}

impl Default for TwitterPlatform {
    fn default() -> Self {
        Self::new()
    }
}
