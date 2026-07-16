//! Comment threading and review status. [FR-REV, SDS §2.2.9]
//!
//! Supports threaded comments with review status (accepted/rejected/completed),
//! filtering by author/type/status/page, and navigation to commented locations.
//! [FR-REV-1, FR-REV-2, FR-REV-3, FR-REV-4]

use crate::annotation::{Annotation, AnnotationStore, ReviewStatus};

/// A comment thread: a top-level annotation with its replies.
#[derive(Debug, Clone)]
pub struct CommentThread {
    /// The top-level comment annotation.
    pub root: Annotation,
    /// Reply annotations in chronological order.
    pub replies: Vec<Annotation>,
}

impl CommentThread {
    /// Create a new thread from a root annotation.
    pub fn new(root: Annotation) -> Self {
        Self {
            root,
            replies: Vec::new(),
        }
    }

    /// Add a reply to this thread.
    pub fn add_reply(&mut self, reply: Annotation) {
        self.replies.push(reply);
    }

    /// Total comment count (root + replies).
    pub fn count(&self) -> usize {
        1 + self.replies.len()
    }

    /// All annotations in the thread (root first, then replies).
    pub fn all(&self) -> Vec<&Annotation> {
        let mut result = vec![&self.root];
        result.extend(self.replies.iter());
        result
    }

    /// The most recent comment in the thread.
    pub fn latest(&self) -> &Annotation {
        self.replies.last().unwrap_or(&self.root)
    }

    /// The root's author.
    pub fn author(&self) -> &str {
        &self.root.properties.author
    }

    /// The root's contents.
    pub fn contents(&self) -> &str {
        &self.root.properties.contents
    }

    /// Thread age (root creation time).
    pub fn created_at(&self) -> u64 {
        self.root.properties.creation_time
    }
}

/// Filter options for comments/reviews. [FR-REV-2]
#[derive(Debug, Clone, Default)]
pub struct CommentFilter {
    /// Filter by author name (case-insensitive substring).
    pub author: Option<String>,
    /// Filter by review status.
    pub status: Option<ReviewStatus>,
    /// Filter by page index.
    pub page: Option<u32>,
}

/// Review manager: organizes annotations into threads and supports
/// filtering and export. [FR-REV]
pub struct ReviewManager {
    /// Comment threads indexed by root annotation ID.
    threads: std::collections::HashMap<u64, CommentThread>,
}

impl ReviewManager {
    pub fn new() -> Self {
        Self {
            threads: std::collections::HashMap::new(),
        }
    }

    /// Build threads from an annotation store.
    ///
    /// Scans all annotations and groups them into threads based on parent_id.
    pub fn build_threads(&mut self, store: &AnnotationStore) {
        self.threads.clear();

        // First pass: collect all annotations as potential roots.
        let all_annots: Vec<&Annotation> = store.all_annotations();

        // Second pass: organize into threads.
        for &ann in &all_annots {
            if ann.parent_id == 0 {
                // This is a root comment.
                self.threads.entry(ann.id)
                    .or_insert_with(|| CommentThread::new(ann.clone()));
            }
        }

        // Third pass: add replies to threads.
        for &ann in &all_annots {
            if ann.parent_id != 0 {
                if let Some(thread) = self.threads.get_mut(&ann.parent_id) {
                    thread.add_reply(ann.clone());
                }
            }
        }
    }

    /// Get all threads.
    pub fn threads(&self) -> Vec<&CommentThread> {
        self.threads.values().collect()
    }

    /// Get a thread by root annotation ID.
    pub fn thread(&self, root_id: u64) -> Option<&CommentThread> {
        self.threads.get(&root_id)
    }

    /// Filter threads by the given criteria.
    pub fn filter(&self, filter: &CommentFilter) -> Vec<&CommentThread> {
        self.threads.values()
            .filter(|t| {
                if let Some(ref author) = filter.author {
                    if !t.author().to_lowercase().contains(&author.to_lowercase()) {
                        return false;
                    }
                }
                if let Some(status) = filter.status {
                    if t.root.review_status != status {
                        return false;
                    }
                }
                if let Some(page) = filter.page {
                    if t.root.page_index != page {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Total thread count.
    pub fn thread_count(&self) -> usize {
        self.threads.len()
    }

    /// Total comment count across all threads.
    pub fn total_comments(&self) -> usize {
        self.threads.values().map(|t| t.count()).sum()
    }

    /// Export comment summary as text. [FR-REV-3]
    pub fn export_summary(&self) -> String {
        let mut output = String::new();
        for (i, thread) in self.threads.values().enumerate() {
            output.push_str(&format!("--- Comment {} ---\n", i + 1));
            output.push_str(&format!("Author: {}\n", thread.author()));
            output.push_str(&format!("Page: {}\n", thread.root.page_index + 1));
            output.push_str(&format!("Content: {}\n", thread.contents()));
            output.push_str(&format!("Status: {:?}\n", thread.root.review_status));
            output.push_str(&format!("Replies: {}\n\n", thread.replies.len()));

            for (j, reply) in thread.replies.iter().enumerate() {
                output.push_str(&format!("  Reply {} ({}):\n", j + 1, reply.properties.author));
                output.push_str(&format!("  {}\n\n", reply.properties.contents));
            }
        }
        output
    }
}

impl Default for ReviewManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{AnnotationType, Rect};

    fn make_annotation(id: u64, author: &str, contents: &str, page: u32) -> Annotation {
        Annotation::new(id, page, AnnotationType::StickyNote, Rect::new(0.0, 0.0, 20.0, 20.0))
            .with_author(author)
            .with_contents(contents)
    }

    #[test]
    fn thread_construction() {
        let mut root = make_annotation(1, "Alice", "Original comment", 0);
        root.replies = vec![2, 3];

        let reply1 = make_annotation(2, "Bob", "Reply 1", 0);
        let reply2 = make_annotation(3, "Alice", "Reply 2", 0);

        let mut thread = CommentThread::new(root);
        thread.add_reply(reply1);
        thread.add_reply(reply2);

        assert_eq!(thread.count(), 3);
        assert_eq!(thread.author(), "Alice");
        assert_eq!(thread.latest().properties.contents, "Reply 2");
    }

    #[test]
    fn review_manager_build_threads() {
        let mut store = AnnotationStore::new();

        let mut root = make_annotation(1, "Alice", "Top comment", 0);
        root.replies = vec![2];
        store.page_mut(0).add(root);

        let mut reply = make_annotation(2, "Bob", "Reply", 0);
        reply.parent_id = 1;
        store.page_mut(0).add(reply);

        let mut manager = ReviewManager::new();
        manager.build_threads(&store);

        assert_eq!(manager.thread_count(), 1);
        assert_eq!(manager.total_comments(), 2);

        let thread = manager.thread(1).unwrap();
        assert_eq!(thread.replies.len(), 1);
    }

    #[test]
    fn review_manager_filter() {
        let mut store = AnnotationStore::new();
        let mut root1 = make_annotation(1, "Alice", "Comment on page 0", 0);
        root1.replies = vec![];
        store.page_mut(0).add(root1);

        let mut root2 = make_annotation(2, "Bob", "Comment on page 1", 1);
        root2.review_status = ReviewStatus::Accepted;
        root2.replies = vec![];
        store.page_mut(1).add(root2);

        let mut manager = ReviewManager::new();
        manager.build_threads(&store);

        // Filter by author.
        let filter = CommentFilter { author: Some("Alice".into()), ..Default::default() };
        let results = manager.filter(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].author(), "Alice");

        // Filter by page.
        let filter = CommentFilter { page: Some(1), ..Default::default() };
        let results = manager.filter(&filter);
        assert_eq!(results.len(), 1);

        // Filter by status.
        let filter = CommentFilter { status: Some(ReviewStatus::Accepted), ..Default::default() };
        let results = manager.filter(&filter);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn export_summary() {
        let mut store = AnnotationStore::new();
        let root = make_annotation(1, "Alice", "Review this section", 0);
        store.page_mut(0).add(root);

        let mut manager = ReviewManager::new();
        manager.build_threads(&store);

        let summary = manager.export_summary();
        assert!(summary.contains("Alice"));
        assert!(summary.contains("Review this section"));
        assert!(summary.contains("Comment 1"));
    }
}
