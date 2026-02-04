//! Tests for the auto-eval framework.

use tempfile::TempDir;

use super::*;

fn create_test_store() -> (FeedbackStore, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let store = FeedbackStore::new(temp_dir.path().to_path_buf()).unwrap();
    (store, temp_dir)
}

#[test]
fn test_feedback_type_display() {
    assert_eq!(FeedbackType::ExplicitNegative.to_string(), "explicit_negative");
    assert_eq!(FeedbackType::ImplicitRetry.to_string(), "implicit_retry");
    assert_eq!(FeedbackType::Error.to_string(), "error");
    assert_eq!(FeedbackType::Timeout.to_string(), "timeout");
    assert_eq!(FeedbackType::Correction.to_string(), "correction");
    assert_eq!(FeedbackType::Positive.to_string(), "positive");
}

#[test]
fn test_feedback_creation() {
    let feedback = Feedback::new(
        "agent-1",
        FeedbackType::ExplicitNegative,
        "Testing feature",
        "That's wrong",
        "Here's the result",
    );

    assert_eq!(feedback.agent_id, "agent-1");
    assert_eq!(feedback.feedback_type, FeedbackType::ExplicitNegative);
    assert!(feedback.correction.is_none());

    let feedback = feedback.with_correction("Do it this way");
    assert_eq!(feedback.correction, Some("Do it this way".to_string()));
}

#[test]
fn test_feedback_detector_negative() {
    let detector = FeedbackDetector::new();

    // Should detect negative feedback
    assert_eq!(
        detector.detect("That's wrong", "Some output"),
        Some(FeedbackType::ExplicitNegative)
    );
    assert_eq!(
        detector.detect("No, that's not what I wanted", "Some output"),
        Some(FeedbackType::ExplicitNegative)
    );
    assert_eq!(
        detector.detect("This is broken and doesn't work", "Some output"),
        Some(FeedbackType::ExplicitNegative)
    );
    assert_eq!(
        detector.detect("Abort the operation", "Some output"),
        Some(FeedbackType::ExplicitNegative)
    );
}

#[test]
fn test_feedback_detector_positive() {
    let detector = FeedbackDetector::new();

    // Should detect positive feedback
    assert_eq!(
        detector.detect("Thanks, that's great!", "Some output"),
        Some(FeedbackType::Positive)
    );
    assert_eq!(
        detector.detect("Perfect, exactly what I needed", "Some output"),
        Some(FeedbackType::Positive)
    );
}

#[test]
fn test_feedback_detector_correction() {
    let detector = FeedbackDetector::new();

    // Should detect corrections
    assert_eq!(
        detector.detect("I meant the other file", "Some output"),
        Some(FeedbackType::Correction)
    );
    assert_eq!(
        detector.detect("Actually, use Python", "Some output"),
        Some(FeedbackType::Correction)
    );
    assert_eq!(
        detector.detect("It should be 'hello' not 'world'", "Some output"),
        Some(FeedbackType::Correction)
    );
}

#[test]
fn test_feedback_detector_false_positive() {
    let detector = FeedbackDetector::new();

    // "No problem" should not be detected as negative
    assert_ne!(
        detector.detect("No problem, thanks!", "Some output"),
        Some(FeedbackType::ExplicitNegative)
    );
}

#[test]
fn test_feedback_detector_no_signal() {
    let detector = FeedbackDetector::new();

    // Neutral messages should return None
    assert_eq!(detector.detect("Can you help me with this?", "Some output"), None);
    assert_eq!(detector.detect("Show me the code", "Some output"), None);
}

#[test]
fn test_retry_detection() {
    let detector = FeedbackDetector::new();

    // Exact retry
    assert!(detector.is_retry("Generate a report", "Generate a report"));

    // Similar retry (case insensitive)
    assert!(detector.is_retry("Generate a Report", "generate a report"));

    // Similar retry (with extra punctuation)
    assert!(detector.is_retry("Generate a report!", "Generate a report."));

    // Different requests
    assert!(!detector.is_retry("Generate a report", "Delete the file"));
    assert!(!detector.is_retry("Generate a report", "Show me the logs"));
}

#[tokio::test]
async fn test_feedback_store_add_and_get() {
    let (mut store, _dir) = create_test_store();

    let feedback = Feedback::new(
        "agent-1",
        FeedbackType::ExplicitNegative,
        "Test context",
        "User input",
        "Agent output",
    );

    store.add(feedback).await.unwrap();

    let recent = store.get_recent("agent-1", 10).await;
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].agent_id, "agent-1");
}

#[tokio::test]
async fn test_feedback_store_get_by_type() {
    let (mut store, _dir) = create_test_store();

    store
        .add(Feedback::new(
            "agent-1",
            FeedbackType::ExplicitNegative,
            "Context",
            "Input 1",
            "Output 1",
        ))
        .await
        .unwrap();

    store
        .add(Feedback::new(
            "agent-1",
            FeedbackType::Positive,
            "Context",
            "Input 2",
            "Output 2",
        ))
        .await
        .unwrap();

    store
        .add(Feedback::new(
            "agent-1",
            FeedbackType::ExplicitNegative,
            "Context",
            "Input 3",
            "Output 3",
        ))
        .await
        .unwrap();

    let negative = store.get_by_type(FeedbackType::ExplicitNegative, 10).await;
    assert_eq!(negative.len(), 2);

    let positive = store.get_by_type(FeedbackType::Positive, 10).await;
    assert_eq!(positive.len(), 1);
}

#[tokio::test]
async fn test_feedback_store_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().to_path_buf();

    // Create store and add feedback
    {
        let mut store = FeedbackStore::new(path.clone()).unwrap();
        store
            .add(Feedback::new(
                "agent-1",
                FeedbackType::ExplicitNegative,
                "Context",
                "Input",
                "Output",
            ))
            .await
            .unwrap();
    }

    // Create new store and verify persistence
    {
        let store = FeedbackStore::new(path).unwrap();
        let recent = store.get_recent("agent-1", 10).await;
        assert_eq!(recent.len(), 1);
    }
}

#[tokio::test]
async fn test_auto_eval_process_turn() {
    let temp_dir = TempDir::new().unwrap();
    let mut eval = AutoEval::new(temp_dir.path().to_path_buf()).unwrap();

    // Negative feedback should be detected
    let feedback = eval
        .process_turn(
            "agent-1",
            "That's wrong, try again",
            "Here's the result",
            None,
            None,
        )
        .await
        .unwrap();

    assert!(feedback.is_some());
    assert_eq!(feedback.unwrap().feedback_type, FeedbackType::ExplicitNegative);
}

#[tokio::test]
async fn test_auto_eval_error() {
    let temp_dir = TempDir::new().unwrap();
    let mut eval = AutoEval::new(temp_dir.path().to_path_buf()).unwrap();

    // Error should be recorded
    let feedback = eval
        .process_turn(
            "agent-1",
            "Do something",
            "Failed",
            None,
            Some("Connection timeout"),
        )
        .await
        .unwrap();

    assert!(feedback.is_some());
    let fb = feedback.unwrap();
    assert_eq!(fb.feedback_type, FeedbackType::Error);
    assert_eq!(fb.correction, Some("Connection timeout".to_string()));
}

#[tokio::test]
async fn test_auto_eval_retry_detection() {
    let temp_dir = TempDir::new().unwrap();
    let mut eval = AutoEval::new(temp_dir.path().to_path_buf()).unwrap();

    // First request - no retry
    let feedback = eval
        .process_turn("agent-1", "Generate a report", "Here's the report", None, None)
        .await
        .unwrap();
    assert!(feedback.is_none());

    // Same request again - should be detected as retry
    let feedback = eval
        .process_turn(
            "agent-1",
            "Generate a report",
            "Here's another report",
            Some("Generate a report"),
            None,
        )
        .await
        .unwrap();

    assert!(feedback.is_some());
    assert_eq!(feedback.unwrap().feedback_type, FeedbackType::ImplicitRetry);
}

#[tokio::test]
async fn test_auto_eval_summary() {
    let temp_dir = TempDir::new().unwrap();
    let mut eval = AutoEval::new(temp_dir.path().to_path_buf()).unwrap();

    // Add various feedback
    eval.process_turn("agent-1", "Wrong!", "Output", None, None)
        .await
        .unwrap();
    eval.process_turn("agent-1", "Thanks!", "Output", None, None)
        .await
        .unwrap();
    eval.process_turn("agent-1", "Error", "Output", None, Some("Failed"))
        .await
        .unwrap();

    let summary = eval.summary("agent-1");
    assert_eq!(summary.total, 3);
    assert_eq!(summary.positive, 1);
    assert_eq!(summary.negative, 1);
    assert_eq!(summary.errors, 1);
}

#[tokio::test]
async fn test_improvement_generator() {
    let generator = ImprovementGenerator::new();

    // Not enough feedback
    let feedback: Vec<Feedback> = vec![];
    let improvements = generator.analyze(&feedback).await.unwrap();
    assert!(improvements.is_empty());

    // With enough retry feedback
    let feedback: Vec<Feedback> = (0..6)
        .map(|i| {
            Feedback::new(
                "agent-1",
                FeedbackType::ImplicitRetry,
                "Context",
                format!("Input {}", i),
                "Output",
            )
        })
        .collect();

    let improvements = generator.analyze(&feedback).await.unwrap();
    assert!(!improvements.is_empty());
    assert!(improvements.iter().any(|i| i.category == "clarity"));
}
