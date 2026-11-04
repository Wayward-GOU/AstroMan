use chrono::{Duration, Local, TimeZone};

use super::*;

fn note(title: &str, due: Option<i64>) -> NoteInput {
    NoteInput {
        title: title.to_string(),
        due_at: due,
        ..Default::default()
    }
}

#[test]
fn tags_split_dedupe_and_clip() {
    let tags = normalize_tags(vec![
        "#工作, 个人".into(),
        "＃工作".into(),
        "  ".into(),
        "a".repeat(40),
    ]);
    assert_eq!(
        tags,
        vec!["工作".to_string(), "个人".to_string(), "a".repeat(32)]
    );
}

#[test]
fn monthly_repeat_clamps_to_month_end() {
    // 2027-01-31 → 2027-02-28（非闰年）
    let start = Local
        .with_ymd_and_hms(2027, 1, 31, 9, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    let next = advance_once(start, "monthly").unwrap();
    let date = Local.timestamp_millis_opt(next).single().unwrap();
    assert_eq!(
        date.format("%Y-%m-%d %H:%M").to_string(),
        "2027-02-28 09:00"
    );
}

#[test]
fn weekly_repeat_keeps_weekday_and_time() {
    let start = Local
        .with_ymd_and_hms(2026, 11, 2, 10, 30, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    let next = advance_once(start, "weekly").unwrap();
    let date = Local.timestamp_millis_opt(next).single().unwrap();
    assert_eq!(
        date.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-09 10:30"
    );
}

#[test]
fn weekdays_skip_weekend() {
    // 2026-11-06 是周五
    let start = Local
        .with_ymd_and_hms(2026, 11, 6, 18, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    let next = advance_once(start, "weekdays").unwrap();
    let date = Local.timestamp_millis_opt(next).single().unwrap();
    assert_eq!(
        date.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-09 18:00"
    );
}

#[test]
fn advance_until_future_walks_past_missed_periods() {
    let now = Local
        .with_ymd_and_hms(2026, 11, 10, 12, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    let start = Local
        .with_ymd_and_hms(2026, 11, 1, 9, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    let next = advance_until_future(start, "daily", now);
    let date = Local.timestamp_millis_opt(next).single().unwrap();
    assert_eq!(
        date.format("%Y-%m-%d %H:%M").to_string(),
        "2026-11-11 09:00"
    );
}

#[test]
fn quiet_hours_handle_cross_midnight() {
    let settings = NotesSettings {
        quiet_hours_enabled: true,
        quiet_start: "22:00".into(),
        quiet_end: "08:00".into(),
        ..Default::default()
    };
    let at = |hour: u32, minute: u32| {
        Local
            .with_ymd_and_hms(2026, 11, 4, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp_millis()
    };
    assert!(is_quiet_now(&settings, at(23, 0)));
    assert!(is_quiet_now(&settings, at(7, 59)));
    assert!(!is_quiet_now(&settings, at(8, 0)));
    assert!(!is_quiet_now(&settings, at(12, 0)));
}

#[test]
fn create_list_and_counts_cover_overdue_and_no_due() {
    let (dir, pool) = test_pool("notes-crud");
    let now = now_ms();
    let yesterday = now - 24 * 60 * 60 * 1000;
    let created = create(
        &pool,
        NoteInput {
            title: "逾期任务".into(),
            due_at: Some(yesterday),
            remind_lead_min: Some(15),
            tags: Some(vec!["工作".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(created.summary.remind_at, Some(yesterday - 15 * 60_000));
    assert_eq!(created.summary.tags, vec!["工作"]);

    create(&pool, note("无截止任务", None)).unwrap();

    let list = list(
        &pool,
        &NotesFilter {
            scope: "today".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(list.items.len(), 2, "逾期与无截止都应在默认视图");
    let counts = list.counts;
    assert_eq!(counts.open, 2);
    assert_eq!(counts.overdue, 1);
    assert_eq!(counts.due_today, 0);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn toggle_repeat_rolls_forward() {
    let (dir, pool) = test_pool("notes-repeat");
    let now = now_ms();
    let due = now - 60 * 60 * 1000;
    let created = create(
        &pool,
        NoteInput {
            title: "站会".into(),
            due_at: Some(due),
            repeat: Some("daily".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let result = toggle(&pool, created.summary.id, true).unwrap();
    assert_eq!(result.note.summary.status, "open");
    let rolled = result.rolled_to.expect("重复条目需顺延");
    assert!(rolled > now);
    assert!(result.note.summary.done_at.is_some());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn snooze_one_time_never_earlier_than_remind() {
    let (dir, pool) = test_pool("notes-snooze");
    let now = now_ms();
    let due = now + 30 * 60 * 1000;
    let created = create(
        &pool,
        NoteInput {
            title: "取快递".into(),
            due_at: Some(due),
            remind_lead_min: Some(10),
            ..Default::default()
        },
    )
    .unwrap();

    // 原提醒在 20 分钟后，稍后 10 分钟不能把它提前
    let snoozed = snooze(&pool, created.summary.id, 10).unwrap();
    assert_eq!(snoozed.summary.snooze_until, Some(due - 10 * 60_000));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn snoozed_note_fires_again_after_window() {
    let (dir, pool) = test_pool("notes-snooze-refire");
    let now = now_ms();
    let created = create(
        &pool,
        NoteInput {
            title: "交周报".into(),
            due_at: Some(now - 60_000),
            ..Default::default()
        },
    )
    .unwrap();

    // 第一次到期：调度器标记已通知，随后不应重复
    let first = due_notes(&pool, now).unwrap();
    assert_eq!(first.len(), 1);
    mark_fired(&pool, &first, now).unwrap();
    assert!(due_notes(&pool, now + 1_000).unwrap().is_empty());

    // 点「稍后 10 分钟」后必须在新的时间点重新命中
    let snoozed = snooze(&pool, created.summary.id, 10).unwrap();
    let target = snoozed.summary.snooze_until.expect("未写入稍后时间");
    assert!(target > now);
    assert!(due_notes(&pool, target - 1_000).unwrap().is_empty());
    let again = due_notes(&pool, target).unwrap();
    assert_eq!(again.len(), 1, "稍后到点必须再次提醒");
    mark_fired(&pool, &again, target).unwrap();
    assert!(due_notes(&pool, target + 1_000).unwrap().is_empty());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn kind_defaults_to_todo_and_supports_filter() {
    let (dir, pool) = test_pool("notes-kind");
    let todo = create(&pool, note("买牛奶", None)).unwrap();
    assert_eq!(todo.summary.kind, "todo");

    let idea = create(
        &pool,
        NoteInput {
            title: "写个抓包小工具".into(),
            kind: Some("idea".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(idea.summary.kind, "idea");

    let only_ideas = list(
        &pool,
        &NotesFilter {
            scope: "all".into(),
            kind: Some("idea".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(only_ideas.items.len(), 1);
    assert_eq!(only_ideas.items[0].id, idea.summary.id);

    let updated = update(
        &pool,
        todo.summary.id,
        NotePatch {
            kind: Some("reference".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(updated.summary.kind, "reference");
    assert_eq!(normalize_kind(Some("unknown")), "todo");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn delete_restore_and_clear_done() {
    let (dir, pool) = test_pool("notes-trash");
    let first = create(&pool, note("一条", None)).unwrap();
    let second = create(&pool, note("两条", None)).unwrap();

    delete(&pool, first.summary.id).unwrap();
    let restored = restore(&pool, first.summary.id).unwrap();
    assert_eq!(restored.summary.status, "open");

    toggle(&pool, second.summary.id, true).unwrap();
    assert_eq!(clear_done(&pool).unwrap(), 1);
    assert_eq!(counts(&pool).unwrap().done, 0);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn due_notes_marks_once_then_advances_repeat() {
    let (dir, pool) = test_pool("notes-due");
    let now = now_ms();
    let due = now - 60_000;
    let one_time = create(
        &pool,
        NoteInput {
            title: "一次性".into(),
            due_at: Some(due),
            ..Default::default()
        },
    )
    .unwrap();
    let repeating = create(
        &pool,
        NoteInput {
            title: "每天".into(),
            due_at: Some(due),
            repeat: Some("daily".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let batch = due_notes(&pool, now).unwrap();
    assert_eq!(batch.len(), 2);
    mark_fired(&pool, &batch, now).unwrap();

    // 一次性条目已标记，不再出现；重复条目已推进到未来
    let again = due_notes(&pool, now).unwrap();
    assert!(again.is_empty());

    let detail = get(&pool, repeating.summary.id).unwrap();
    assert!(detail.summary.due_at.unwrap() > now);
    let one = get(&pool, one_time.summary.id).unwrap();
    assert_eq!(one.summary.status, "open");
    assert!(one.summary.snooze_until.is_none());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn list_keyword_matches_content_and_escapes_wildcards() {
    let (dir, pool) = test_pool("notes-search");
    create(
        &pool,
        NoteInput {
            title: "网关排查".into(),
            content: "结论：证书过期".into(),
            ..Default::default()
        },
    )
    .unwrap();
    create(&pool, note("百分之百", None)).unwrap();

    let hit = list(
        &pool,
        &NotesFilter {
            scope: "all".into(),
            keyword: Some("证书".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(hit.items.len(), 1);

    let none = list(
        &pool,
        &NotesFilter {
            scope: "all".into(),
            keyword: Some("%".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(none.items.is_empty(), "通配符必须转义");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn update_clearing_due_resets_schedule() {
    let (dir, pool) = test_pool("notes-update");
    let now = now_ms();
    let created = create(
        &pool,
        NoteInput {
            title: "交周报".into(),
            due_at: Some(now + Duration::hours(2).num_milliseconds()),
            remind_lead_min: Some(15),
            repeat: Some("weekly".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let updated = update(
        &pool,
        created.summary.id,
        NotePatch {
            due_at: Some(None),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(updated.summary.due_at.is_none());
    assert!(updated.summary.remind_at.is_none());
    assert_eq!(updated.summary.repeat, "none");
    assert_eq!(updated.summary.remind_lead_min, 0);

    let _ = std::fs::remove_dir_all(dir);
}
