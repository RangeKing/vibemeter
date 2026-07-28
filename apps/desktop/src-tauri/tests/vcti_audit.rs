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

    for range in ["7d", "30d", "90d", "180d", "year", "all"] {
        let profile = database.vcti_profile(range).expect("VCTI profile");
        println!(
            "{range}: primary={} secondary={} badges={} status={} confidence={:.1}% margin={:.1}% sessions={} days={}",
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
            profile.active_days
        );
    }
}
