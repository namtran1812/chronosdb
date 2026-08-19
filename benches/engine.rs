use std::hint::black_box;
use std::time::Instant;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use chronosdb::engine::DurableTransactionalHeap;

fn bench_sequential_inserts(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_insert");

    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(10));

    for count in [1_000usize, 10_000] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter(|| {
                let directory = tempfile::tempdir().unwrap();

                let heap = directory.path().join("table.heap");

                let wal = directory.path().join("chronos.wal");

                let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

                let tx = db.begin().unwrap();

                for i in 0..count {
                    db.insert(tx.id(), black_box(format!("row-{i:08}").into_bytes()))
                        .unwrap();
                }

                db.commit(tx.id()).unwrap();
            });
        });
    }

    group.finish();
}

fn bench_visible_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("visible_scan");

    group.sample_size(20);
    group.measurement_time(std::time::Duration::from_secs(10));

    for count in [1_000usize, 10_000] {
        let directory = tempfile::tempdir().unwrap();

        let heap = directory.path().join("table.heap");

        let wal = directory.path().join("chronos.wal");

        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        let writer = db.begin().unwrap();

        for i in 0..count {
            db.insert(writer.id(), format!("row-{i:08}").into_bytes())
                .unwrap();
        }

        db.commit(writer.id()).unwrap();

        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                let reader = db.begin().unwrap();

                let rows = db.visible_scan(&reader).unwrap();

                black_box(rows.len());

                db.abort(reader.id()).unwrap();
            });
        });
    }

    group.finish();
}

fn bench_update_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("update_workload");

    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(10));

    for count in [100usize, 1_000] {
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter(|| {
                let directory = tempfile::tempdir().unwrap();

                let heap = directory.path().join("table.heap");

                let wal = directory.path().join("chronos.wal");

                let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

                let creator = db.begin().unwrap();

                let mut records = Vec::with_capacity(count);

                for i in 0..count {
                    records.push(
                        db.insert(creator.id(), format!("base-{i}").into_bytes())
                            .unwrap(),
                    );
                }

                db.commit(creator.id()).unwrap();

                let updater = db.begin().unwrap();

                for (i, record) in records.iter().enumerate() {
                    db.update(
                        &updater,
                        *record,
                        black_box(format!("updated-{i}").into_bytes()),
                    )
                    .unwrap();
                }

                db.commit(updater.id()).unwrap();
            });
        });
    }

    group.finish();
}

fn recovery_trial(transactions: usize, compact: bool) -> std::time::Duration {
    let directory = tempfile::tempdir().unwrap();

    let heap = directory.path().join("table.heap");

    let wal = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

        for i in 0..transactions {
            let tx = db.begin().unwrap();

            db.insert(tx.id(), format!("transaction-{i}").into_bytes())
                .unwrap();

            db.commit(tx.id()).unwrap();
        }

        if compact {
            db.checkpoint_and_compact().unwrap();
        }
    }

    let start = Instant::now();

    let _db = DurableTransactionalHeap::open(&heap, &wal).unwrap();

    start.elapsed()
}

fn bench_recovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("recovery");

    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(10));

    for transactions in [100usize, 1_000] {
        group.bench_function(format!("full_wal/{transactions}"), |b| {
            b.iter(|| {
                black_box(recovery_trial(transactions, false));
            });
        });

        group.bench_function(format!("checkpoint_compacted/{transactions}"), |b| {
            b.iter(|| {
                black_box(recovery_trial(transactions, true));
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_sequential_inserts,
    bench_visible_scan,
    bench_update_workload,
    bench_recovery,
);

criterion_main!(benches);
