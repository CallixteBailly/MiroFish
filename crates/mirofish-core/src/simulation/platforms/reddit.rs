//! Reddit platform simulation model.
//!
//! Subreddits, posts, comments, and votes.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A Reddit post.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedditPost {
    pub post_id: u64,
    pub subreddit: String,
    pub author_id: u64,
    pub author_name: String,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub upvotes: HashSet<u64>,
    pub downvotes: HashSet<u64>,
    pub comment_ids: Vec<u64>,
}

impl RedditPost {
    /// Net vote score.
    pub fn score(&self) -> i64 {
        self.upvotes.len() as i64 - self.downvotes.len() as i64
    }
}

/// A Reddit comment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedditComment {
    pub comment_id: u64,
    pub post_id: u64,
    pub author_id: u64,
    pub author_name: String,
    pub content: String,
    pub created_at: String,
    pub upvotes: HashSet<u64>,
    pub downvotes: HashSet<u64>,
    pub parent_comment_id: Option<u64>,
}

impl RedditComment {
    pub fn score(&self) -> i64 {
        self.upvotes.len() as i64 - self.downvotes.len() as i64
    }
}

/// A Reddit user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedditUser {
    pub user_id: u64,
    pub username: String,
    pub name: String,
    pub bio: String,
    pub karma: i64,
    pub following: HashSet<u64>,
    pub muted: HashSet<u64>,
    pub post_ids: Vec<u64>,
}

/// In-memory Reddit platform state.
pub struct RedditPlatform {
    pub users: HashMap<u64, RedditUser>,
    pub posts: HashMap<u64, RedditPost>,
    pub comments: HashMap<u64, RedditComment>,
    pub subreddits: HashSet<String>,
    next_post_id: u64,
    next_comment_id: u64,
}

impl RedditPlatform {
    /// Create a new empty platform.
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
            posts: HashMap::new(),
            comments: HashMap::new(),
            subreddits: HashSet::new(),
            next_post_id: 1,
            next_comment_id: 1,
        }
    }

    /// Register a user.
    pub fn add_user(&mut self, user_id: u64, username: &str, name: &str, bio: &str, karma: i64) {
        self.users.insert(user_id, RedditUser {
            user_id,
            username: username.to_string(),
            name: name.to_string(),
            bio: bio.to_string(),
            karma,
            following: HashSet::new(),
            muted: HashSet::new(),
            post_ids: Vec::new(),
        });
    }

    /// Create a subreddit if it doesn't exist.
    pub fn ensure_subreddit(&mut self, name: &str) {
        self.subreddits.insert(name.to_string());
    }

    /// Create a post.
    pub fn create_post(&mut self, author_id: u64, subreddit: &str, title: &str, content: &str) -> Option<u64> {
        let author_name = self.users.get(&author_id)?.name.clone();
        self.ensure_subreddit(subreddit);

        let post_id = self.next_post_id;
        self.next_post_id += 1;

        let post = RedditPost {
            post_id,
            subreddit: subreddit.to_string(),
            author_id,
            author_name,
            title: title.to_string(),
            content: content.to_string(),
            created_at: Utc::now().to_rfc3339(),
            upvotes: HashSet::new(),
            downvotes: HashSet::new(),
            comment_ids: Vec::new(),
        };

        self.posts.insert(post_id, post);
        if let Some(user) = self.users.get_mut(&author_id) {
            user.post_ids.push(post_id);
            user.karma += 1;
        }

        Some(post_id)
    }

    /// Create a comment on a post.
    pub fn create_comment(
        &mut self,
        author_id: u64,
        post_id: u64,
        content: &str,
        parent_comment_id: Option<u64>,
    ) -> Option<u64> {
        if !self.posts.contains_key(&post_id) {
            return None;
        }
        let author_name = self.users.get(&author_id)?.name.clone();

        let comment_id = self.next_comment_id;
        self.next_comment_id += 1;

        let comment = RedditComment {
            comment_id,
            post_id,
            author_id,
            author_name,
            content: content.to_string(),
            created_at: Utc::now().to_rfc3339(),
            upvotes: HashSet::new(),
            downvotes: HashSet::new(),
            parent_comment_id,
        };

        self.comments.insert(comment_id, comment);
        if let Some(post) = self.posts.get_mut(&post_id) {
            post.comment_ids.push(comment_id);
        }
        if let Some(user) = self.users.get_mut(&author_id) {
            user.karma += 1;
        }

        Some(comment_id)
    }

    /// Upvote a post.
    pub fn like_post(&mut self, user_id: u64, post_id: u64) -> bool {
        if let Some(post) = self.posts.get_mut(&post_id) {
            post.downvotes.remove(&user_id);
            post.upvotes.insert(user_id);
            if let Some(author) = self.users.get_mut(&post.author_id) {
                author.karma += 1;
            }
            true
        } else {
            false
        }
    }

    /// Downvote a post.
    pub fn dislike_post(&mut self, user_id: u64, post_id: u64) -> bool {
        if let Some(post) = self.posts.get_mut(&post_id) {
            post.upvotes.remove(&user_id);
            post.downvotes.insert(user_id);
            if let Some(author) = self.users.get_mut(&post.author_id) {
                author.karma -= 1;
            }
            true
        } else {
            false
        }
    }

    /// Upvote a comment.
    pub fn like_comment(&mut self, user_id: u64, comment_id: u64) -> bool {
        if let Some(comment) = self.comments.get_mut(&comment_id) {
            comment.downvotes.remove(&user_id);
            comment.upvotes.insert(user_id);
            true
        } else {
            false
        }
    }

    /// Downvote a comment.
    pub fn dislike_comment(&mut self, user_id: u64, comment_id: u64) -> bool {
        if let Some(comment) = self.comments.get_mut(&comment_id) {
            comment.upvotes.remove(&user_id);
            comment.downvotes.insert(user_id);
            true
        } else {
            false
        }
    }

    /// Follow a user.
    pub fn follow(&mut self, follower_id: u64, target_id: u64) -> bool {
        if follower_id == target_id || !self.users.contains_key(&target_id) {
            return false;
        }
        if let Some(user) = self.users.get_mut(&follower_id) {
            user.following.insert(target_id);
            true
        } else {
            false
        }
    }

    /// Mute a user.
    pub fn mute(&mut self, user_id: u64, target_id: u64) -> bool {
        if let Some(user) = self.users.get_mut(&user_id) {
            user.muted.insert(target_id);
            true
        } else {
            false
        }
    }

    /// Search posts by keyword.
    pub fn search_posts(&self, query: &str, limit: usize) -> Vec<&RedditPost> {
        let q = query.to_lowercase();
        let mut results: Vec<&RedditPost> = self
            .posts
            .values()
            .filter(|p| {
                p.title.to_lowercase().contains(&q)
                    || p.content.to_lowercase().contains(&q)
            })
            .collect();
        results.sort_by(|a, b| b.score().cmp(&a.score()));
        results.truncate(limit);
        results
    }

    /// Get trending posts (highest score).
    pub fn trending(&self, limit: usize) -> Vec<&RedditPost> {
        let mut posts: Vec<&RedditPost> = self.posts.values().collect();
        posts.sort_by(|a, b| b.score().cmp(&a.score()));
        posts.truncate(limit);
        posts
    }

    /// Generate a feed for a user.
    pub fn generate_feed(&self, user_id: u64, max_items: usize) -> Vec<&RedditPost> {
        let muted = self
            .users
            .get(&user_id)
            .map(|u| &u.muted)
            .cloned()
            .unwrap_or_default();

        let mut feed: Vec<&RedditPost> = self
            .posts
            .values()
            .filter(|p| !muted.contains(&p.author_id))
            .collect();

        // Sort by score then recency
        feed.sort_by(|a, b| {
            b.score().cmp(&a.score()).then(b.created_at.cmp(&a.created_at))
        });
        feed.truncate(max_items);
        feed
    }

    /// Format the feed as text for LLM prompt.
    pub fn format_feed_for_prompt(&self, user_id: u64, max_items: usize) -> String {
        let feed = self.generate_feed(user_id, max_items);
        if feed.is_empty() {
            return "No posts available.".to_string();
        }

        let mut lines = Vec::new();
        for (i, post) in feed.iter().enumerate() {
            let comments_count = post.comment_ids.len();
            let mut line = format!(
                "{}. [Post #{} in r/{}] @{}: {}\n   {}\n   Score: {} (up: {}, down: {}), Comments: {}",
                i + 1,
                post.post_id,
                post.subreddit,
                post.author_name,
                post.title,
                post.content,
                post.score(),
                post.upvotes.len(),
                post.downvotes.len(),
                comments_count,
            );

            // Show top comments
            let comment_ids = &post.comment_ids;
            let mut shown = 0;
            for cid in comment_ids.iter().rev().take(3) {
                if let Some(comment) = self.comments.get(cid) {
                    line.push_str(&format!(
                        "\n     > @{}: {} (score: {})",
                        comment.author_name, comment.content, comment.score()
                    ));
                    shown += 1;
                }
            }
            if comments_count > shown {
                line.push_str(&format!("\n     ... and {} more comments", comments_count - shown));
            }

            lines.push(line);
        }

        lines.join("\n\n")
    }

    /// Get total post count.
    pub fn post_count(&self) -> usize {
        self.posts.len()
    }
}

impl Default for RedditPlatform {
    fn default() -> Self {
        Self::new()
    }
}
