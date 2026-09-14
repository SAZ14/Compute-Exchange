use exchange_core::{AccountId, Command, Exchange, JobId, OfferId, WorkerId};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut exchange = Exchange::new();
    println!("COMPUTE EXCHANGE | demo-compute | simulated microcredits");
    let commands = [
        Command::Fund {
            account: AccountId(1),
            amount: 1_000,
        },
        Command::Fund {
            account: AccountId(2),
            amount: 1_000,
        },
        Command::RegisterOffer {
            id: OfferId(30),
            worker: WorkerId(1),
            provider: AccountId(101),
            price_per_second: 8,
        },
        Command::RegisterOffer {
            id: OfferId(20),
            worker: WorkerId(2),
            provider: AccountId(102),
            price_per_second: 3,
        },
        Command::RegisterOffer {
            id: OfferId(10),
            worker: WorkerId(3),
            provider: AccountId(103),
            price_per_second: 3,
        },
        Command::SubmitJob {
            id: JobId(1),
            buyer: AccountId(1),
            max_price_per_second: 5,
            duration_seconds: 10,
        },
        Command::SubmitJob {
            id: JobId(2),
            buyer: AccountId(2),
            max_price_per_second: 5,
            duration_seconds: 10,
        },
        Command::SubmitJob {
            id: JobId(3),
            buyer: AccountId(1),
            max_price_per_second: 2,
            duration_seconds: 10,
        },
        Command::SubmitJob {
            id: JobId(4),
            buyer: AccountId(2),
            max_price_per_second: 10,
            duration_seconds: 10,
        },
        Command::CancelJob {
            id: JobId(3),
            buyer: AccountId(1),
        },
    ];
    for (now, command) in commands.into_iter().enumerate() {
        for event in exchange.execute(command, now as u64)? {
            println!("{now:02}: {event:?}");
        }
    }
    let before = exchange.clone();
    let rejected = exchange.execute(
        Command::SubmitJob {
            id: JobId(5),
            buyer: AccountId(1),
            max_price_per_second: 1_000,
            duration_seconds: 10,
        },
        10,
    );
    assert!(rejected.is_err());
    assert_eq!(exchange, before);
    println!("\nInsufficient funding rejected atomically: {rejected:?}");
    println!("\nFinal balances: {:#?}", exchange.accounts());
    println!("Final jobs: {:#?}", exchange.jobs());
    println!("Reservations: {:#?}", exchange.reservations());
    println!("\nNext milestone: durable replay and settlement. No tasks executed yet.");
    Ok(())
}
