use crate::db::MemoryItem;
use crate::error::AppError;
use chrono::{Duration, Utc};

#[derive(Debug, Clone, Copy)]
pub enum Rating {
    Again,
    Hard,
    Easy,
}

impl Rating {
    pub fn from_str(s: &str) -> Result<Self, AppError> {
        match s {
            "again" | "不认识" => Ok(Self::Again),
            "hard" | "模糊" => Ok(Self::Hard),
            "easy" | "认识" => Ok(Self::Easy),
            _ => Err(AppError::msg(format!("unknown rating: {s}"))),
        }
    }
}

/// Simplified SRS: again ~10m, hard 1d, easy 1→7→14 days.
/// Mastered when 3 *consecutive* easy ratings reach the 14-day step: any
/// again/hard rating resets the streak. (PRD requires "3 consecutive easy
/// ratings reaching the 14-day step", so the ladder has 3 steps.)
pub fn apply_rating(item: &mut MemoryItem, rating: Rating) {
    item.reps += 1;
    let now = Utc::now();
    match rating {
        Rating::Again => {
            item.consecutive_know = 0;
            item.interval_days = 0.0;
            item.next_review_at = (now + Duration::minutes(10)).to_rfc3339();
        }
        Rating::Hard => {
            // Gentle reminder: schedule tomorrow, but a fuzzy recall is not
            // an easy one — the consecutive-easy streak restarts.
            item.consecutive_know = 0;
            item.interval_days = 1.0;
            item.next_review_at = (now + Duration::days(1)).to_rfc3339();
        }
        Rating::Easy => {
            item.consecutive_know += 1;
            let next = match item.interval_days {
                x if x < 1.0 => 1.0,
                x if x < 7.0 => 7.0,
                _ => 14.0,
            };
            item.interval_days = next;
            item.next_review_at = (now + Duration::days(next as i64)).to_rfc3339();
            // PRD: 3 consecutive easy ratings reaching the 14-day step → mastered.
            if item.consecutive_know >= 3 && (next - 14.0).abs() < f64::EPSILON {
                item.status = "mastered".into();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MemoryItem {
        MemoryItem {
            id: "1".into(),
            kind: "word".into(),
            term: "t".into(),
            definition_zh: "测".into(),
            word_type: "noun".into(),
            collocations: vec![],
            context_sentence: "".into(),
            article_id: None,
            status: "learning".into(),
            interval_days: 0.0,
            reps: 0,
            consecutive_know: 0,
            next_review_at: Utc::now().to_rfc3339(),
            created_at: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn easy_progresses_and_masters() {
        // Ladder 1→7→14: 3 consecutive easies reach the 14-day step and graduate.
        let mut item = sample();
        apply_rating(&mut item, Rating::Easy);
        assert_eq!(item.interval_days, 1.0);
        assert_eq!(item.status, "learning");
        apply_rating(&mut item, Rating::Easy);
        assert_eq!(item.interval_days, 7.0);
        assert_eq!(item.status, "learning");
        apply_rating(&mut item, Rating::Easy);
        assert_eq!(item.interval_days, 14.0);
        assert_eq!(item.consecutive_know, 3);
        assert_eq!(item.status, "mastered");
    }

    #[test]
    fn hard_schedules_tomorrow_and_breaks_streak() {
        let mut item = sample();
        apply_rating(&mut item, Rating::Easy);
        apply_rating(&mut item, Rating::Easy);
        assert_eq!(item.consecutive_know, 2);
        apply_rating(&mut item, Rating::Hard);
        assert_eq!(item.interval_days, 1.0);
        assert_eq!(item.consecutive_know, 0, "hard breaks the easy streak");
        assert_eq!(item.status, "learning");
    }

    #[test]
    fn interrupted_streak_never_masters() {
        let mut item = sample();
        for rating in [Rating::Easy, Rating::Easy, Rating::Hard, Rating::Easy] {
            apply_rating(&mut item, rating);
        }
        assert_eq!(item.status, "learning");
        assert_eq!(item.consecutive_know, 1);
    }

    #[test]
    fn again_resets_streak() {
        let mut item = sample();
        apply_rating(&mut item, Rating::Easy);
        apply_rating(&mut item, Rating::Again);
        assert_eq!(item.consecutive_know, 0);
        assert_eq!(item.interval_days, 0.0);
    }
}
