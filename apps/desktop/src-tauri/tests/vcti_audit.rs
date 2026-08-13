use std::path::PathBuf;
use vibemeter_lib::database::Database;

#[test]
#[ignore = "requires VIBEMETER_TEST_DB with a sanitized or local VibeMeter database snapshot"]
fn audits_real_profile_ranges() {
    let database_path = std::env::var("VIBEMETER_TEST_DB")
        .or_else(|_| std::env::var("AFTERVIBE_TEST_DB"))
        .or_else(|_| std::env::var("TOKEN_GRAPH_TEST_DB"))
        .map(PathBuf::from)
        .expect("VIBEMETER_TEST_DB");
    let database = Database::open(database_path).expect("database snapshot");

    let mut populated_ranges = 0;
    let mut temporary_ranges = 0;
    let mut collecting_ranges = 0;
    for range in ["today", "7d", "30d", "90d", "180d", "year", "all"] {
        let profile = database.vcti_profile(range).expect("VCTI profile");
        populated_ranges += u64::from(profile.session_count > 0);
        temporary_ranges += u64::from(profile.temporary && profile.session_count > 0);
        collecting_ranges += u64::from(profile.status == "collecting");
        assert_eq!(profile.identity_visual.range, range);
        assert_eq!(
            profile.identity_visual.algorithm_version,
            profile.algorithm_version
        );
        if profile.status == "collecting" {
            assert!(!profile.identity_visual.available);
            assert!(profile.identity_visual.paths.is_empty());
        }
        println!(
            "{range}: primary={} secondary={} badges={} status={} confidence={:.1}% margin={:.1}% sessions={} days={} visual={} paths={}",
            profile.primary_type.as_deref().unwrap_or("unassigned"),
            profile.secondary_type.as_deref().unwrap_or("none"),
            profile
                .badges
                .iter()
                .map(|badge| badge.code.as_str())
                .collect::<Vec<_>>()
                .join(","),
            profile.status,
            profile.confidence,
            profile.type_margin * 100.0,
            profile.session_count,
            profile.active_days,
            profile.identity_visual.version,
            profile.identity_visual.paths.len()
        );
    }
    assert!(
        populated_ranges >= 2,
        "two populated real-data ranges required"
    );
    assert!(
        temporary_ranges >= 1,
        "one temporary real-data range required"
    );
    assert!(collecting_ranges >= 1, "one collecting range required");
}
