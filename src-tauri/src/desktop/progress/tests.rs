use super::*;

    fn progress(id: &str) -> ActivationProgress {
        ActivationProgress { operation_id: id.into(), profile_name: "Flight rig".into(), title: "Activating profile".into(), phase: ActivationPhase::Preparing, message: "Preparing companions".into(), details: vec![], can_cancel: true }
    }

    #[test]
    fn stale_window_actions_cannot_change_or_cancel_the_next_activation() {
        let state = ActivationWindowState::default();
        state.start(progress("old-operation"));
        assert!(state.request_cancel("old-operation"));
        state.start(progress("new-operation"));
        assert!(!state.update(progress("old-operation"), false));
        assert!(!state.request_cancel("old-operation"));
        assert!(!state.finish("old-operation"));
        assert!(!state.snapshot().unwrap().cancel_requested);
        assert!(state.request_cancel("new-operation"));
        assert!(state.snapshot().unwrap().cancel_requested);
        assert!(state.finish("new-operation"));
        assert!(state.snapshot().is_none());
    }

    #[test]
    fn progress_updates_preserve_cancel_intent_and_disable_repeated_cancel() {
        let state = ActivationWindowState::default();
        state.start(progress("operation"));
        assert!(state.request_cancel("operation"));
        let mut next = progress("operation");
        next.phase = ActivationPhase::Cancelling;
        next.can_cancel = false;
        assert!(state.update(next, false));
        assert!(state.snapshot().unwrap().cancel_requested);
        assert!(!state.request_cancel("operation"));
    }

    #[test]
    fn a_failed_cancel_request_can_be_retried_without_restarting_activation() {
        let state = ActivationWindowState::default();
        state.start(progress("operation"));
        assert!(state.request_cancel("operation"));
        assert!(!state.update(progress("old-operation"), true));
        assert!(state.snapshot().unwrap().cancel_requested);
        assert!(state.update(progress("operation"), true));
        assert!(!state.snapshot().unwrap().cancel_requested);
        assert!(state.request_cancel("operation"));
    }

    #[test]
    fn bounds_reject_untrusted_or_unbounded_progress() {
        assert!(progress("a-valid-operation_1").validate().is_ok());
        assert!(progress("../other?route=1").validate().is_err());
        let mut value = progress("operation");
        value.details = vec![String::new(); 33];
        assert!(value.validate().is_err());
    }

    #[test]
    fn game_launch_status_keeps_the_same_window_without_allowing_cancel() {
        let state = ActivationWindowState::default();
        state.start(progress("operation"));
        let mut launching = progress("operation");
        launching.phase = ActivationPhase::Launching;
        launching.message = "Launching Assetto Corsa Rally".into();
        launching.can_cancel = false;
        assert!(launching.validate().is_ok());
        assert!(state.update(launching, false));
        assert!(!state.request_cancel("operation"));
        let snapshot = serde_json::to_value(state.snapshot().unwrap()).unwrap();
        assert_eq!(snapshot["progress"]["phase"], "launching");
        assert_eq!(snapshot["progress"]["message"], "Launching Assetto Corsa Rally");
        assert!(state.finish("operation"));
    }

    #[test]
    fn cleanup_phases_retain_native_status_and_refuse_cancel_even_with_a_stale_flag() {
        for (phase, serialized) in [(ActivationPhase::Deactivating, "deactivating"), (ActivationPhase::Recovering, "recovering")] {
            let state = ActivationWindowState::default();
            state.start(progress("operation"));
            assert!(state.request_cancel("operation"));
            let mut cleanup = progress("operation");
            cleanup.phase = phase;
            cleanup.message = "Unhiding peripherals".into();
            // The phase remains authoritative if a stale renderer carries the
            // prior cancellable activation flag across the cleanup boundary.
            cleanup.can_cancel = true;
            assert!(cleanup.validate().is_ok());
            assert!(state.update(cleanup, true));
            assert!(!state.request_cancel("operation"));
            let snapshot = serde_json::to_value(state.snapshot().unwrap()).unwrap();
            assert_eq!(snapshot["progress"]["phase"], serialized);
            assert_eq!(snapshot["progress"]["message"], "Unhiding peripherals");
            assert_eq!(snapshot["cancelRequested"], false);
            assert!(state.finish("operation"));
        }
    }

    #[test]
    fn vehicle_switching_progress_is_never_cancellable() {
        let state = ActivationWindowState::default();
        let mut switching = progress("vehicle-switch-1");
        switching.phase = ActivationPhase::Switching;
        // A renderer cannot enable cancellation of an automatic switch by
        // carrying the manual activation's previous flag into its next update.
        switching.can_cancel = true;
        assert!(switching.validate().is_ok());
        state.start(switching);
        assert!(!state.request_cancel("vehicle-switch-1"));
        assert_eq!(serde_json::to_value(state.snapshot().unwrap()).unwrap()["progress"]["phase"], "switching");
    }

    #[test]
    fn notification_corners_respect_the_work_area_and_taskbar() {
        let cases = [
            (NotificationPosition::BottomRight, 1484, 928),
            (NotificationPosition::BottomLeft, 16, 928),
            (NotificationPosition::TopRight, 1484, 16),
            (NotificationPosition::TopLeft, 16, 16),
        ];
        for (position, x, y) in cases {
            assert_eq!(notification_geometry(position, (0, 0), (1920, 1040), 1.0).unwrap(), NotificationGeometry { x, y, width: 420, height: 96 });
        }
    }

    #[test]
    fn mixed_dpi_and_negative_monitor_origins_use_physical_coordinates() {
        assert_eq!(notification_geometry(NotificationPosition::BottomRight, (-2560, -120), (2560, 1400), 1.5).unwrap(), NotificationGeometry { x: -654, y: 1112, width: 630, height: 144 });
        assert_eq!(notification_geometry(NotificationPosition::TopLeft, (-1920, 80), (1920, 1000), 1.25).unwrap(), NotificationGeometry { x: -1900, y: 100, width: 525, height: 120 });
    }

    #[test]
    fn tiny_work_areas_keep_the_whole_notification_visible() {
        for position in [NotificationPosition::BottomRight, NotificationPosition::BottomLeft, NotificationPosition::TopRight, NotificationPosition::TopLeft] {
            assert_eq!(notification_geometry(position, (-320, -40), (300, 80), 2.0).unwrap(), NotificationGeometry { x: -320, y: -40, width: 300, height: 80 });
            let geometry = notification_geometry(position, (-430, 0), (430, 100), 1.0).unwrap();
            assert_eq!(geometry.width, 420);
            assert_eq!(geometry.height, 96);
            assert!(geometry.x >= -430 && i64::from(geometry.x) + i64::from(geometry.width) <= 0);
            assert!(geometry.y >= 0 && geometry.y + geometry.height as i32 <= 100);
        }
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(notification_geometry(NotificationPosition::BottomRight, (0, 0), (1920, 1080), scale).is_err());
        }
        assert!(notification_geometry(NotificationPosition::BottomRight, (0, 0), (0, 1080), 1.0).is_err());
    }

    #[test]
    fn stale_delivery_results_cannot_replace_the_current_operation() {
        let state = ActivationWindowState::default();
        state.start(progress("old"));
        assert!(state.delivery("old", ProgressDelivery::System));
        state.start(progress("current"));
        assert!(!state.delivery("old", ProgressDelivery::System));
        assert_eq!(state.snapshot().unwrap().delivery, ProgressDelivery::Window);
        assert!(state.delivery("current", ProgressDelivery::System));
        assert!(!state.finish("old"));
        assert_eq!(state.snapshot().unwrap().delivery, ProgressDelivery::System);
    }


