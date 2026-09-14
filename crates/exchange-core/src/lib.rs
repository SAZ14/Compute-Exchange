//! Deterministic, single-writer compute exchange using simulated microcredits.
//! Commands execute against a candidate state and commit atomically on success.

use std::collections::BTreeMap;
use std::fmt;

macro_rules! identifier {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u64);
    };
}

identifier!(AccountId);
identifier!(WorkerId);
identifier!(OfferId);
identifier!(JobId);

pub const CAPACITY_CLASS: &str = "demo-compute";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Balance {
    pub available: u64,
    pub reserved: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Reserved { offer_id: OfferId },
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferState {
    Available,
    Reserved { job_id: JobId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: JobId,
    pub buyer: AccountId,
    pub max_price_per_second: u64,
    pub duration_seconds: u64,
    pub held_microcredits: u64,
    pub arrival: u64,
    pub state: JobState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub id: OfferId,
    pub worker: WorkerId,
    pub provider: AccountId,
    pub price_per_second: u64,
    pub arrival: u64,
    pub state: OfferState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    pub job_id: JobId,
    pub offer_id: OfferId,
    pub price_per_second: u64,
    pub duration_seconds: u64,
    pub held_microcredits: u64,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Fund {
        account: AccountId,
        amount: u64,
    },
    RegisterOffer {
        id: OfferId,
        worker: WorkerId,
        provider: AccountId,
        price_per_second: u64,
    },
    SubmitJob {
        id: JobId,
        buyer: AccountId,
        max_price_per_second: u64,
        duration_seconds: u64,
    },
    CancelJob {
        id: JobId,
        buyer: AccountId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    AccountFunded {
        account: AccountId,
        amount: u64,
    },
    OfferRegistered(OfferId),
    JobQueued(JobId),
    JobReserved(Reservation),
    JobCancelled(JobId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeError {
    ZeroValue,
    ArithmeticOverflow,
    InsufficientFunds,
    DuplicateJob,
    DuplicateOffer,
    WorkerAlreadyRegistered,
    UnknownJob,
    WrongBuyer,
    JobNotQueued,
    TimeWentBackwards,
}

impl fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ExchangeError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exchange {
    accounts: BTreeMap<AccountId, Balance>,
    offers: BTreeMap<OfferId, Offer>,
    jobs: BTreeMap<JobId, Job>,
    reservations: BTreeMap<JobId, Reservation>,
    workers: BTreeMap<WorkerId, OfferId>,
    sequence: u64,
    last_time: Option<u64>,
    total_funded: u64,
}

impl Exchange {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn accounts(&self) -> &BTreeMap<AccountId, Balance> {
        &self.accounts
    }

    pub fn offers(&self) -> &BTreeMap<OfferId, Offer> {
        &self.offers
    }

    pub fn jobs(&self) -> &BTreeMap<JobId, Job> {
        &self.jobs
    }

    pub fn reservations(&self) -> &BTreeMap<JobId, Reservation> {
        &self.reservations
    }

    pub fn balance(&self, account: AccountId) -> Balance {
        self.accounts.get(&account).copied().unwrap_or_default()
    }

    pub fn total_funded(&self) -> u64 {
        self.total_funded
    }

    /// Time is an externally supplied, nondecreasing logical timestamp.
    /// The caller owns the exchange exclusively. No network or clock access occurs.
    /// Failed commands leave every field unchanged, including time and sequence.
    pub fn execute(
        &mut self,
        command: Command,
        now: u64,
    ) -> Result<Vec<Event>, ExchangeError> {
        if self.last_time.is_some_and(|previous| now < previous) {
            return Err(ExchangeError::TimeWentBackwards);
        }
        let mut candidate = self.clone();
        let events = candidate.apply(command, now)?;
        candidate.last_time = Some(now);
        *self = candidate;
        Ok(events)
    }

    fn next_sequence(&mut self) -> Result<u64, ExchangeError> {
        self.sequence = add(self.sequence, 1)?;
        Ok(self.sequence)
    }

    fn apply(&mut self, command: Command, now: u64) -> Result<Vec<Event>, ExchangeError> {
        let mut events = Vec::new();
        match command {
            Command::Fund { account, amount } => {
                positive(amount)?;
                self.total_funded = add(self.total_funded, amount)?;
                let balance = self.accounts.entry(account).or_default();
                balance.available = add(balance.available, amount)?;
                events.push(Event::AccountFunded { account, amount });
            }
            Command::RegisterOffer {
                id,
                worker,
                provider,
                price_per_second,
            } => {
                positive(price_per_second)?;
                if self.offers.contains_key(&id) {
                    return Err(ExchangeError::DuplicateOffer);
                }
                if self.workers.contains_key(&worker) {
                    return Err(ExchangeError::WorkerAlreadyRegistered);
                }
                let arrival = self.next_sequence()?;
                self.accounts.entry(provider).or_default();
                self.workers.insert(worker, id);
                self.offers.insert(
                    id,
                    Offer {
                        id,
                        worker,
                        provider,
                        price_per_second,
                        arrival,
                        state: OfferState::Available,
                    },
                );
                events.push(Event::OfferRegistered(id));
                self.match_jobs(now, &mut events)?;
            }
            Command::SubmitJob {
                id,
                buyer,
                max_price_per_second,
                duration_seconds,
            } => {
                positive(max_price_per_second)?;
                positive(duration_seconds)?;
                if self.jobs.contains_key(&id) {
                    return Err(ExchangeError::DuplicateJob);
                }
                let hold = multiply(max_price_per_second, duration_seconds)?;
                let balance = self.accounts.entry(buyer).or_default();
                if balance.available < hold {
                    return Err(ExchangeError::InsufficientFunds);
                }
                balance.available -= hold;
                balance.reserved = add(balance.reserved, hold)?;
                let arrival = self.next_sequence()?;
                self.jobs.insert(
                    id,
                    Job {
                        id,
                        buyer,
                        max_price_per_second,
                        duration_seconds,
                        held_microcredits: hold,
                        arrival,
                        state: JobState::Queued,
                    },
                );
                events.push(Event::JobQueued(id));
                self.match_jobs(now, &mut events)?;
            }
            Command::CancelJob { id, buyer } => {
                let job = self.jobs.get_mut(&id).ok_or(ExchangeError::UnknownJob)?;
                if job.buyer != buyer {
                    return Err(ExchangeError::WrongBuyer);
                }
                if job.state != JobState::Queued {
                    return Err(ExchangeError::JobNotQueued);
                }
                let balance = self.accounts.get_mut(&buyer).expect("funded buyer exists");
                balance.available = add(balance.available, job.held_microcredits)?;
                balance.reserved -= job.held_microcredits;
                job.held_microcredits = 0;
                job.state = JobState::Cancelled;
                events.push(Event::JobCancelled(id));
            }
        }
        Ok(events)
    }

    fn match_jobs(&mut self, now: u64, events: &mut Vec<Event>) -> Result<(), ExchangeError> {
        let mut queued: Vec<_> = self
            .jobs
            .values()
            .filter(|job| job.state == JobState::Queued)
            .map(|job| (job.arrival, job.id))
            .collect();
        queued.sort_unstable();
        for (_, job_id) in queued {
            let job = &self.jobs[&job_id];
            let best = self
                .offers
                .values()
                .filter(|offer| {
                    offer.state == OfferState::Available
                        && offer.price_per_second <= job.max_price_per_second
                })
                .min_by_key(|offer| (offer.price_per_second, offer.arrival))
                .map(|offer| (offer.id, offer.price_per_second));
            let Some((offer_id, price_per_second)) = best else {
                continue;
            };
            let job = self.jobs.get_mut(&job_id).expect("queued job exists");
            let hold = multiply(price_per_second, job.duration_seconds)?;
            let release = job.held_microcredits - hold;
            let balance = self.accounts.get_mut(&job.buyer).expect("funded buyer exists");
            balance.reserved -= release;
            balance.available = add(balance.available, release)?;
            job.held_microcredits = hold;
            job.state = JobState::Reserved { offer_id };
            self.offers.get_mut(&offer_id).expect("offer exists").state =
                OfferState::Reserved { job_id };
            let reservation = Reservation {
                job_id,
                offer_id,
                price_per_second,
                duration_seconds: job.duration_seconds,
                held_microcredits: hold,
                created_at: now,
            };
            self.reservations.insert(job_id, reservation.clone());
            events.push(Event::JobReserved(reservation));
        }
        Ok(())
    }
}

fn positive(value: u64) -> Result<(), ExchangeError> {
    if value == 0 {
        Err(ExchangeError::ZeroValue)
    } else {
        Ok(())
    }
}

fn add(left: u64, right: u64) -> Result<u64, ExchangeError> {
    left.checked_add(right).ok_or(ExchangeError::ArithmeticOverflow)
}

fn multiply(left: u64, right: u64) -> Result<u64, ExchangeError> {
    left.checked_mul(right).ok_or(ExchangeError::ArithmeticOverflow)
}

#[cfg(test)]
mod internal_tests {
    use super::*;

    #[test]
    fn sequence_overflow_is_atomic() {
        let mut exchange = Exchange {
            sequence: u64::MAX,
            ..Exchange::default()
        };
        let before = exchange.clone();
        let result = exchange.execute(
            Command::RegisterOffer {
                id: OfferId(1),
                worker: WorkerId(1),
                provider: AccountId(1),
                price_per_second: 1,
            },
            0,
        );
        assert_eq!(result, Err(ExchangeError::ArithmeticOverflow));
        assert_eq!(exchange, before);
    }
}
