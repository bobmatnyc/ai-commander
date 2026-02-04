//! Tests for the change detection module.

use std::time::Duration;

use super::*;

#[test]
fn test_change_event_none() {
    let event = ChangeEvent::none();
    assert_eq!(event.change_type, ChangeType::None);
    assert_eq!(event.significance, Significance::Ignore);
    assert!(!event.is_meaningful());
    assert!(!event.requires_notification());
}

#[test]
fn test_significance_ordering() {
    assert!(Significance::Ignore < Significance::Low);
    assert!(Significance::Low < Significance::Medium);
    assert!(Significance::Medium < Significance::High);
    assert!(Significance::High < Significance::Critical);
}

#[test]
fn test_detector_no_change() {
    let mut detector = ChangeDetector::new();
    let output = "Some output text\nMore text";

    // First detection - establishes baseline
    let event1 = detector.detect(output);
    assert!(matches!(event1.change_type, ChangeType::Addition));

    // Same output - no change
    let event2 = detector.detect(output);
    assert_eq!(event2.change_type, ChangeType::None);
    assert_eq!(event2.significance, Significance::Ignore);
}

#[test]
fn test_detector_detects_addition() {
    let mut detector = ChangeDetector::new();

    detector.detect("Line 1");
    let event = detector.detect("Line 1\nLine 2\nLine 3");

    assert_eq!(event.change_type, ChangeType::Addition);
    assert!(event.diff_lines.len() >= 2);
}

#[test]
fn test_detector_detects_completion() {
    let mut detector = ChangeDetector::new();

    detector.detect("Starting task...");
    let event = detector.detect("Starting task...\nTask completed successfully!");

    assert_eq!(event.change_type, ChangeType::Completion);
    assert_eq!(event.significance, Significance::High);
}

#[test]
fn test_detector_detects_error() {
    let mut detector = ChangeDetector::new();

    detector.detect("Running tests...");
    let event = detector.detect("Running tests...\nError: test failed!");

    assert_eq!(event.change_type, ChangeType::Error);
    assert!(event.significance >= Significance::High);
}

#[test]
fn test_detector_detects_waiting_for_input() {
    let mut detector = ChangeDetector::new();

    detector.detect("Installing package...");
    let event = detector.detect("Installing package...\nProceed? [y/n]");

    assert_eq!(event.change_type, ChangeType::WaitingForInput);
    assert_eq!(event.significance, Significance::High);
}

#[test]
fn test_detector_filters_noise() {
    let mut detector = ChangeDetector::new();

    // First establish baseline with noise
    detector.detect("Content line\n⠋ Loading...");

    // Add new content with different spinner
    let event = detector.detect("Content line\n⠙ Loading...\nNew actual content");

    // Should detect the new content, not spinner changes
    assert!(event.diff_lines.iter().any(|l| l.contains("actual content")));
}

#[test]
fn test_detector_detects_test_results() {
    let mut detector = ChangeDetector::new();

    detector.detect("Running tests");
    let event = detector.detect("Running tests\n42 tests passed, 3 failed");

    assert_eq!(event.change_type, ChangeType::Progress);
    assert_eq!(event.significance, Significance::Medium);
}

#[test]
fn test_detector_reset() {
    let mut detector = ChangeDetector::new();

    detector.detect("Some output");
    detector.reset();

    // After reset, same output should be detected as new
    let event = detector.detect("Some output");
    assert!(!matches!(event.change_type, ChangeType::None));
}

#[test]
fn test_smart_poller_default() {
    let poller = SmartPoller::default();
    assert_eq!(poller.interval(), Duration::from_millis(500));
}

#[test]
fn test_smart_poller_speeds_up_on_activity() {
    let mut poller = SmartPoller::new(Duration::from_millis(100), Duration::from_secs(10));

    // Slow down first
    for _ in 0..10 {
        poller.next_interval(&ChangeEvent::none());
    }
    let slow_interval = poller.interval();

    // Activity should speed up
    let event = ChangeEvent {
        change_type: ChangeType::Error,
        significance: Significance::High,
        ..Default::default()
    };
    poller.next_interval(&event);

    assert!(poller.interval() < slow_interval);
}

#[test]
fn test_smart_poller_slows_down_when_idle() {
    let mut poller = SmartPoller::new(Duration::from_millis(100), Duration::from_secs(10));

    let initial = poller.interval();

    // Simulate idle period
    for _ in 0..10 {
        poller.next_interval(&ChangeEvent::none());
    }

    assert!(poller.interval() > initial);
    assert!(poller.is_idle());
}

#[test]
fn test_smart_poller_respects_max_interval() {
    let mut poller = SmartPoller::new(Duration::from_millis(100), Duration::from_secs(1));

    // Try to slow down a lot
    for _ in 0..100 {
        poller.next_interval(&ChangeEvent::none());
    }

    assert!(poller.interval() <= Duration::from_secs(1));
}

#[test]
fn test_smart_poller_reset() {
    let mut poller = SmartPoller::new(Duration::from_millis(100), Duration::from_secs(10));

    // Slow down
    for _ in 0..10 {
        poller.next_interval(&ChangeEvent::none());
    }

    // Reset
    poller.reset();

    assert_eq!(poller.interval(), Duration::from_millis(100));
    assert!(!poller.is_idle());
}

#[test]
fn test_change_notification_fields() {
    let notification = ChangeNotification {
        session_id: "test-session".to_string(),
        summary: "Task completed".to_string(),
        requires_action: false,
        change_type: ChangeType::Completion,
        significance: Significance::High,
    };

    assert_eq!(notification.session_id, "test-session");
    assert!(!notification.requires_action);
}

#[test]
fn test_custom_patterns() {
    let mut detector = ChangeDetector::new();

    // Add custom pattern for deployment
    detector
        .add_significant_pattern(
            r"(?i)deployed to \w+",
            ChangeType::Completion,
            Significance::Critical,
        )
        .unwrap();

    detector.detect("Starting deployment");
    let event = detector.detect("Starting deployment\nDeployed to production!");

    assert_eq!(event.change_type, ChangeType::Completion);
    assert_eq!(event.significance, Significance::Critical);
}

#[test]
fn test_summary_truncation() {
    let mut detector = ChangeDetector::new();
    detector.detect("");

    let long_line = "x".repeat(200);
    let event = detector.detect(&long_line);

    // Summary should be truncated
    assert!(event.summary.len() < 200);
    assert!(event.summary.contains("..."));
}

#[test]
fn test_significance_meaningful_threshold() {
    // Low significance is not meaningful
    let low_event = ChangeEvent {
        significance: Significance::Low,
        ..Default::default()
    };
    assert!(!low_event.is_meaningful());

    // Medium significance is meaningful
    let medium_event = ChangeEvent {
        significance: Significance::Medium,
        ..Default::default()
    };
    assert!(medium_event.is_meaningful());
}

#[test]
fn test_notification_threshold() {
    // Medium significance does not require notification
    let medium_event = ChangeEvent {
        significance: Significance::Medium,
        ..Default::default()
    };
    assert!(!medium_event.requires_notification());

    // High significance requires notification
    let high_event = ChangeEvent {
        significance: Significance::High,
        ..Default::default()
    };
    assert!(high_event.requires_notification());
}
