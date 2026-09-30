//! Secret Santa's business rules, as pure functions of the model.

use std::collections::HashSet;

use rand::Rng;
use rand::seq::SliceRandom;
use serenity::all::UserId;

use super::model::{Event, EventStatus, SsError};

pub fn ensure_host(event: &Event, user: UserId) -> Result<(), SsError> {
    if event.host == user {
        Ok(())
    } else {
        Err(SsError::NotHost)
    }
}

/// Participants can only change before assignments are drawn.
pub fn ensure_participants_editable(event: &Event) -> Result<(), SsError> {
    match event.status {
        EventStatus::PreRun => Ok(()),
        _ => Err(SsError::ParticipantsLocked),
    }
}

pub fn ensure_can_start(event: &Event) -> Result<(), SsError> {
    match event.status {
        EventStatus::PreRun => Ok(()),
        _ => Err(SsError::NotPreparing),
    }
}

pub fn ensure_can_end(event: &Event) -> Result<(), SsError> {
    match event.status {
        EventStatus::Running => Ok(()),
        _ => Err(SsError::NotRunning),
    }
}

pub fn ensure_can_cancel(event: &Event) -> Result<(), SsError> {
    match event.status {
        EventStatus::PreRun | EventStatus::Running => Ok(()),
        EventStatus::Finished => Err(SsError::AlreadyFinished),
    }
}

/// Draw `(santa, recipient)` pairs: shuffle, then each person gives to the next, wrapping
/// around. This forms one cycle, so everyone gives and receives exactly once and nobody
/// draws themselves.
pub fn assign_santas(
    participants: &[UserId],
    rng: &mut impl Rng,
) -> Result<Vec<(UserId, UserId)>, SsError> {
    if participants.len() < 2 {
        return Err(SsError::NotEnoughParticipants);
    }
    let mut order = participants.to_vec();
    order.shuffle(rng);
    Ok((0..order.len())
        .map(|i| (order[i], order[(i + 1) % order.len()]))
        .collect())
}

/// Changes that turn `existing` participants into `selected`: `(add, remove)`.
pub fn participant_diff(existing: &[UserId], selected: &[UserId]) -> (Vec<UserId>, Vec<UserId>) {
    let existing_set: HashSet<UserId> = existing.iter().copied().collect();
    let selected_set: HashSet<UserId> = selected.iter().copied().collect();
    let add = selected
        .iter()
        .copied()
        .filter(|user| !existing_set.contains(user))
        .collect();
    let remove = existing
        .iter()
        .copied()
        .filter(|user| !selected_set.contains(user))
        .collect();
    (add, remove)
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;
    use crate::features::secret_santa::model::EventId;

    fn user(id: u64) -> UserId {
        UserId::new(id)
    }

    fn users(ids: &[u64]) -> Vec<UserId> {
        ids.iter().copied().map(user).collect()
    }

    fn event(status: EventStatus) -> Event {
        Event {
            id: EventId(1),
            name: "Party".into(),
            description: None,
            host: user(1),
            status,
        }
    }

    #[test]
    fn only_the_host_passes_ensure_host() {
        let event = event(EventStatus::PreRun);
        assert!(ensure_host(&event, user(1)).is_ok());
        assert!(matches!(
            ensure_host(&event, user(2)),
            Err(SsError::NotHost)
        ));
    }

    #[test]
    fn status_rules() {
        use EventStatus::*;
        let allowed =
            |rule: fn(&Event) -> Result<(), SsError>, status| rule(&event(status)).is_ok();

        // (status, edit participants, start, end, cancel)
        let table = [
            (PreRun, true, true, false, true),
            (Running, false, false, true, true),
            (Finished, false, false, false, false),
        ];
        for (status, edit, start, end, cancel) in table {
            assert_eq!(
                allowed(ensure_participants_editable, status),
                edit,
                "edit {status:?}"
            );
            assert_eq!(allowed(ensure_can_start, status), start, "start {status:?}");
            assert_eq!(allowed(ensure_can_end, status), end, "end {status:?}");
            assert_eq!(
                allowed(ensure_can_cancel, status),
                cancel,
                "cancel {status:?}"
            );
        }
    }

    #[test]
    fn assignments_form_a_single_cycle() {
        for n in 2..=12 {
            let people = users(&(1..=n).collect::<Vec<_>>());
            for seed in 0..20 {
                let pairs = assign_santas(&people, &mut StdRng::seed_from_u64(seed)).unwrap();

                assert_eq!(pairs.len(), people.len());
                assert!(pairs.iter().all(|(santa, recipient)| santa != recipient));
                // Following santa → recipient from anyone visits everyone once.
                let mut seen = HashSet::new();
                let mut current = people[0];
                for _ in 0..people.len() {
                    assert!(seen.insert(current), "revisited {current}");
                    current = pairs.iter().find(|(s, _)| *s == current).unwrap().1;
                }
                assert_eq!(current, people[0]);
            }
        }
    }

    #[test]
    fn assignment_needs_two_people() {
        let mut rng = StdRng::seed_from_u64(0);
        assert!(matches!(
            assign_santas(&users(&[1]), &mut rng),
            Err(SsError::NotEnoughParticipants)
        ));
        assert!(matches!(
            assign_santas(&[], &mut rng),
            Err(SsError::NotEnoughParticipants)
        ));
    }

    #[test]
    fn diff_adds_new_and_removes_unselected() {
        let (add, remove) = participant_diff(&users(&[1, 2, 3]), &users(&[1, 3, 4]));

        assert_eq!(add, users(&[4]));
        assert_eq!(remove, users(&[2]));
    }
}
