use launchpad_core::frecency::{Frecency, score};

const DAY: u64 = 86_400;
const NOW: u64 = 1_000 * DAY;

#[test]
fn launches_decay_with_a_10_day_half_life() {
    assert_eq!(score([], NOW), 0);
    assert_eq!(score([NOW], NOW), 1);
    // exp(-0.0693 * 10) is just above 0.5, which rounds up.
    assert_eq!(score([NOW - 10 * DAY], NOW), 1);
    assert_eq!(score([NOW - 11 * DAY], NOW), 0);
    assert_eq!(score([NOW, NOW, NOW - 10 * DAY, NOW - 10 * DAY], NOW), 3);
    assert_eq!(score([NOW + DAY], NOW), 1, "a future launch counts as now");
}

#[test]
fn sums_above_10_grow_as_a_square_root() {
    assert_eq!(score([NOW; 10], NOW), 10);
    // 10 + sqrt(4)
    assert_eq!(score([NOW; 14], NOW), 12);
    // 10 + sqrt(118)
    assert_eq!(score([NOW; 128], NOW), 21);
}

#[test]
fn launches_older_than_30_days_do_not_count() {
    // exp(-0.0693 * 30) is about 0.125, so 8 launches sum to 1.
    assert_eq!(score([NOW - 30 * DAY; 8], NOW), 1);
    assert_eq!(score([NOW - 30 * DAY - 1; 8], NOW), 0);
}

#[test]
fn launches_of_one_directory_add_up_in_any_spelling() {
    let frecency = Frecency::new(
        "/home/ada",
        NOW,
        [
            ("/home/ada/work", NOW),
            ("~/work/", NOW),
            ("/home/ada/other", NOW - 20 * DAY),
            ("relative", NOW),
        ],
    );
    assert_eq!(frecency.get("/home/ada/work"), 2);
    assert_eq!(frecency.get("/home/ada/other"), 0);
    assert_eq!(frecency.get("/home/ada/missing"), 0);
    let frecency = Frecency::new(
        r"C:\Users\Ada",
        NOW,
        [(r"C:\Users\Ada\Work", NOW), (r"c:\users\ada\work", NOW)],
    );
    assert_eq!(frecency.get(r"C:\Users\Ada\WORK"), 2);
}
