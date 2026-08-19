use std::time::Instant;

use chronosdb::engine::DurableTransactionalHeap;

fn run_insert_trial(count: usize) -> f64 {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    let tx = db.begin().unwrap();

    let start = Instant::now();

    for i in 0..count {
        db.insert(tx.id(), format!("row-{i:08}").into_bytes())
            .unwrap();
    }

    db.commit(tx.id()).unwrap();

    start.elapsed().as_secs_f64()
}

fn recovery_trial(count: usize, compact: bool) -> f64 {
    let directory = tempfile::tempdir().unwrap();

    let heap_path = directory.path().join("table.heap");

    let wal_path = directory.path().join("chronos.wal");

    {
        let mut db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

        for i in 0..count {
            let tx = db.begin().unwrap();

            db.insert(tx.id(), format!("row-{i:08}").into_bytes())
                .unwrap();

            db.commit(tx.id()).unwrap();
        }

        if compact {
            db.checkpoint_and_compact().unwrap();
        }
    }

    let start = Instant::now();

    let _db = DurableTransactionalHeap::open(&heap_path, &wal_path).unwrap();

    start.elapsed().as_secs_f64()
}

fn main() {
    println!("workload,count,seconds,ops_per_second");

    for count in [1_000usize, 5_000] {
        let seconds = run_insert_trial(count);

        println!("insert,{count},{seconds:.6},{:.2}", count as f64 / seconds,);
    }

    for count in [100usize, 1_000, 5_000] {
        let full = recovery_trial(count, false);

        println!("recovery_full,{count},{full:.6},{:.2}", count as f64 / full,);

        let compact = recovery_trial(count, true);

        println!(
            "recovery_compacted,{count},{compact:.6},{:.2}",
            count as f64 / compact,
        );
    }
}
