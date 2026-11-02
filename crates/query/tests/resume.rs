//! A resume position gives exactly the remainder.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{Scratch, all_seen, drain, rig, sample, seen, store, target};
use norn_core::{Sample, Value, ValueKind};
use norn_query::{
    Filter, Function, Order, Output, QueryError, QueryLimits, QuerySpec, Reduction, TimeRange,
};

fn signals(n: u64) -> Vec<norn_query::Target> {
    (0..n).map(|s| target(s, ValueKind::F64)).collect()
}

fn spec(n: u64, output: Output) -> QuerySpec {
    QuerySpec {
        targets: signals(n),
        range: TimeRange::default(),
        filter: Filter::default(),
        output,
        resume: None,
    }
}

fn data(n: u64, each: i64) -> Vec<Sample> {
    let row = |s: u64| (0..each).map(move |t| sample(s, t * 10, Value::F64(t as f64)));
    (0..n).flat_map(row).collect()
}

fn tiny() -> QueryLimits {
    let mut limits = QueryLimits::default();
    limits.page_results = 50;
    limits.slice_bytes = 16 << 10;
    limits
}

#[test]
fn a_resume_token_gives_exactly_the_remainder_in_every_shape() {
    let s = Scratch::new();
    store(&s.dir(), &[data(3, 700)], 90);
    let r = rig(&s.0, 1 << 30, tiny());
    for order in [Order::BySignal, Order::ByTime] {
        let output = Output::Samples(order);
        let whole = all_seen(&r.engine, spec(3, output));
        assert_eq!(whole.len(), 2_100);
        let mut query = r.engine.open(spec(3, output)).unwrap();
        let mut sofar = 0;
        while let Some(page) = query.next_page().unwrap() {
            sofar += seen(&page).len();
            let token = page.position.encode();
            let parsed = norn_query::Position::parse(&token).unwrap();
            let rest = QuerySpec {
                resume: Some(parsed),
                ..spec(3, output)
            };
            assert_eq!(
                all_seen(&r.engine, rest),
                whole[sofar..],
                "{order:?} at {sofar}"
            );
        }
    }
}

#[test]
fn a_resume_token_gives_exactly_the_remaining_windows() {
    let s = Scratch::new();
    store(&s.dir(), &[data(2, 300)], 40);
    let mut limits = tiny();
    limits.page_results = 3;
    let r = rig(&s.0, 1 << 30, limits);
    for function in [Function::Count, Function::Latest] {
        let window = (function != Function::Latest).then_some(100);
        let output = Output::Aggregates(Reduction { function, window });
        let pages = |spec| {
            let mut q = r.engine.open(spec).unwrap();
            drain(&mut q).unwrap()
        };
        let rows = |ps: &[norn_query::Page]| -> Vec<String> {
            ps.iter()
                .map(|p| format!("{:?}", p.results))
                .collect::<Vec<_>>()
        };
        let whole = pages(spec(2, output));
        for (n, page) in whole.iter().enumerate() {
            let resume = Some(page.position.clone());
            let rest = pages(QuerySpec {
                resume,
                ..spec(2, output)
            });
            assert_eq!(rows(&rest), rows(&whole[n + 1..]), "{function:?} after {n}");
        }
    }
}

#[test]
fn a_position_of_the_wrong_shape_is_refused() {
    let s = Scratch::new();
    let r = rig(&s.0, 1 << 30, tiny());
    let resume = norn_query::Position::parse("w.1.x");
    let wrong = QuerySpec {
        resume,
        ..spec(1, Output::Samples(Order::BySignal))
    };
    assert!(matches!(r.engine.open(wrong), Err(QueryError::BadPosition)));
}
