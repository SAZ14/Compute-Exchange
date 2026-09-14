use exchange_core::{
    AccountId, Command, Event, Exchange, ExchangeError, JobId, JobState, OfferId, OfferState,
    WorkerId,
};

fn fund(exchange: &mut Exchange, amount: u64) {
    exchange
        .execute(
            Command::Fund {
                account: AccountId(1),
                amount,
            },
            0,
        )
        .unwrap();
}

fn offer(id: u64, price: u64) -> Command {
    Command::RegisterOffer {
        id: OfferId(id),
        worker: WorkerId(id),
        provider: AccountId(100),
        price_per_second: price,
    }
}

fn job(id: u64, price: u64, duration: u64) -> Command {
    Command::SubmitJob {
        id: JobId(id),
        buyer: AccountId(1),
        max_price_per_second: price,
        duration_seconds: duration,
    }
}

fn assert_invariants(exchange: &Exchange) {
    let total: u128 = exchange
        .accounts()
        .values()
        .map(|balance| u128::from(balance.available) + u128::from(balance.reserved))
        .sum();
    assert_eq!(total, u128::from(exchange.total_funded()));
    for (account, balance) in exchange.accounts() {
        let held: u128 = exchange
            .jobs()
            .values()
            .filter(|job| job.buyer == *account)
            .map(|job| u128::from(job.held_microcredits))
            .sum();
        assert_eq!(held, u128::from(balance.reserved));
    }
    for (id, reservation) in exchange.reservations() {
        assert_eq!(
            exchange.jobs()[id].state,
            JobState::Reserved {
                offer_id: reservation.offer_id
            }
        );
        assert_eq!(
            exchange.offers()[&reservation.offer_id].state,
            OfferState::Reserved { job_id: *id }
        );
        assert_eq!(
            reservation.held_microcredits,
            reservation.price_per_second * reservation.duration_seconds
        );
    }
    let reserved_offers = exchange
        .offers()
        .values()
        .filter(|offer| matches!(offer.state, OfferState::Reserved { .. }))
        .count();
    assert_eq!(reserved_offers, exchange.reservations().len());
}

#[test]
fn cheapest_offer_wins_and_releases_price_improvement() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 100);
    exchange.execute(offer(1, 8), 0).unwrap();
    exchange.execute(offer(2, 3), 0).unwrap();
    let events = exchange.execute(job(1, 10, 5), 0).unwrap();
    assert_eq!(exchange.reservations()[&JobId(1)].offer_id, OfferId(2));
    assert_eq!(exchange.balance(AccountId(1)).available, 85);
    assert_eq!(exchange.balance(AccountId(1)).reserved, 15);
    assert!(matches!(
        events.as_slice(),
        [Event::JobQueued(_), Event::JobReserved(_)]
    ));
    assert_invariants(&exchange);
}

#[test]
fn equal_prices_use_offer_arrival_not_identifier() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 100);
    exchange.execute(offer(20, 3), 0).unwrap();
    exchange.execute(offer(10, 3), 0).unwrap();
    exchange.execute(job(1, 5, 1), 0).unwrap();
    assert_eq!(exchange.reservations()[&JobId(1)].offer_id, OfferId(20));
}

#[test]
fn queued_jobs_use_arrival_order_not_identifier() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 100);
    exchange.execute(job(20, 5, 1), 0).unwrap();
    exchange.execute(job(10, 5, 1), 0).unwrap();
    exchange.execute(offer(1, 3), 0).unwrap();
    assert!(exchange.reservations().contains_key(&JobId(20)));
    assert_eq!(exchange.jobs()[&JobId(10)].state, JobState::Queued);
    assert_invariants(&exchange);
}

#[test]
fn incompatible_older_job_does_not_block_matching() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 100);
    exchange.execute(job(1, 2, 1), 0).unwrap();
    exchange.execute(job(2, 5, 1), 0).unwrap();
    exchange.execute(offer(1, 3), 0).unwrap();
    assert_eq!(exchange.jobs()[&JobId(1)].state, JobState::Queued);
    assert!(exchange.reservations().contains_key(&JobId(2)));
}

#[test]
fn queued_holds_prevent_overspending_and_rejection_is_atomic() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 100);
    exchange.execute(job(1, 6, 10), 0).unwrap();
    let before = exchange.clone();
    assert_eq!(
        exchange.execute(job(2, 5, 10), 1),
        Err(ExchangeError::InsufficientFunds)
    );
    assert_eq!(exchange, before);
    assert_invariants(&exchange);
}

#[test]
fn cancellation_releases_hold_and_cannot_be_repeated() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 100);
    exchange.execute(job(1, 6, 10), 0).unwrap();
    let cancel = Command::CancelJob {
        id: JobId(1),
        buyer: AccountId(1),
    };
    exchange.execute(cancel.clone(), 0).unwrap();
    assert_eq!(exchange.balance(AccountId(1)).available, 100);
    assert_eq!(exchange.balance(AccountId(1)).reserved, 0);
    let before = exchange.clone();
    assert_eq!(
        exchange.execute(cancel, 0),
        Err(ExchangeError::JobNotQueued)
    );
    assert_eq!(exchange, before);
    assert_invariants(&exchange);
}

#[test]
fn cancellation_checks_buyer_and_state() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 100);
    exchange.execute(job(1, 5, 1), 0).unwrap();
    let before = exchange.clone();
    assert_eq!(
        exchange.execute(
            Command::CancelJob {
                id: JobId(1),
                buyer: AccountId(2)
            },
            0
        ),
        Err(ExchangeError::WrongBuyer)
    );
    assert_eq!(exchange, before);
    exchange.execute(offer(1, 3), 0).unwrap();
    let before = exchange.clone();
    assert_eq!(
        exchange.execute(
            Command::CancelJob {
                id: JobId(1),
                buyer: AccountId(1)
            },
            0
        ),
        Err(ExchangeError::JobNotQueued)
    );
    assert_eq!(exchange, before);
}

#[test]
fn money_overflow_is_atomic() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, u64::MAX);
    let before = exchange.clone();
    assert_eq!(
        exchange.execute(job(1, u64::MAX, 2), 0),
        Err(ExchangeError::ArithmeticOverflow)
    );
    assert_eq!(exchange, before);
    assert_eq!(
        exchange.execute(
            Command::Fund {
                account: AccountId(2),
                amount: 1
            },
            0
        ),
        Err(ExchangeError::ArithmeticOverflow)
    );
    assert_eq!(exchange, before);
}

#[test]
fn one_worker_slot_cannot_be_allocated_twice() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 100);
    exchange.execute(offer(1, 3), 0).unwrap();
    exchange.execute(job(1, 5, 1), 0).unwrap();
    exchange.execute(job(2, 5, 1), 0).unwrap();
    assert_eq!(exchange.reservations().len(), 1);
    assert_eq!(exchange.jobs()[&JobId(2)].state, JobState::Queued);
    let before = exchange.clone();
    assert_eq!(
        exchange.execute(
            Command::RegisterOffer {
                id: OfferId(2),
                worker: WorkerId(1),
                provider: AccountId(100),
                price_per_second: 2
            },
            0
        ),
        Err(ExchangeError::WorkerAlreadyRegistered)
    );
    assert_eq!(exchange, before);
    assert_invariants(&exchange);
}

#[test]
fn duplicate_identifiers_are_rejected_without_mutation() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 100);
    exchange.execute(offer(1, 3), 0).unwrap();
    exchange.execute(job(1, 5, 1), 0).unwrap();
    let before = exchange.clone();
    assert_eq!(
        exchange.execute(offer(1, 4), 0),
        Err(ExchangeError::DuplicateOffer)
    );
    assert_eq!(exchange, before);
    assert_eq!(
        exchange.execute(job(1, 5, 1), 0),
        Err(ExchangeError::DuplicateJob)
    );
    assert_eq!(exchange, before);
}

#[test]
fn invalid_values_and_backwards_time_are_atomic() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 100);
    let before = exchange.clone();
    for command in [job(1, 0, 1), job(1, 1, 0), offer(1, 0)] {
        assert_eq!(exchange.execute(command, 0), Err(ExchangeError::ZeroValue));
        assert_eq!(exchange, before);
    }
    exchange.execute(job(1, 1, 1), 10).unwrap();
    let before = exchange.clone();
    assert_eq!(
        exchange.execute(offer(1, 1), 9),
        Err(ExchangeError::TimeWentBackwards)
    );
    assert_eq!(exchange, before);
}

#[test]
fn same_commands_and_times_produce_identical_states_and_events() {
    let commands = [
        Command::Fund {
            account: AccountId(1),
            amount: 1_000,
        },
        job(1, 4, 10),
        job(2, 6, 10),
        offer(2, 5),
        offer(1, 3),
    ];
    let mut first = Exchange::new();
    let mut second = Exchange::new();
    for (now, command) in commands.into_iter().enumerate() {
        let left = first.execute(command.clone(), now as u64).unwrap();
        let right = second.execute(command, now as u64).unwrap();
        assert_eq!(left, right);
        assert_eq!(first, second);
        assert_invariants(&first);
    }
}

#[test]
fn varied_commands_preserve_accounting_and_slot_invariants() {
    let mut exchange = Exchange::new();
    fund(&mut exchange, 1_000_000);
    for id in 1..=100 {
        exchange
            .execute(job(id, id % 7 + 1, id % 5 + 1), 0)
            .unwrap();
        assert_invariants(&exchange);
        if id % 3 == 0 {
            exchange.execute(offer(id, id % 9 + 1), 0).unwrap();
            assert_invariants(&exchange);
        }
    }
    let queued: Vec<_> = exchange
        .jobs()
        .values()
        .filter(|job| job.state == JobState::Queued)
        .map(|job| job.id)
        .collect();
    for id in queued {
        exchange
            .execute(
                Command::CancelJob {
                    id,
                    buyer: AccountId(1),
                },
                0,
            )
            .unwrap();
        assert_invariants(&exchange);
    }
}
